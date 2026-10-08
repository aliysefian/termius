//! Oracle Database, over `oracle-rs`: a pure-Rust client of Oracle's own network protocol, so no Oracle client
//! library (Instant Client) has to be installed. Connections are by service name (or SID); TLS is TCPS.
//!
//! A connection is to one database, so the tree starts at its schemas (users). Rows are read in batches until the
//! row limit; a query that stops at the limit or is cancelled closes its connection, which the next one replaces.
//! Oracle doesn't commit by itself, so a data change made from here is committed as soon as it succeeds, like the
//! other engines' autocommit; a statement that is a program block (`BEGIN … END;`) runs as it is.

use std::sync::Arc;
use std::time::Instant;

use oracle_rs::{Config, Connection, OracleType, TlsConfig, Value as OraValue};
use serde_json::{json, Value};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use uuid::Uuid;

use super::{
    cut_text, join_with, update_pieces, Backend, Column, ColumnInfo, ColumnKind, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit,
    TableInfo, TlsMode, TreeNode, CONNECT_TIMEOUT, MAX_CELL_BYTES,
};

const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;
const FETCH: u32 = 500;
/// Users Oracle creates for itself; their schemas are listed last when `oracle_maintained` isn't there (11g).
const SYSTEM_USERS: [&str; 12] = ["SYS", "SYSTEM", "OUTLN", "DBSNMP", "APPQOSSYS", "XDB", "CTXSYS", "MDSYS", "ORDSYS", "WMSYS", "ANONYMOUS", "AUDSYS"];

pub struct OracleConn {
    cfg: Config,
    addr: String,
    conn: AsyncMutex<Option<Arc<Connection>>>,
    running: std::sync::Mutex<std::collections::HashMap<Uuid, Arc<Notify>>>,
}

/// A name for Oracle: double quotes, doubled inside. Names from the tree are already in their exact case.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn literal(value: &Option<String>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(v) => format!("'{}'", v.replace('\'', "''")),
    }
}

pub fn preview_update(edit: &RowEdit) -> String {
    let (pieces, cells) = update_pieces(edit, quote_ident, "");
    join_with(&pieces, |i| literal(&cells[i].value))
}

fn err(e: oracle_rs::Error) -> DbError {
    use oracle_rs::Error as E;
    match e {
        E::OracleError { code, message } => DbError::Server(format!("ORA-{code:05}: {}", message.trim().trim_start_matches(&format!("ORA-{code:05}: ")))),
        E::ServerError { code, message } => DbError::Server(format!("{} ({code})", message.trim())),
        E::ConnectionRefused { error_code, message } => DbError::Server(format!("the listener refused the connection{}: {}", error_code.map(|c| format!(" (ORA-{c:05})")).unwrap_or_default(), message.unwrap_or_else(|| "no reason given".into()))),
        other => DbError::Server(other.to_string()),
    }
}

pub fn config_for(spec: &ConnectSpec) -> DbResult<Config> {
    if spec.user.trim().is_empty() {
        return Err(DbError::Invalid("enter a user name".into()));
    }
    let (host, port) = spec.dial();
    let password = spec.password.clone().unwrap_or_default();
    let mut cfg = match (spec.option("sid"), spec.option("service_name").or(spec.database.as_deref().filter(|d| !d.is_empty()))) {
        (Some(sid), _) => Config::with_sid(host, port, sid, &spec.user, password),
        (None, Some(service)) => Config::new(host, port, service, &spec.user, password),
        (None, None) => return Err(DbError::Invalid("enter the service name (the database field), like FREEPDB1 or ORCL, or a SID in the connection's settings".into())),
    };
    cfg = cfg.connect_timeout(CONNECT_TIMEOUT);
    match spec.tls {
        TlsMode::Disable => {}
        TlsMode::Require => {
            let mut t = TlsConfig::new();
            t.verify_server = false;
            t.server_name = Some(spec.host.clone());
            cfg = cfg.tls_config(t);
        }
        TlsMode::VerifyFull => {
            let mut t = TlsConfig::new();
            // The name the certificate is checked against stays the server's even through a tunnel.
            t.server_name = Some(spec.host.clone());
            if let Some(ca) = spec.option("ca_file") {
                t = t.with_ca_cert(ca);
            }
            if let Some(wallet) = spec.option("wallet") {
                t = t.with_wallet(wallet, spec.option("wallet_password").map(str::to_string));
            }
            cfg = cfg.tls_config(t);
        }
    }
    Ok(cfg)
}

