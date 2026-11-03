//! PostgreSQL, over `tokio-postgres`.
//!
//! A connection is to one database, so the tree starts at its schemas. Rows
//! are read through the simple query protocol, which streams: reading stops
//! at the row limit and the rest is cancelled on the server rather than
//! drained. Values arrive as text, so types come from a describe of the
//! statement first (when it is a single statement).
//!
//! Queries borrow a connection from a small pool, last-used first, so a
//! person working in one tab keeps one server session (session settings and
//! temporary tables survive between runs); only queries that overlap in time
//! open another.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytes::BytesMut;
use futures_util::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring as ring_provider, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_platform_verifier::BuilderVerifierExt;
use serde_json::{json, Value};
use tokio_postgres::config::SslMode;
use tokio_postgres::types::{to_sql_checked, Format, IsNull, ToSql, Type};
use tokio_postgres::{CancelToken, Client, Config, NoTls, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;
use uuid::Uuid;

use super::meta::{self, ConstraintKind, ConstraintMeta, Ddl, ForeignRef, IncomingRef, IndexMeta, SearchHit, TableMeta};
use super::{
    join_with, safety, update_pieces, Column, ColumnInfo, ColumnKind, ConnectSpec, DbError, DbResult, NodeKind, QueryResult,
    RowEdit, TableInfo, TlsMode, TreeNode, CONNECT_TIMEOUT, MAX_CELL_BYTES,
};

/// Connections kept for reuse. More can be open while queries overlap.
const MAX_IDLE: usize = 4;
/// Largest whole number a JSON reader (a browser) holds exactly.
const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;
/// The database a connection starts in when none is given, like `psql`'s usual one.
const DEFAULT_DATABASE: &str = "postgres";

#[derive(Clone)]
enum Tls {
    Off,
    On(MakeRustlsConnect),
}

pub struct PgConn {
    cfg: Config,
    tls: Tls,
    addr: String,
    idle: Mutex<Vec<Client>>,
    /// Cancel handle of each running query.
    running: Mutex<HashMap<Uuid, CancelToken>>,
    /// Queries a caller asked to stop, so their failure reads as a cancel.
    cancelled: Mutex<HashSet<Uuid>>,
}

// -- errors -----------------------------------------------------------------

/// The server's message with its detail and hint, or the whole chain of a
/// lower-level failure (TLS, socket) so the real reason is visible.
fn describe(e: &tokio_postgres::Error) -> String {
    if let Some(db) = e.as_db_error() {
        let mut s = format!("{} ({})", db.message(), db.code().code());
        if let Some(d) = db.detail() {
            s.push_str(&format!("\nDETAIL: {d}"));
        }
        if let Some(h) = db.hint() {
            s.push_str(&format!("\nHINT: {h}"));
        }
        return s;
    }
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(c) = src {
        let t = c.to_string();
        if !s.contains(&t) {
            s.push_str(": ");
            s.push_str(&t);
        }
        src = c.source();
    }
    s
}

fn server_err(e: tokio_postgres::Error) -> DbError {
    DbError::Server(describe(&e))
}

/// "cannot insert multiple commands into a prepared statement": the text had
/// more than one statement after all (the reader can be fooled by exotic syntax).
fn is_multi_command(e: &tokio_postgres::Error) -> bool {
    e.as_db_error().is_some_and(|d| d.message().contains("multiple commands"))
}

// -- TLS --------------------------------------------------------------------

/// Accepts any certificate: the connection is encrypted, but the server isn't
/// identified. Only for the "don't verify" setting.
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_for(mode: TlsMode) -> DbResult<Tls> {
    if mode == TlsMode::Disable {
        return Ok(Tls::Off);
    }
    let provider = Arc::new(ring_provider::default_provider());
    let tls_err = |e: rustls::Error| DbError::Invalid(format!("could not set up TLS: {e}"));
    let builder = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(tls_err)?;
    let config = match mode {
        TlsMode::Require => builder.dangerous().with_custom_certificate_verifier(Arc::new(AcceptAny(provider))).with_no_client_auth(),
        // The operating system's trust store, so a company CA that is already installed works.
        _ => builder.with_platform_verifier().map_err(tls_err)?.with_no_client_auth(),
    };
    Ok(Tls::On(MakeRustlsConnect::new(config)))
}

fn config_for(spec: &ConnectSpec) -> DbResult<Config> {
    if spec.host.starts_with('/') {
        return Err(DbError::Invalid("Unix sockets aren't supported; enter a host name or address".into()));
    }
    if spec.user.trim().is_empty() {
        return Err(DbError::Invalid("enter a user name".into()));
    }
    let mut c = Config::new();
    c.host(&spec.host)
        // The tunnel's loopback port when there is one; the server's name is kept for the certificate check.
        .port(spec.tunnel_port.unwrap_or(spec.port))
        .user(&spec.user)
        .dbname(spec.database.as_deref().filter(|d| !d.is_empty()).unwrap_or(DEFAULT_DATABASE))
        .application_name("SSHVault")
        .connect_timeout(CONNECT_TIMEOUT)
        .ssl_mode(if spec.tls == TlsMode::Disable { SslMode::Disable } else { SslMode::Require });
    if spec.tunnel_port.is_some() {
        c.hostaddr(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    if spec.read_only() {
        c.options("-c default_transaction_read_only=on");
    }
    if let Some(pw) = spec.password.as_deref().filter(|p| !p.is_empty()) {
        c.password(pw);
    }
    Ok(c)
}

// -- connection ---------------------------------------------------------------

impl PgConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        let me = Self {
            cfg: config_for(spec)?,
            tls: tls_for(spec.tls)?,
            addr: format!("{}:{}", spec.host, spec.port),
            idle: Mutex::default(),
            running: Mutex::default(),
            cancelled: Mutex::default(),
        };
        // Fail now, with the real reason, rather than on the first query.
        let first = me.open().await?;
        me.release(first);
        Ok(me)
    }

    async fn open(&self) -> DbResult<Client> {
        let attempt = async {
            match &self.tls {
                Tls::Off => {
                    let (c, conn) = self.cfg.connect(NoTls).await?;
                    tokio::spawn(async move {
                        let _ = conn.await;
                    });
                    Ok::<_, tokio_postgres::Error>(c)
                }
                Tls::On(m) => {
                    let (c, conn) = self.cfg.connect(m.clone()).await?;
                    tokio::spawn(async move {
                        let _ = conn.await;
                    });
                    Ok(c)
                }
            }
        };
        match tokio::time::timeout(CONNECT_TIMEOUT + std::time::Duration::from_secs(5), attempt).await {
            Err(_) => Err(DbError::Timeout(self.addr.clone())),
            Ok(Err(e)) if e.as_db_error().is_some() => Err(server_err(e)),
            Ok(Err(e)) => Err(DbError::Connect { addr: self.addr.clone(), reason: describe(&e) }),
            Ok(Ok(c)) => Ok(c),
        }
    }

    /// The most recently used idle connection, or a new one.
    async fn acquire(&self) -> DbResult<Client> {
        loop {
            let next = self.idle.lock().unwrap_or_else(|p| p.into_inner()).pop();
            match next {
                Some(c) if !c.is_closed() => return Ok(c),
                Some(_) => continue,
                None => return self.open().await,
            }
        }
    }

    fn release(&self, c: Client) {
        if c.is_closed() {
            return;
        }
        let mut idle = self.idle.lock().unwrap_or_else(|p| p.into_inner());
        if idle.len() < MAX_IDLE {
            idle.push(c);
        }
    }

    pub async fn close(&self) {
        self.idle.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    pub async fn server_version(&self) -> DbResult<String> {
        let c = self.acquire().await?;
        let row = c.query_one("SHOW server_version", &[]).await;
        self.release(c);
        Ok(row.map_err(server_err)?.get::<_, String>(0))
    }

    async fn send_cancel(&self, token: &CancelToken) -> Result<(), tokio_postgres::Error> {
        match &self.tls {
            Tls::Off => token.cancel_query(NoTls).await,
            Tls::On(m) => token.cancel_query(m.clone()).await,
        }
    }

    pub async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        let token = self.running.lock().unwrap_or_else(|p| p.into_inner()).get(&qid).cloned();
        let Some(token) = token else { return Ok(()) };
        self.cancelled.lock().unwrap_or_else(|p| p.into_inner()).insert(qid);
        self.send_cancel(&token).await.map_err(server_err)
    }

    // -- queries --------------------------------------------------------------

    pub async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let client = self.acquire().await?;
        self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(qid, client.cancel_token());

        let outcome = self.run(&client, sql, limit).await;

        self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&qid);
        let was_cancelled = self.cancelled.lock().unwrap_or_else(|p| p.into_inner()).remove(&qid);
        if outcome.is_err() || was_cancelled {
            // A failed script can leave an explicit transaction open and aborted.
            let _ = client.simple_query("ROLLBACK").await;
        }
        self.release(client);
        if was_cancelled {
            return Err(DbError::Cancelled);
        }
        let mut result = outcome.map_err(server_err)?;
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(result)
    }

    async fn run(&self, c: &Client, sql: &str, limit: usize) -> Result<QueryResult, tokio_postgres::Error> {
        // Column types, by describing the statement (this runs nothing).
        let mut meta: Option<Vec<Column>> = None;
        if safety::is_single_statement(sql) {
            match c.prepare(sql).await {
                Ok(st) => meta = Some(st.columns().iter().map(|col| describe_column(col.name(), col.type_())).collect()),
                Err(e) if is_multi_command(&e) => {}
                Err(e) => return Err(e),
            }
        }

        let stream = c.simple_query_raw(sql).await?;
        futures_util::pin_mut!(stream);
        let mut out = QueryResult { columns: meta.clone().unwrap_or_default(), ..Default::default() };
        let mut sets = 0;
        let mut cut = false;
        let token = c.cancel_token();
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                // After we cancelled to stop at the limit, the server's "cancelled" is expected.
                Err(_) if cut => break,
                Err(e) => return Err(e),
            };
            match msg {
                SimpleQueryMessage::RowDescription(cols) => {
                    sets += 1;
                    if sets == 1 && meta.is_none() {
                        out.columns = cols.iter().map(|c| Column { name: c.name().into(), data_type: "text".into(), kind: ColumnKind::Text }).collect();
                    }
                }
                // Only the first result set is shown, as with MySQL.
                SimpleQueryMessage::Row(row) if sets == 1 && !cut => {
                    if out.rows.len() >= limit {
                        out.truncated = true;
                        cut = true;
                        // Stop the server producing rows we won't read.
                        let _ = self.send_cancel(&token).await;
                        continue;
                    }
                    out.rows.push(out.columns.iter().enumerate().map(|(i, col)| to_json(row.get(i), col)).collect());
                }
                SimpleQueryMessage::CommandComplete(n) if sets == 0 => out.affected_rows = Some(n),
                _ => {}
            }
        }
        if !out.columns.is_empty() {
            out.affected_rows = None;
        }
        Ok(out)
    }

    // -- browsing -------------------------------------------------------------

    pub async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        let c = self.acquire().await?;
        let out = self.children_with(&c, path).await;
        self.release(c);
        out
    }

    async fn children_with(&self, c: &Client, path: &[String]) -> DbResult<Vec<TreeNode>> {
        match path {
            [] => {
                let rows = c
                    .query("SELECT nspname::text FROM pg_namespace WHERE nspname !~ '^pg_(temp|toast)' ORDER BY 1", &[])
                    .await
                    .map_err(server_err)?;
                let mut nodes: Vec<TreeNode> = rows
                    .iter()
                    .map(|r| {
                        let name: String = r.get(0);
                        let system = name == "information_schema" || name.starts_with("pg_");
                        TreeNode { name, kind: NodeKind::Schema, detail: system.then(|| "system".to_string()), expandable: true }
                    })
                    .collect();
                // Your own schemas first, the server's own last.
                nodes.sort_by_key(|n| (n.detail.is_some(), n.name.to_lowercase()));
                Ok(nodes)
            }
            [schema] => {
                let rows = c
                    .query(
                        "SELECT c.relname::text, c.relkind::text, c.reltuples::bigint \
                         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m','f') ORDER BY c.relname",
                        &[schema],
                    )
                    .await
                    .map_err(server_err)?;
                let nodes = rows
                    .iter()
                    .map(|r| {
                        let (name, relkind, est): (String, String, i64) = (r.get(0), r.get(1), r.get(2));
                        let (kind, detail) = match relkind.as_str() {
                            "v" => (NodeKind::View, Some("view".to_string())),
                            "m" => (NodeKind::View, Some("materialized view".to_string())),
                            "f" => (NodeKind::Table, Some("foreign".to_string())),
                            "p" => (NodeKind::Table, Some("partitioned".to_string())),
                            // reltuples is -1 until the table has been analysed.
                            _ => (NodeKind::Table, (est >= 0).then(|| format!("~{est} rows"))),
                        };
                        TreeNode { name, kind, detail, expandable: true }
                    })
                    .collect::<Vec<_>>();
                let mut nodes = nodes;
                // Routines and sequences come after the tables: the window groups them by kind.
                let routines = c
                    .query(
                        "SELECT p.proname::text, p.prokind::text, pg_get_function_identity_arguments(p.oid) \
                         FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                         WHERE n.nspname = $1 AND p.prokind IN ('f','p') \
                           AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e') \
                         ORDER BY p.proname, 3",
                        &[schema],
                    )
                    .await
                    .map_err(server_err)?;
                nodes.extend(routines.iter().map(|r| {
                    let (name, kind, args): (String, String, String) = (r.get(0), r.get(1), r.get(2));
                    TreeNode {
                        name,
                        kind: if kind == "p" { NodeKind::Procedure } else { NodeKind::Function },
                        detail: Some(format!("({args})")),
                        expandable: false,
                    }
                }));
                let seqs = c
                    .query(
                        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relkind = 'S' ORDER BY c.relname",
                        &[schema],
                    )
                    .await
                    .map_err(server_err)?;
                nodes.extend(seqs.iter().map(|r| TreeNode { name: r.get(0), kind: NodeKind::Sequence, detail: None, expandable: false }));
                Ok(nodes)
            }
            [schema, table] => {
                let info = self.table_info_with(c, schema, table).await?;
                let mut nodes: Vec<TreeNode> = info
                    .columns
                    .iter()
                    .map(|col| TreeNode {
                        name: col.name.clone(),
                        kind: NodeKind::Column,
                        detail: Some(format!(
                            "{}{}{}",
                            col.data_type,
                            if col.primary_key { " · PK" } else { "" },
                            if col.nullable { "" } else { " · not null" }
                        )),
                        expandable: false,
                    })
                    .collect();
                let idx = c
                    .query(
                        "SELECT ic.relname::text, i.indisunique, string_agg(a.attname::text, ',' ORDER BY k.ord) \
                         FROM pg_index i \
                         JOIN pg_class ic ON ic.oid = i.indexrelid \
                         JOIN pg_class c ON c.oid = i.indrelid \
                         JOIN pg_namespace n ON n.oid = c.relnamespace \
                         CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
                         WHERE n.nspname = $1 AND c.relname = $2 \
                         GROUP BY ic.relname, i.indisunique ORDER BY 1",
                        &[schema, table],
                    )
                    .await
                    .map_err(server_err)?;
                nodes.extend(idx.iter().map(|r| {
                    let (name, unique, cols): (String, bool, String) = (r.get(0), r.get(1), r.get(2));
                    TreeNode {
                        name,
                        kind: NodeKind::Index,
                        detail: Some(format!("{}({cols})", if unique { "unique " } else { "" })),
                        expandable: false,
                    }
                }));
                let trg = c
                    .query(
                        "SELECT t.tgname::text, pg_get_triggerdef(t.oid) FROM pg_trigger t \
                         JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = $1 AND c.relname = $2 AND NOT t.tgisinternal ORDER BY t.tgname",
                        &[schema, table],
                    )
                    .await
                    .map_err(server_err)?;
                nodes.extend(trg.iter().map(|r| {
                    let (name, def): (String, String) = (r.get(0), r.get(1));
                    // "CREATE TRIGGER x BEFORE INSERT ON t …": keep what says when it fires.
                    let when = def.split(" ON ").next().and_then(|h| h.splitn(4, ' ').nth(3)).map(str::to_string);
                    TreeNode { name, kind: NodeKind::Trigger, detail: when, expandable: false }
                }));
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    pub async fn table_info(&self, schema: &str, table: &str) -> DbResult<TableInfo> {
        let c = self.acquire().await?;
        let out = self.table_info_with(&c, schema, table).await;
        self.release(c);
        out
    }

    async fn table_info_with(&self, c: &Client, schema: &str, table: &str) -> DbResult<TableInfo> {
        let cols = c
            .query(
                "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, pg_get_expr(d.adbin, d.adrelid) \
                 FROM pg_attribute a \
                 JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &[&schema, &table],
            )
            .await
            .map_err(server_err)?;
        let pk: Vec<String> = c
            .query(
                "SELECT a.attname::text FROM pg_index i \
                 JOIN pg_class c ON c.oid = i.indrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                 JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
                 WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2 ORDER BY k.ord",
                &[&schema, &table],
            )
            .await
            .map_err(server_err)?
            .iter()
            .map(|r| r.get(0))
            .collect();
        Ok(TableInfo {
            columns: cols
                .iter()
                .map(|r| {
                    let name: String = r.get(0);
                    ColumnInfo { primary_key: pk.contains(&name), name, data_type: r.get(1), nullable: r.get(2), default: r.get(3) }
                })
                .collect(),
            primary_key: pk,
        })
    }

    pub async fn table_meta(&self, schema: &str, table: &str) -> DbResult<TableMeta> {
        let c = self.acquire().await?;
        let out = self.table_meta_with(&c, schema, table).await;
        self.release(c);
        out
    }

    async fn table_meta_with(&self, c: &Client, schema: &str, table: &str) -> DbResult<TableMeta> {
        let Some(head) = c
            .query_opt(
                "SELECT c.relkind::text, obj_description(c.oid, 'pg_class'), c.reltuples::bigint, \
                        CASE WHEN c.relkind IN ('r','p','m') THEN pg_total_relation_size(c.oid) END, c.oid::int8 \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2",
                &[&schema, &table],
            )
            .await
            .map_err(server_err)?
        else {
            return Err(DbError::Invalid(format!("{schema}.{table} was not found; it may have been dropped")));
        };
        let (relkind, comment, est, size, oid): (String, Option<String>, i64, Option<i64>, i64) =
            (head.get(0), head.get(1), head.get(2), head.get(3), head.get(4));
        let kind = match relkind.as_str() {
            "v" => "view",
            "m" => "materialized view",
            "f" => "foreign table",
            "p" => "partitioned table",
            _ => "table",
        };
        let oid = oid as u32;

        let indexes: Vec<IndexMeta> = c
            .query(
                "SELECT ic.relname::text, i.indisunique, i.indisprimary, am.amname::text, pg_get_indexdef(ic.oid), \
                        (SELECT array_agg(a.attname::text ORDER BY k.ord) \
                           FROM unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                           JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum WHERE k.attnum > 0), \
                        pg_relation_size(ic.oid) \
                 FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid JOIN pg_am am ON am.oid = ic.relam \
                 WHERE i.indrelid = $1::oid ORDER BY i.indisprimary DESC, ic.relname",
                &[&oid],
            )
            .await
            .map_err(server_err)?
            .iter()
            .map(|r| IndexMeta {
                name: r.get(0),
                unique: r.get(1),
                primary: r.get(2),
                method: Some(r.get(3)),
                definition: Some(r.get(4)),
                columns: r.get::<_, Option<Vec<String>>>(5).unwrap_or_default(),
                size_bytes: Some(r.get(6)),
            })
            .collect();

        let constraints: Vec<ConstraintMeta> = c
            .query(
                "SELECT con.conname::text, con.contype::text, pg_get_constraintdef(con.oid), \
                        (SELECT array_agg(a.attname::text ORDER BY u.ord) FROM unnest(con.conkey) WITH ORDINALITY AS u(attnum, ord) \
                           JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = u.attnum), \
                        fn.nspname::text, fc.relname::text, \
                        (SELECT array_agg(a.attname::text ORDER BY u.ord) FROM unnest(con.confkey) WITH ORDINALITY AS u(attnum, ord) \
                           JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = u.attnum), \
                        con.confupdtype::text, con.confdeltype::text \
                 FROM pg_constraint con \
                 LEFT JOIN pg_class fc ON fc.oid = con.confrelid LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
                 WHERE con.conrelid = $1::oid AND con.contype IN ('p','f','u','c','x') \
                 ORDER BY con.contype = 'c', con.contype, con.conname",
                &[&oid],
            )
            .await
            .map_err(server_err)?
            .iter()
            .map(|r| {
                let ty: String = r.get(1);
                let kind = match ty.as_str() {
                    "p" => ConstraintKind::PrimaryKey,
                    "f" => ConstraintKind::ForeignKey,
                    "u" => ConstraintKind::Unique,
                    "x" => ConstraintKind::Exclusion,
                    _ => ConstraintKind::Check,
                };
                let references = (kind == ConstraintKind::ForeignKey).then(|| ForeignRef {
                    schema: r.get::<_, Option<String>>(4).unwrap_or_default(),
                    table: r.get::<_, Option<String>>(5).unwrap_or_default(),
                    columns: r.get::<_, Option<Vec<String>>>(6).unwrap_or_default(),
                    on_update: meta::pg_action(&r.get::<_, String>(7)),
                    on_delete: meta::pg_action(&r.get::<_, String>(8)),
                });
                ConstraintMeta {
                    name: r.get(0),
                    kind,
                    columns: r.get::<_, Option<Vec<String>>>(3).unwrap_or_default(),
                    definition: Some(r.get(2)),
                    references,
                }
            })
            .collect();

        let referenced_by: Vec<IncomingRef> = c
            .query(
                "SELECT con.conname::text, ns.nspname::text, src.relname::text, \
                        (SELECT array_agg(a.attname::text ORDER BY u.ord) FROM unnest(con.conkey) WITH ORDINALITY AS u(attnum, ord) \
                           JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = u.attnum), \
                        (SELECT array_agg(a.attname::text ORDER BY u.ord) FROM unnest(con.confkey) WITH ORDINALITY AS u(attnum, ord) \
                           JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = u.attnum), \
                        con.confupdtype::text, con.confdeltype::text \
                 FROM pg_constraint con JOIN pg_class src ON src.oid = con.conrelid JOIN pg_namespace ns ON ns.oid = src.relnamespace \
                 WHERE con.confrelid = $1::oid AND con.contype = 'f' ORDER BY ns.nspname, src.relname, con.conname",
                &[&oid],
            )
            .await
            .map_err(server_err)?
            .iter()
            .map(|r| IncomingRef {
                name: r.get(0),
                schema: r.get(1),
                table: r.get(2),
                columns: r.get::<_, Option<Vec<String>>>(3).unwrap_or_default(),
                referenced_columns: r.get::<_, Option<Vec<String>>>(4).unwrap_or_default(),
                on_update: meta::pg_action(&r.get::<_, String>(5)),
                on_delete: meta::pg_action(&r.get::<_, String>(6)),
            })
            .collect();

        let column_notes: Vec<(String, String)> = c
            .query(
                "SELECT a.attname::text, CASE WHEN a.attidentity = 'a' THEN 'identity (always)' \
                        WHEN a.attidentity = 'd' THEN 'identity (by default)' WHEN a.attgenerated = 's' THEN 'generated' END \
                 FROM pg_attribute a WHERE a.attrelid = $1::oid AND a.attnum > 0 AND NOT a.attisdropped \
                   AND (a.attidentity <> '' OR a.attgenerated <> '') ORDER BY a.attnum",
                &[&oid],
            )
            .await
            .map_err(server_err)?
            .iter()
            .filter_map(|r| r.get::<_, Option<String>>(1).map(|n| (r.get::<_, String>(0), n)))
            .collect();

        let ddl = match relkind.as_str() {
            "v" | "m" => {
                let def: Option<String> =
                    c.query_one("SELECT pg_get_viewdef($1::oid, true)", &[&oid]).await.map_err(server_err)?.get(0);
                def.map(|d| Ddl {
                    text: format!("CREATE {} {}.{} AS\n{}", if relkind == "m" { "MATERIALIZED VIEW" } else { "VIEW" }, meta_ident(schema), meta_ident(table), d.trim_end()),
                    reconstructed: true,
                })
            }
            _ => {
                let info = self.table_info_with(c, schema, table).await?;
                Some(Ddl { text: meta::pg_table_ddl(schema, table, &info, &column_notes, &constraints, &indexes), reconstructed: true })
            }
        };

        Ok(TableMeta {
            supported: true,
            kind: Some(kind.into()),
            comment,
            // reltuples is -1 until the table has been analysed.
            approx_rows: (est >= 0).then_some(est),
            size_bytes: size,
            indexes,
            constraints,
            referenced_by,
            column_notes,
            ddl,
        })
    }

    pub async fn search(&self, term: &str, limit: usize) -> DbResult<Vec<SearchHit>> {
        let pattern = format!("%{}%", meta::like_escape(term));
        let c = self.acquire().await?;
        let rows = c
            .query(
                "SELECT * FROM ((SELECT n.nspname::text, c.relname::text, c.relkind::text, NULL::text FROM pg_class c \
                    JOIN pg_namespace n ON n.oid = c.relnamespace \
                  WHERE c.relkind IN ('r','p','v','m','f','S') AND c.relname ILIKE $1 AND n.nspname !~ '^pg_(temp|toast)') \
                 UNION ALL \
                 (SELECT n.nspname::text, p.proname::text, p.prokind::text, '(' || pg_get_function_identity_arguments(p.oid) || ')' FROM pg_proc p \
                    JOIN pg_namespace n ON n.oid = p.pronamespace \
                  WHERE p.prokind IN ('f','p') AND p.proname ILIKE $1 AND n.nspname !~ '^pg_(temp|toast)' \
                    AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e')) \
                 ) AS s(schema_name, object_name, code, detail) \
                 ORDER BY schema_name IN ('information_schema', 'pg_catalog'), schema_name, object_name LIMIT $2",
                &[&pattern, &(limit as i64)],
            )
            .await;
        self.release(c);
        Ok(rows
            .map_err(server_err)?
            .iter()
            .map(|r| {
                let (schema, name, code, detail): (String, String, String, Option<String>) = (r.get(0), r.get(1), r.get(2), r.get(3));
                let (kind, detail) = match code.as_str() {
                    "v" => (NodeKind::View, Some("view".to_string())),
                    "m" => (NodeKind::View, Some("materialized view".to_string())),
                    "S" => (NodeKind::Sequence, None),
                    "f" if detail.is_some() => (NodeKind::Function, detail),
                    "p" if detail.is_some() => (NodeKind::Procedure, detail),
                    _ => (NodeKind::Table, None),
                };
                SearchHit { parent: vec![schema], name, kind, detail }
            })
            .collect())
    }

    pub async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        check_text(edit)?;
        let (pieces, cells) = update_pieces(edit, quote_ident, "");
        let sql = join_with(&pieces, |i| format!("${}", i + 1));
        let c = self.acquire().await?;
        let result = async {
            // The server infers each parameter's type from its column, and parses our text as that type.
            let st = c.prepare(&sql).await?;
            let values: Vec<TextParam> = cells.iter().map(|c| TextParam(c.value.as_deref())).collect();
            let params: Vec<&(dyn ToSql + Sync)> = values.iter().map(|v| v as &(dyn ToSql + Sync)).collect();
            c.execute(&st, &params).await
        }
        .await;
        self.release(c);
        result.map_err(server_err)
    }
}

/// A value sent in PostgreSQL's *text* parameter format, for any column type:
/// the server parses it with the type's own input function, as it would a
/// literal, but it travels as a bound parameter and is never part of the SQL.
/// (The client library otherwise sends binary, which needs the Rust type to
/// match the column exactly.)
#[derive(Debug)]
struct TextParam<'a>(Option<&'a str>);

