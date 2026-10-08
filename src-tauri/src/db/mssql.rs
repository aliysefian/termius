//! Microsoft SQL Server (and Azure SQL), over `tiberius` (the TDS protocol).
//!
//! A connection is to one database, so the tree starts at its schemas, like PostgreSQL. SQL logins and Windows
//! logins (`DOMAIN\user`, NTLM) are supported; named instances are reached by their port. Rows are read as a
//! stream, and a query that stops at the row limit or is cancelled closes its connection instead of draining it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use futures_util::TryStreamExt;
use serde_json::{json, Value};
use tiberius::{AuthMethod, Client, ColumnData, ColumnType, Config, EncryptionLevel, FromSql, QueryItem, ToSql};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};
use uuid::Uuid;

use super::{
    cut_text, join_with, update_pieces, Backend, Column, ColumnInfo, ColumnKind, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit,
    TableInfo, TlsMode, TreeNode, CONNECT_TIMEOUT, MAX_CELL_BYTES,
};

type Cli = Client<Compat<TcpStream>>;

const MAX_IDLE: usize = 4;
const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;

pub struct MssqlConn {
    cfg: Config,
    dial: (String, u16),
    addr: String,
    idle: Mutex<Vec<Cli>>,
    running: Mutex<HashMap<Uuid, Arc<Notify>>>,
}

/// A name for T-SQL: square brackets, with `]` doubled.
pub fn quote_ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

fn literal(value: &Option<String>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(v) => format!("N'{}'", v.replace('\'', "''")),
    }
}

pub fn preview_update(edit: &RowEdit) -> String {
    let (pieces, cells) = update_pieces(edit, quote_ident, "");
    join_with(&pieces, |i| literal(&cells[i].value))
}

fn server_err(e: tiberius::error::Error) -> DbError {
    use tiberius::error::Error as E;
    match e {
        E::Server(s) => DbError::Server(format!("{} ({})", s.message().trim(), s.code())),
        E::Tls(m) => DbError::Server(format!("TLS: {m}")),
        other => DbError::Server(other.to_string()),
    }
}

/// How the login is made: a SQL login, or a Windows one when the name has a domain (`CORP\ann`). Windows logins
/// use the operating system's own sign-in, so they exist only in the Windows version.
pub fn auth_for(user: &str, password: &str) -> DbResult<AuthMethod> {
    if user.contains('\\') {
        #[cfg(windows)]
        return Ok(AuthMethod::windows(user, password));
        #[cfg(not(windows))]
        return Err(DbError::Invalid("Windows logins (DOMAIN\\user) work in the Windows version of SSHVault; use a SQL login here".into()));
    }
    Ok(AuthMethod::sql_server(user, password))
}

pub fn config_for(spec: &ConnectSpec) -> DbResult<Config> {
    if spec.user.trim().is_empty() {
        return Err(DbError::Invalid("enter a user name (a SQL login, or DOMAIN\\user for a Windows login)".into()));
    }
    let mut c = Config::new();
    // The name the certificate is checked against; the socket is dialled separately (a tunnel is loopback).
    c.host(&spec.host);
    c.port(spec.port);
    if let Some(db) = spec.database.as_deref().filter(|d| !d.is_empty()) {
        c.database(db);
    }
    c.application_name("SSHVault");
    c.authentication(auth_for(&spec.user, spec.password.as_deref().unwrap_or(""))?);
    match spec.tls {
        // The login itself is still encrypted (TDS always protects it); the data is not.
        TlsMode::Disable => c.encryption(EncryptionLevel::Off),
        TlsMode::Require => {
            c.encryption(EncryptionLevel::Required);
            c.trust_cert();
        }
        TlsMode::VerifyFull => {
            c.encryption(EncryptionLevel::Required);
            if let Some(ca) = spec.option("ca_file") {
                c.trust_cert_ca(ca);
            }
        }
    }
    if spec.option("read_only").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        c.readonly(true);
    }
    Ok(c)
}