fn kind_of(t: OracleType) -> ColumnKind {
    use OracleType::*;
    match t {
        Number | BinaryInteger | BinaryFloat | BinaryDouble => ColumnKind::Number,
        Date | Timestamp | TimestampTz | TimestampLtz | IntervalYm | IntervalDs => ColumnKind::DateTime,
        Raw | LongRaw | Blob | Bfile => ColumnKind::Binary,
        Json => ColumnKind::Json,
        Varchar | Long | Rowid | Urowid | Char | Clob => ColumnKind::Text,
        _ => ColumnKind::Other,
    }
}

fn hex(bytes: &[u8]) -> Value {
    let shown = bytes.len().min(MAX_CELL_BYTES / 2);
    let mut s = String::with_capacity(2 + shown * 2);
    s.push_str("0x");
    for b in &bytes[..shown] {
        s.push_str(&format!("{b:02X}"));
    }
    if shown < bytes.len() {
        json!({ "truncated": true, "preview": s, "bytes": bytes.len() })
    } else {
        Value::String(s)
    }
}

/// One cell as the grid shows it.
pub fn to_json(v: &OraValue) -> Value {
    match v {
        OraValue::Null => Value::Null,
        OraValue::String(s) => cut_text(s),
        OraValue::Bytes(b) => hex(b),
        OraValue::Integer(i) if i.abs() <= MAX_SAFE_INT => json!(i),
        OraValue::Integer(i) => Value::String(i.to_string()),
        OraValue::Float(f) if f.is_finite() => json!(f),
        OraValue::Float(f) => Value::String(f.to_string()),
        // NUMBER keeps every digit: a number with a decimal point or past 2^53 stays text, so nothing rounds.
        OraValue::Number(n) => match n.as_str().parse::<i64>() {
            Ok(i) if i.abs() <= MAX_SAFE_INT => json!(i),
            _ => match n.as_str().parse::<f64>() {
                Ok(f) if f.is_finite() && f.to_string() == n.as_str() => json!(f),
                _ => Value::String(n.as_str().to_string()),
            },
        },
        OraValue::Boolean(b) => json!(b),
        OraValue::Json(j) => cut_text(&j.to_string()),
        other => cut_text(&other.to_string()),
    }
}

fn text(r: &oracle_rs::Row, i: usize) -> Option<String> {
    r.get(i).and_then(OraValue::as_str).map(str::to_string)
}

impl OracleConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        // oracle-rs builds its TLS configuration with rustls' default provider; install ring once.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let me = Self { cfg: config_for(spec)?, addr: format!("{}:{}", spec.host, spec.port), conn: AsyncMutex::new(None), running: Default::default() };
        // Fail now, with the real reason, rather than on the first query.
        me.connection().await?;
        Ok(me)
    }

    /// The open connection, or a new one.
    async fn connection(&self) -> DbResult<Arc<Connection>> {
        let mut slot = self.conn.lock().await;
        if let Some(c) = slot.as_ref().filter(|c| !c.is_closed()) {
            return Ok(Arc::clone(c));
        }
        let c = match tokio::time::timeout(CONNECT_TIMEOUT + std::time::Duration::from_secs(5), Connection::connect_with_config(self.cfg.clone())).await {
            Err(_) => return Err(DbError::Timeout(self.addr.clone())),
            Ok(Err(e)) => {
                return Err(match e {
                    oracle_rs::Error::Io(io) => DbError::Connect { addr: self.addr.clone(), reason: io.to_string() },
                    other => err(other),
                })
            }
            Ok(Ok(c)) => Arc::new(c),
        };
        *slot = Some(Arc::clone(&c));
        Ok(c)
    }

    /// Forget the connection (and close it), so the next use opens a new one.
    async fn discard(&self) {
        if let Some(c) = self.conn.lock().await.take() {
            let _ = c.close().await;
        }
    }

    /// Rows for browsing: bound parameters, no limit worth speaking of.
    async fn rows(&self, sql: &str, params: &[OraValue]) -> DbResult<Vec<oracle_rs::Row>> {
        let c = self.connection().await?;
        let r = c.query(sql, params).await.map_err(err)?;
        let mut rows = r.rows;
        let (mut more, id, cols) = (r.has_more_rows, r.cursor_id, r.columns);
        while more && rows.len() < 20_000 {
            let next = c.fetch_more(id, &cols, FETCH).await.map_err(err)?;
            more = next.has_more_rows;
            rows.extend(next.rows);
        }
        Ok(rows)
    }

    async fn run(c: &Connection, sql: &str, limit: usize) -> DbResult<(QueryResult, bool)> {
        // A trailing semicolon ends a SQL statement in a client, but Oracle rejects it (a PL/SQL block needs it).
        let trimmed = sql.trim().trim_end_matches(';').trim_end();
        let body = if is_plsql(sql) { sql.trim() } else { trimmed };
        let r = c.execute(body, &[]).await.map_err(err)?;
        let mut out = QueryResult { columns: r.columns.iter().map(describe_column).collect(), ..Default::default() };
        let mut clean = true;
        let (mut more, id, cols) = (r.has_more_rows, r.cursor_id, r.columns.clone());
        out.rows = r.rows.iter().map(|row| row.values().iter().map(to_json).collect()).collect();
        while more && out.rows.len() <= limit {
            let next = c.fetch_more(id, &cols, FETCH).await.map_err(err)?;
            more = next.has_more_rows;
            out.rows.extend(next.rows.iter().map(|row| row.values().iter().map(to_json).collect::<Vec<_>>()));
        }
        if out.rows.len() > limit {
            out.rows.truncate(limit);
            out.truncated = true;
        }
        // A cursor that is still open is left behind with its connection.
        if more || out.truncated {
            clean = false;
        }
        if out.columns.is_empty() {
            out.affected_rows = Some(r.rows_affected);
            // Oracle keeps a data change in the session until it is committed; here it is, as the others do.
            if r.rows_affected > 0 && is_dml(sql) {
                c.commit().await.map_err(err)?;
            }
        }
        Ok((out, clean))
    }
}