impl ToSql for TextParam<'_> {
    fn to_sql(&self, _ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self.0 {
            None => Ok(IsNull::Yes),
            Some(s) => {
                out.extend_from_slice(s.as_bytes());
                Ok(IsNull::No)
            }
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Text
    }

    to_sql_checked!();
}

fn meta_ident(name: &str) -> String {
    quote_ident(name)
}

// -- text helpers ---------------------------------------------------------------

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A literal for display only. Uses `E'...'` when a backslash is present so
/// it reads the same whatever `standard_conforming_strings` is.
pub fn quote_literal(value: &Option<String>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(s) if s.contains('\\') => format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''")),
        Some(s) => format!("'{}'", s.replace('\'', "''")),
    }
}

/// PostgreSQL text can't hold a NUL character.
fn check_text(edit: &RowEdit) -> DbResult<()> {
    let bad = edit.changes.iter().chain(edit.key.iter()).any(|c| c.value.as_deref().is_some_and(|v| v.contains('\0')));
    if bad {
        return Err(DbError::Invalid("PostgreSQL text can't contain a NUL character".into()));
    }
    Ok(())
}

pub fn preview_update(edit: &RowEdit) -> DbResult<String> {
    check_text(edit)?;
    let (pieces, cells) = update_pieces(edit, quote_ident, "");
    Ok(join_with(&pieces, |i| quote_literal(&cells[i].value)))
}