fn kind_of(t: ColumnType) -> ColumnKind {
    use ColumnType::*;
    match t {
        Bit | Bitn | Int1 | Int2 | Int4 | Int8 | Intn | Float4 | Float8 | Floatn | Money | Money4 | Decimaln | Numericn => ColumnKind::Number,
        Datetime | Datetime4 | Datetimen | Daten | Timen | Datetime2 | DatetimeOffsetn => ColumnKind::DateTime,
        BigVarBin | BigBinary | Image | Udt => ColumnKind::Binary,
        Xml => ColumnKind::Json,
        BigVarChar | BigChar | NVarchar | NChar | Text | NText | Guid => ColumnKind::Text,
        Null | SSVariant => ColumnKind::Other,
    }
}

fn describe_column(c: &tiberius::Column) -> Column {
    let ty = format!("{:?}", c.column_type()).to_ascii_lowercase();
    Column { name: c.name().to_string(), data_type: ty, kind: kind_of(c.column_type()) }
}

fn int(v: i64) -> Value {
    if v.abs() <= MAX_SAFE_INT {
        json!(v)
    } else {
        // Beyond 2^53 a JSON reader would round it: keep the exact digits.
        Value::String(v.to_string())
    }
}

fn float(v: f64) -> Value {
    if v.is_finite() {
        json!(v)
    } else {
        Value::String(v.to_string())
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
pub fn to_json(d: &ColumnData<'static>) -> Value {
    match d {
        ColumnData::U8(v) => v.map_or(Value::Null, |v| json!(v)),
        ColumnData::I16(v) => v.map_or(Value::Null, |v| json!(v)),
        ColumnData::I32(v) => v.map_or(Value::Null, |v| json!(v)),
        ColumnData::I64(v) => v.map_or(Value::Null, int),
        ColumnData::F32(v) => v.map_or(Value::Null, |v| float(f64::from(v))),
        ColumnData::F64(v) => v.map_or(Value::Null, float),
        ColumnData::Bit(v) => v.map_or(Value::Null, |v| json!(v)),
        ColumnData::String(v) => v.as_ref().map_or(Value::Null, |s| cut_text(s)),
        ColumnData::Guid(v) => v.map_or(Value::Null, |g| Value::String(g.to_string().to_uppercase())),
        ColumnData::Binary(v) => v.as_ref().map_or(Value::Null, |b| hex(b)),
        // Decimals keep every digit as text, so nothing rounds.
        ColumnData::Numeric(v) => v.map_or(Value::Null, |n| Value::String(n.to_string())),
        ColumnData::Xml(v) => v.as_ref().map_or(Value::Null, |x| cut_text(x.as_ref().as_ref())),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => match NaiveDateTime::from_sql(d) {
            Ok(Some(t)) => Value::String(t.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
            _ => Value::Null,
        },
        ColumnData::Date(_) => match NaiveDate::from_sql(d) {
            Ok(Some(t)) => Value::String(t.format("%Y-%m-%d").to_string()),
            _ => Value::Null,
        },
        ColumnData::Time(_) => match NaiveTime::from_sql(d) {
            Ok(Some(t)) => Value::String(t.format("%H:%M:%S%.f").to_string()),
            _ => Value::Null,
        },
        ColumnData::DateTimeOffset(_) => match chrono::DateTime::<chrono::FixedOffset>::from_sql(d) {
            Ok(Some(t)) => Value::String(t.format("%Y-%m-%d %H:%M:%S%.f %:z").to_string()),
            _ => Value::Null,
        },
    }
}

impl MssqlConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        // tiberius builds its TLS configuration with rustls' default provider; install ring once.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let me = Self { cfg: config_for(spec)?, dial: spec.dial(), addr: format!("{}:{}", spec.host, spec.port), idle: Mutex::default(), running: Mutex::default() };
        // Fail now, with the real reason, rather than on the first query.
        let first = me.open().await?;
        me.release(first);
        Ok(me)
    }

    async fn open(&self) -> DbResult<Cli> {
        let attempt = async {
            let tcp = TcpStream::connect((self.dial.0.as_str(), self.dial.1)).await.map_err(|e| DbError::Connect { addr: self.addr.clone(), reason: e.to_string() })?;
            let _ = tcp.set_nodelay(true);
            Client::connect(self.cfg.clone(), tcp.compat_write()).await.map_err(|e| match e {
                tiberius::error::Error::Server(_) => server_err(e),
                other => DbError::Connect { addr: self.addr.clone(), reason: other.to_string() },
            })
        };
        match tokio::time::timeout(CONNECT_TIMEOUT + std::time::Duration::from_secs(5), attempt).await {
            Err(_) => Err(DbError::Timeout(self.addr.clone())),
            Ok(r) => r,
        }
    }

    async fn acquire(&self) -> DbResult<Cli> {
        let next = self.idle.lock().unwrap_or_else(|p| p.into_inner()).pop();
        match next {
            Some(c) => Ok(c),
            None => self.open().await,
        }
    }

    fn release(&self, c: Cli) {
        let mut idle = self.idle.lock().unwrap_or_else(|p| p.into_inner());
        if idle.len() < MAX_IDLE {
            idle.push(c);
        }
    }

    /// Rows of a query with bound parameters, for browsing.
    async fn rows(&self, sql: &str, params: &[&dyn ToSql]) -> DbResult<Vec<tiberius::Row>> {
        let mut c = self.acquire().await?;
        let out = async { c.query(sql, params).await?.into_first_result().await }.await;
        match out {
            Ok(rows) => {
                self.release(c);
                Ok(rows)
            }
            Err(e) => Err(server_err(e)),
        }
    }

    /// One statement, read as a stream. Returns the connection too, when it is clean enough to reuse.
    async fn run(c: &mut Cli, sql: &str, limit: usize) -> Result<(QueryResult, bool), tiberius::error::Error> {
        let mut out = QueryResult::default();
        let mut stream = c.simple_query(sql).await?;
        let mut clean = true;
        while let Some(item) = stream.try_next().await? {
            match item {
                QueryItem::Metadata(m) => {
                    // Only the first result set is shown, as with the other engines.
                    if m.result_index() > 0 {
                        clean = false;
                        break;
                    }
                    out.columns = m.columns().iter().map(describe_column).collect();
                }
                QueryItem::Row(r) if r.result_index() == 0 => {
                    if out.rows.len() >= limit {
                        out.truncated = true;
                        clean = false;
                        break;
                    }
                    out.rows.push(r.cells().map(|(_, d)| to_json(d)).collect());
                }
                QueryItem::Row(_) => {}
            }
        }
        drop(stream);
        // A statement that returns no rows reports how many it changed.
        if out.columns.is_empty() && clean {
            if let Some(n) = c.simple_query("SELECT @@ROWCOUNT").await?.into_first_result().await?.first().and_then(|r| r.get::<i32, _>(0)) {
                out.affected_rows = Some(n.max(0) as u64);
            }
        }
        Ok((out, clean))
    }
}

fn text(r: &tiberius::Row, i: usize) -> Option<String> {
    r.get::<&str, _>(i).map(str::to_string)
}

#[async_trait::async_trait]
impl Backend for MssqlConn {
    async fn server_version(&self) -> DbResult<String> {
        let rows = self.rows("SELECT @@VERSION", &[]).await?;
        let v = rows.first().and_then(|r| text(r, 0)).unwrap_or_default();
        // The first line is the product and build; the rest is the licence notice.
        Ok(v.lines().next().unwrap_or("").trim().to_string())
    }

    async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let mut c = self.acquire().await?;
        let notify = Arc::new(Notify::new());
        self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(qid, Arc::clone(&notify));
        // Cancelling drops the connection, which makes the server stop the statement.
        let outcome = tokio::select! {
            r = Self::run(&mut c, sql, limit) => Some(r),
            _ = notify.notified() => None,
        };
        self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&qid);
        match outcome {
            None => {
                drop(c);
                Err(DbError::Cancelled)
            }
            Some(Err(e)) => Err(server_err(e)),
            Some(Ok((mut result, clean))) => {
                if clean {
                    self.release(c);
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
                let rows = self
                    .rows(
                        "SELECT s.name FROM sys.schemas s WHERE s.name NOT IN ('sys','INFORMATION_SCHEMA','guest') AND s.name NOT LIKE 'db[_]%' \
                         ORDER BY CASE WHEN s.name = 'dbo' THEN 0 ELSE 1 END, s.name",
                        &[],
                    )
                    .await?;
                Ok(rows.iter().filter_map(|r| text(r, 0)).map(|name| TreeNode { name, kind: NodeKind::Schema, detail: None, expandable: true }).collect())
            }
            [schema] => {
                let rows = self
                    .rows(
                        "SELECT o.name, o.type FROM sys.objects o JOIN sys.schemas s ON s.schema_id = o.schema_id \
                         WHERE s.name = @P1 AND o.type IN ('U','V') AND o.is_ms_shipped = 0 ORDER BY o.type DESC, o.name",
                        &[schema],
                    )
                    .await?;
                Ok(rows
                    .iter()
                    .filter_map(|r| {
                        let name = text(r, 0)?;
                        let view = text(r, 1)?.trim() == "V";
                        Some(TreeNode { name, kind: if view { NodeKind::View } else { NodeKind::Table }, detail: None, expandable: true })
                    })
                    .collect())
            }
            [schema, table] => {
                let info = self.table_info(schema, table).await?;
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
                    .rows(
                        "SELECT i.name, i.is_unique, i.is_primary_key FROM sys.indexes i \
                         WHERE i.object_id = OBJECT_ID(QUOTENAME(@P1) + '.' + QUOTENAME(@P2)) AND i.name IS NOT NULL ORDER BY i.name",
                        &[schema, table],
                    )
                    .await?;
                nodes.extend(idx.iter().filter_map(|r| {
                    let name = text(r, 0)?;
                    let unique = r.get::<bool, _>(1).unwrap_or(false);
                    let pk = r.get::<bool, _>(2).unwrap_or(false);
                    Some(TreeNode { name, kind: NodeKind::Index, detail: if pk { Some("primary key".into()) } else { unique.then(|| "unique".into()) }, expandable: false })
                }));
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn table_info(&self, schema: &str, table: &str) -> DbResult<TableInfo> {
        let cols = self
            .rows(
                "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT, CHARACTER_MAXIMUM_LENGTH FROM INFORMATION_SCHEMA.COLUMNS \
                 WHERE TABLE_SCHEMA = @P1 AND TABLE_NAME = @P2 ORDER BY ORDINAL_POSITION",
                &[&schema, &table],
            )
            .await?;
        let keys = self
            .rows(
                "SELECT kcu.COLUMN_NAME FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
                 JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
                 WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' AND tc.TABLE_SCHEMA = @P1 AND tc.TABLE_NAME = @P2 ORDER BY kcu.ORDINAL_POSITION",
                &[&schema, &table],
            )
            .await?;
        let primary_key: Vec<String> = keys.iter().filter_map(|r| text(r, 0)).collect();
        let columns = cols
            .iter()
            .filter_map(|r| {
                let name = text(r, 0)?;
                let mut data_type = text(r, 1)?;
                if let Some(len) = r.get::<i32, _>(4) {
                    data_type.push_str(&if len < 0 { "(max)".to_string() } else { format!("({len})") });
                }
                Some(ColumnInfo { primary_key: primary_key.contains(&name), nullable: text(r, 2).is_some_and(|n| n == "YES"), default: text(r, 3), data_type, name })
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
        let sql = join_with(&pieces, |i| format!("@P{}", i + 1));
        let values: Vec<Option<String>> = cells.iter().map(|c| c.value.clone()).collect();
        let params: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
        let mut c = self.acquire().await?;
        match c.execute(sql, &params).await {
            Ok(r) => {
                self.release(c);
                Ok(r.total())
            }
            Err(e) => Err(server_err(e)),
        }
    }

    async fn close(&self) {
        self.idle.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    fn is_sql(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