fn describe_column(c: &oracle_rs::ColumnInfo) -> Column {
    Column { name: c.name.clone(), data_type: format!("{:?}", c.oracle_type).to_ascii_lowercase(), kind: kind_of(c.oracle_type) }
}

fn first_word(sql: &str) -> String {
    let mut rest = sql.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = r.split_once('\n').map_or("", |(_, t)| t).trim_start();
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, t)| t).trim_start();
        } else {
            break;
        }
    }
    rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase()
}

/// A program block, whose closing `;` belongs to it.
pub fn is_plsql(sql: &str) -> bool {
    matches!(first_word(sql).as_str(), "BEGIN" | "DECLARE" | "CREATE" | "CALL") && {
        let w = first_word(sql);
        w != "CREATE" || {
            let up = sql.to_ascii_uppercase();
            ["PROCEDURE", "FUNCTION", "PACKAGE", "TRIGGER", "TYPE BODY"].iter().any(|k| up.contains(k))
        }
    }
}

/// Statements that change rows, which this engine commits once they succeed.
pub fn is_dml(sql: &str) -> bool {
    matches!(first_word(sql).as_str(), "INSERT" | "UPDATE" | "DELETE" | "MERGE")
}

#[async_trait::async_trait]
impl Backend for OracleConn {
    async fn server_version(&self) -> DbResult<String> {
        let c = self.connection().await?;
        let info = c.server_info().await;
        let banner = info.banner.trim();
        Ok(if banner.is_empty() { format!("Oracle {}", info.version) } else { banner.to_string() })
    }