fn kind_of(t: &Type) -> ColumnKind {
    match *t {
        Type::INT2 | Type::INT4 | Type::INT8 | Type::OID | Type::FLOAT4 | Type::FLOAT8 | Type::NUMERIC => ColumnKind::Number,
        Type::JSON | Type::JSONB => ColumnKind::Json,
        Type::DATE | Type::TIME | Type::TIMETZ | Type::TIMESTAMP | Type::TIMESTAMPTZ | Type::INTERVAL => ColumnKind::DateTime,
        Type::BYTEA => ColumnKind::Binary,
        Type::BOOL => ColumnKind::Other,
        _ => ColumnKind::Text,
    }
}

fn describe_column(name: &str, t: &Type) -> Column {
    Column { name: name.to_string(), data_type: t.name().to_string(), kind: kind_of(t) }
}

fn to_json(text: Option<&str>, col: &Column) -> Value {
    let Some(text) = text else { return Value::Null };
    match (col.kind, col.data_type.as_str()) {
        (ColumnKind::Number, "numeric") => Value::String(text.to_string()),
        (ColumnKind::Number, ty) => number(text, ty),
        (ColumnKind::Other, "bool") => match text {
            "t" => Value::Bool(true),
            "f" => Value::Bool(false),
            other => Value::String(other.to_string()),
        },
        (ColumnKind::Binary, _) => {
            // bytea arrives as \x followed by hex; show it the way MySQL's binary shows.
            let hex = text.strip_prefix("\\x").unwrap_or(text);
            let shown: String = hex.chars().take(512).collect();
            let mut out = format!("0x{shown}");
            if hex.len() > shown.len() {
                out.push_str(&format!("… ({} bytes)", hex.len() / 2));
            }
            Value::String(out)
        }
        _ => cut_text(text),
    }
}

fn number(text: &str, data_type: &str) -> Value {
    if let Ok(i) = text.parse::<i64>() {
        if i.abs() <= MAX_SAFE_INT {
            return json!(i);
        }
    } else if matches!(data_type, "float4" | "float8") {
        if let Ok(f) = text.parse::<f64>() {
            if f.is_finite() {
                return json!(f);
            }
        }
    }
    // Beyond 2^53, NaN, Infinity: keep the exact text.
    Value::String(text.to_string())
}

fn cut_text(text: &str) -> Value {
    if text.len() <= MAX_CELL_BYTES {
        return Value::String(text.to_string());
    }
    let mut end = MAX_CELL_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    json!({ "truncated": true, "preview": &text[..end], "bytes": text.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CellEdit;

    fn edit(changes: &[(&str, Option<&str>)], key: &[(&str, Option<&str>)]) -> RowEdit {
        let cell = |(c, v): &(&str, Option<&str>)| CellEdit { column: c.to_string(), value: v.map(str::to_string) };
        RowEdit { database: "shop".into(), table: "items".into(), key: key.iter().map(cell).collect(), changes: changes.iter().map(cell).collect() }
    }

    fn col(ty: Type) -> Column {
        describe_column("c", &ty)
    }

    #[test]
    fn quotes_names_and_values() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_literal(&None), "NULL");
        assert_eq!(quote_literal(&Some("it's".into())), "'it''s'");
        assert_eq!(quote_literal(&Some("a\\b".into())), "E'a\\\\b'");
        assert_eq!(quote_literal(&Some("a\\'b".into())), "E'a\\\\''b'");
    }

    #[test]
    fn preview_writes_the_values_in_without_a_limit_clause() {
        let e = edit(&[("name", Some("x'y")), ("note", None)], &[("id", Some("7"))]);
        assert_eq!(preview_update(&e).unwrap(), "UPDATE \"shop\".\"items\" SET \"name\" = 'x''y', \"note\" = NULL WHERE \"id\" = '7'");
    }

    #[test]
    fn a_placeholder_in_a_value_cannot_shift_the_parameters() {
        let e = edit(&[("a", Some("$2")), ("b", Some("{}"))], &[("id", Some("1"))]);
        let (pieces, cells) = update_pieces(&e, quote_ident, "");
        assert_eq!(join_with(&pieces, |i| format!("${}", i + 1)), "UPDATE \"shop\".\"items\" SET \"a\" = $1, \"b\" = $2 WHERE \"id\" = $3");
        assert_eq!(cells.len(), 3);
        assert_eq!(preview_update(&e).unwrap(), "UPDATE \"shop\".\"items\" SET \"a\" = '$2', \"b\" = '{}' WHERE \"id\" = '1'");
    }

    #[test]
    fn nul_characters_are_refused() {
        assert!(preview_update(&edit(&[("a", Some("x\0y"))], &[("id", Some("1"))])).is_err());
        assert!(check_text(&edit(&[("a", Some("ok"))], &[("id", Some("1"))])).is_ok());
    }

    #[test]
    fn types_map_to_kinds() {
        assert_eq!(col(Type::INT4).kind, ColumnKind::Number);
        assert_eq!(col(Type::NUMERIC).kind, ColumnKind::Number);
        assert_eq!(col(Type::TEXT).kind, ColumnKind::Text);
        assert_eq!(col(Type::VARCHAR).kind, ColumnKind::Text);
        assert_eq!(col(Type::UUID).kind, ColumnKind::Text);
        assert_eq!(col(Type::JSONB).kind, ColumnKind::Json);
        assert_eq!(col(Type::TIMESTAMPTZ).kind, ColumnKind::DateTime);
        assert_eq!(col(Type::BYTEA).kind, ColumnKind::Binary);
        assert_eq!(col(Type::BOOL).kind, ColumnKind::Other);
        assert_eq!(col(Type::INT8).data_type, "int8");
    }

    #[test]
    fn values_keep_their_meaning() {
        assert_eq!(to_json(None, &col(Type::TEXT)), Value::Null);
        assert_eq!(to_json(Some("42"), &col(Type::INT4)), json!(42));
        assert_eq!(to_json(Some("9007199254740993"), &col(Type::INT8)), json!("9007199254740993"));
        assert_eq!(to_json(Some("1.50"), &col(Type::NUMERIC)), json!("1.50"), "numeric keeps its digits");
        assert_eq!(to_json(Some("2.5"), &col(Type::FLOAT8)), json!(2.5));
        assert_eq!(to_json(Some("NaN"), &col(Type::FLOAT8)), json!("NaN"));
        assert_eq!(to_json(Some("Infinity"), &col(Type::FLOAT4)), json!("Infinity"));
        assert_eq!(to_json(Some("t"), &col(Type::BOOL)), json!(true));
        assert_eq!(to_json(Some("f"), &col(Type::BOOL)), json!(false));
        assert_eq!(to_json(Some("\\x00ff10"), &col(Type::BYTEA)), json!("0x00ff10"));
        assert_eq!(to_json(Some("{\"a\": 1}"), &col(Type::JSONB)), json!("{\"a\": 1}"));
        assert_eq!(to_json(Some("héllo"), &col(Type::TEXT)), json!("héllo"));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        let s = "é".repeat(MAX_CELL_BYTES);
        let v = to_json(Some(&s), &col(Type::TEXT));
        assert_eq!(v["truncated"], json!(true));
        assert_eq!(v["bytes"], json!(s.len()));
        assert!(v["preview"].as_str().unwrap().len() <= MAX_CELL_BYTES);
    }

    #[test]
    fn long_binary_is_shortened_with_its_size() {
        let hex = format!("\\x{}", "ab".repeat(1000));
        let v = to_json(Some(&hex), &col(Type::BYTEA));
        let s = v.as_str().unwrap();
        assert!(s.starts_with("0xabab") && s.ends_with("(1000 bytes)"), "{s}");
    }

    #[test]
    fn unix_sockets_and_empty_users_are_refused() {
        let mut spec = ConnectSpec {
            engine: "postgres".into(),
            host: "/var/run/postgresql".into(),
            port: 5432,
            user: "u".into(),
            password: None,
            database: None,
            tls: TlsMode::Disable,
            tunnel_port: None,
            options: Default::default(),
        };
        assert!(config_for(&spec).is_err());
        spec.host = "db".into();
        spec.user = " ".into();
        assert!(config_for(&spec).is_err());
        spec.user = "u".into();
        assert!(config_for(&spec).is_ok());
    }
}