    async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let c = self.connection().await?;
        let notify = Arc::new(Notify::new());
        self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(qid, Arc::clone(&notify));
        // Cancelling closes the connection, which makes the server stop the statement.
        let outcome = tokio::select! {
            r = Self::run(&c, sql, limit) => Some(r),
            _ = notify.notified() => None,
        };
        self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&qid);
        match outcome {
            None => {
                self.discard().await;
                Err(DbError::Cancelled)
            }
            Some(Err(e)) => {
                // A failed statement can leave the session needing a reset; start fresh to be safe.
                if matches!(&e, DbError::Server(m) if m.contains("ORA-03113") || m.contains("ORA-03114") || m.contains("ORA-00028")) {
                    self.discard().await;
                }
                Err(e)
            }
            Some(Ok((mut result, clean))) => {
                if !clean {
                    self.discard().await;
                }
                result.elapsed_ms = started.elapsed().as_millis() as u64;
                Ok(result)
            }
        }
    }

    async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        if let Some(n) = self.running.lock().unwrap_or_else(|p| p.into_inner()).get(&qid) {
            n.notify_one();
        }
        Ok(())
    }

    async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        match path {
            [] => {
                let current = self.cfg.username.to_ascii_uppercase();
                let rows = self.rows("SELECT username FROM all_users ORDER BY username", &[]).await?;
                let mut names: Vec<String> = rows.iter().filter_map(|r| text(r, 0)).collect();
                // Your own schema first, other people's next, Oracle's own last.
                names.sort_by_key(|n| (if *n == current { 0 } else if SYSTEM_USERS.contains(&n.as_str()) { 2 } else { 1 }, n.clone()));
                Ok(names.into_iter().map(|name| TreeNode { name, kind: NodeKind::Schema, detail: None, expandable: true }).collect())
            }
            [owner] => {
                let rows = self
                    .rows(
                        "SELECT object_name, object_type FROM all_objects WHERE owner = :1 AND object_type IN ('TABLE','VIEW','MATERIALIZED VIEW') \
                         AND object_name NOT LIKE 'BIN$%' ORDER BY object_type, object_name",
                        &[OraValue::String(owner.clone())],
                    )
                    .await?;
                Ok(rows
                    .iter()
                    .filter_map(|r| {
                        let name = text(r, 0)?;
                        let ty = text(r, 1)?;
                        Some(TreeNode { name, kind: if ty == "TABLE" { NodeKind::Table } else { NodeKind::View }, detail: (ty == "MATERIALIZED VIEW").then(|| "materialized".to_string()), expandable: true })
                    })
                    .collect())
            }
            [owner, table] => {
                let info = self.table_info(owner, table).await?;
                let mut nodes: Vec<TreeNode> = info
                    .columns
                    .iter()
                    .map(|c| {
                        let mut detail = c.data_type.clone();
                        if c.primary_key {
                            detail.push_str(" · PK");
                        }
                        if c.nullable {
                            detail.push_str(" · null");
                        }
                        TreeNode { name: c.name.clone(), kind: NodeKind::Column, detail: Some(detail), expandable: false }
                    })
                    .collect();
                let idx = self
                    .rows("SELECT index_name, uniqueness FROM all_indexes WHERE table_owner = :1 AND table_name = :2 ORDER BY index_name", &[OraValue::String(owner.clone()), OraValue::String(table.clone())])
                    .await?;
                nodes.extend(idx.iter().filter_map(|r| {
                    Some(TreeNode { name: text(r, 0)?, kind: NodeKind::Index, detail: (text(r, 1).as_deref() == Some("UNIQUE")).then(|| "unique".to_string()), expandable: false })
                }));
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn table_info(&self, owner: &str, table: &str) -> DbResult<TableInfo> {
        let params = [OraValue::String(owner.to_string()), OraValue::String(table.to_string())];
        let cols = self
            .rows(
                "SELECT column_name, data_type, nullable, data_length, data_precision, data_scale, data_default FROM all_tab_columns \
                 WHERE owner = :1 AND table_name = :2 ORDER BY column_id",
                &params,
            )
            .await?;
        let keys = self
            .rows(
                "SELECT cc.column_name FROM all_constraints c JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name \
                 WHERE c.constraint_type = 'P' AND c.owner = :1 AND c.table_name = :2 ORDER BY cc.position",
                &params,
            )
            .await?;
        let primary_key: Vec<String> = keys.iter().filter_map(|r| text(r, 0)).collect();
        let columns = cols
            .iter()
            .filter_map(|r| {
                let name = text(r, 0)?;
                let mut data_type = text(r, 1)?;
                match (r.get(4).and_then(OraValue::as_i64), r.get(5).and_then(OraValue::as_i64)) {
                    (Some(p), Some(s)) if s != 0 => data_type.push_str(&format!("({p},{s})")),
                    (Some(p), _) => data_type.push_str(&format!("({p})")),
                    _ if data_type.contains("CHAR") => {
                        if let Some(len) = r.get(3).and_then(OraValue::as_i64) {
                            data_type.push_str(&format!("({len})"));
                        }
                    }
                    _ => {}
                }
                Some(ColumnInfo { primary_key: primary_key.contains(&name), nullable: text(r, 2).is_some_and(|n| n == "Y"), default: text(r, 6).map(|d| d.trim().to_string()).filter(|d| !d.is_empty()), data_type, name })
            })
            .collect();
        Ok(TableInfo { columns, primary_key })
    }

    fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        Ok(preview_update(edit))
    }

    async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        for c in edit.changes.iter().chain(&edit.key) {
            if c.value.as_deref().is_some_and(|v| v.contains('\0')) {
                return Err(DbError::Invalid("a value can't contain a NUL character".into()));
            }
        }
        // Values are bound, in the order of the gaps, as text; the server converts them to the column's type.
        let (pieces, cells) = update_pieces(edit, quote_ident, "");
        let sql = join_with(&pieces, |i| format!(":{}", i + 1));
        let params: Vec<OraValue> = cells.iter().map(|c| c.value.clone().map_or(OraValue::Null, OraValue::String)).collect();
        let c = self.connection().await?;
        let changed = c.execute_dml_sql(&sql, &params).await.map_err(err)?;
        c.commit().await.map_err(err)?;
        Ok(changed)
    }

    async fn close(&self) {
        self.discard().await;
    }

    fn is_sql(&self) -> bool {
        true
    }

    /// `BEGIN … END;` is a program block here, not the start of a transaction.
    fn checks_transactions(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
