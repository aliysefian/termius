//! Database connections for the Databases view: a tree to browse, queries
//! to run, and rows to edit. Tauri-agnostic, like the other engine modules.
//!
//! Each engine (MySQL today; others later) lives in its own file and is
//! reached through [`Engine`], so the manager, the commands and the UI don't
//! care which one a connection uses. A connection can go through a saved
//! SSH host: the database port is then reached by a loopback tunnel that
//! lives exactly as long as the session, and is never exposed otherwise.

pub mod elastic;
pub mod http;
pub mod mongo;
pub mod mssql;
pub mod mysql;
pub mod oracle;
pub mod pg;
pub mod redis;
pub mod rqlite;
pub mod safety;
pub mod tlsconf;

#[cfg(test)]
mod live_pg_tests;
#[cfg(test)]
mod live_tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Database types a connection can be saved as.
pub const ENGINES: [&str; 8] = ["mysql", "postgres", "mssql", "oracle", "rqlite", "redis", "mongodb", "elasticsearch"];

/// Longest a connect may take before it is reported as unreachable.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// Rows returned when the caller doesn't say.
pub const DEFAULT_ROW_LIMIT: usize = 1000;
/// Upper bound on a requested row limit.
pub const MAX_ROW_LIMIT: usize = 1_000_000;
/// A cell longer than this is cut and marked, so one huge column can't
/// stall the window. Cut cells are read-only.
pub const MAX_CELL_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The server's own message, shown in full.
    #[error("{0}")]
    Server(String),
    #[error("could not connect to {addr}: {reason}")]
    Connect { addr: String, reason: String },
    #[error("timed out connecting to {0}")]
    Timeout(String),
    #[error("{0}")]
    Invalid(String),
    #[error("the query was cancelled")]
    Cancelled,
    #[error("no such connection; it may have been closed")]
    NoSession,
    /// The statement looks destructive and the caller hasn't confirmed it.
    #[error("{0}")]
    NeedsConfirmation(String),
    #[error(transparent)]
    Tunnel(#[from] crate::forward::ForwardError),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// No encryption. The password and every row cross the network in clear.
    Disable,
    /// Encrypted, but the server's certificate is not checked.
    Require,
    /// Encrypted, and the certificate and name must match.
    #[default]
    VerifyFull,
}

/// Everything needed to reach one database server.
#[derive(Debug, Clone)]
pub struct ConnectSpec {
    /// Which driver: one of [`ENGINES`].
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: Option<String>,
    pub tls: TlsMode,
    /// Set when an SSH tunnel carries the connection: dial this loopback
    /// port instead of `host:port`. `host` stays the server's name as the SSH
    /// host sees it, so an engine can still check the certificate against it.
    pub tunnel_port: Option<u16>,
    /// Settings only some engines have (an Oracle service name, a read consistency level, a Redis database
    /// number, ...). Unknown keys are ignored, so a record from a newer version still opens.
    pub options: BTreeMap<String, String>,
}

impl ConnectSpec {
    /// A setting, if it is there and not blank.
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
    }

    /// The "read-only" setting: sessions start in a read-only transaction mode. A guard against accidents, not a
    /// security boundary (a person can switch it off with SQL); a database account that can only read is the hard limit.
    pub fn read_only(&self) -> bool {
        self.option("read_only").is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    /// Where to dial: the tunnel's loopback port, or the server itself.
    pub fn dial(&self) -> (String, u16) {
        match self.tunnel_port {
            Some(p) => ("127.0.0.1".to_string(), p),
            None => (self.host.clone(), self.port),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Number,
    Text,
    Json,
    DateTime,
    Binary,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Column {
    pub name: String,
    /// The server's own name for the type, e.g. `varchar`.
    pub data_type: String,
    pub kind: ColumnKind,
}

/// What a statement produced. `rows` hold JSON scalars; a cell that was cut
/// short is `{"truncated": true, "preview": "...", "bytes": N}`.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    /// More rows existed than the limit allowed.
    pub truncated: bool,
    /// For statements that change data (no result set).
    pub affected_rows: Option<u64>,
    pub last_insert_id: Option<u64>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Database,
    /// A PostgreSQL schema (its connection is to one database).
    Schema,
    /// A group of names that share a prefix (Redis keys like `user:42:name`).
    Folder,
    Table,
    View,
    Column,
    Index,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TreeNode {
    pub name: String,
    pub kind: NodeKind,
    /// A short note shown beside the name: a type, a row estimate, "PK".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub expandable: bool,
}

/// Where in the tree to look: the connection's root (empty path), a
/// database (`[db]`) or a table (`[db, table]`).
pub type NodePath = Vec<String>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TableInfo {
    pub columns: Vec<ColumnInfo>,
    /// Primary-key column names in key order. Empty means rows can't be
    /// told apart, so they can't be edited in place.
    pub primary_key: Vec<String>,
}

/// One cell change from the grid.
#[derive(Debug, Clone, Deserialize)]
pub struct CellEdit {
    pub column: String,
    /// `None` is NULL.
    pub value: Option<String>,
}

/// An in-place edit of one row, found by its primary key.
#[derive(Debug, Clone, Deserialize)]
pub struct RowEdit {
    pub database: String,
    pub table: String,
    /// Primary-key column and the row's current value (`None` is NULL,
    /// which never matches, so such a row is refused).
    pub key: Vec<CellEdit>,
    pub changes: Vec<CellEdit>,
}

impl RowEdit {
    pub fn validate(&self) -> DbResult<()> {
        if self.key.is_empty() {
            return Err(DbError::Invalid("this table has no primary key, so its rows can't be edited in place".into()));
        }
        if self.key.iter().any(|k| k.value.is_none()) {
            return Err(DbError::Invalid("a primary-key value is NULL, so the row can't be identified".into()));
        }
        if self.changes.is_empty() {
            return Err(DbError::Invalid("nothing was changed".into()));
        }
        if self.database.is_empty() || self.table.is_empty() {
            return Err(DbError::Invalid("the edit doesn't say which table it belongs to".into()));
        }
        Ok(())
    }
}

/// An `UPDATE` as text pieces with a gap for each value, plus the values in
/// gap order (changes first, then the key). Building from pieces, never by
/// replacing text, means a value or a name can't be mistaken for a
/// placeholder. `quote` writes an identifier; `suffix` ends the statement.
pub(crate) fn update_pieces<'a>(edit: &'a RowEdit, quote: fn(&str) -> String, suffix: &str) -> (Vec<String>, Vec<&'a CellEdit>) {
    let mut pieces = vec![format!("UPDATE {}.{} SET ", quote(&edit.database), quote(&edit.table))];
    let mut cells: Vec<&CellEdit> = Vec::new();
    for (i, c) in edit.changes.iter().enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        pieces.last_mut().expect("never empty").push_str(&format!("{sep}{} = ", quote(&c.column)));
        pieces.push(String::new());
        cells.push(c);
    }
    pieces.last_mut().expect("never empty").push_str(" WHERE ");
    for (i, k) in edit.key.iter().enumerate() {
        let sep = if i == 0 { "" } else { " AND " };
        pieces.last_mut().expect("never empty").push_str(&format!("{sep}{} = ", quote(&k.column)));
        pieces.push(String::new());
        cells.push(k);
    }
    pieces.last_mut().expect("never empty").push_str(suffix);
    (pieces, cells)
}

/// Pieces joined, with `fill(i)` written into gap `i`.
pub(crate) fn join_with(pieces: &[String], fill: impl Fn(usize) -> String) -> String {
    let mut out = String::new();
    for (i, p) in pieces.iter().enumerate() {
        out.push_str(p);
        if i + 1 < pieces.len() {
            out.push_str(&fill(i));
        }
    }
    out
}

/// What every engine after MySQL and PostgreSQL implements. The older two keep their own variants below; new
/// ones only need this.
#[async_trait::async_trait]
pub trait Backend: Send + Sync {
    async fn server_version(&self) -> DbResult<String>;
    async fn query(&self, qid: Uuid, text: &str, limit: usize) -> DbResult<QueryResult>;
    /// Stop a running query. Quietly does nothing if it already finished.
    async fn cancel(&self, qid: Uuid) -> DbResult<()>;
    async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>>;
    async fn table_info(&self, database: &str, table: &str) -> DbResult<TableInfo>;
    fn preview_update(&self, edit: &RowEdit) -> DbResult<String>;
    async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64>;
    async fn close(&self);
    /// Whether what is typed is SQL (so the SQL safety checks apply). Other engines say why a command is
    /// destructive themselves.
    fn is_sql(&self) -> bool;
    /// Whether to refuse a run that opens a transaction and doesn't end it (a pooled session would carry it into
    /// the next run). Off for engines where `BEGIN` also starts a program block.
    fn checks_transactions(&self) -> bool {
        self.is_sql()
    }
    fn destructive_reason(&self, _text: &str) -> Option<String> {
        None
    }
}

/// A connected engine.
pub enum Engine {
    Mysql(mysql::MysqlConn),
    /// Boxed: much larger than the MySQL connection.
    Postgres(Box<pg::PgConn>),
    Other(Box<dyn Backend>),
}

impl Engine {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Engine> {
        match spec.engine.as_str() {
            "mysql" => Ok(Engine::Mysql(mysql::MysqlConn::connect(spec).await?)),
            "postgres" => Ok(Engine::Postgres(Box::new(pg::PgConn::connect(spec).await?))),
            "mssql" => Ok(Engine::Other(Box::new(mssql::MssqlConn::connect(spec).await?))),
            "oracle" => Ok(Engine::Other(Box::new(oracle::OracleConn::connect(spec).await?))),
            "rqlite" => Ok(Engine::Other(Box::new(rqlite::RqliteConn::connect(spec).await?))),
            "redis" => Ok(Engine::Other(Box::new(redis::RedisConn::connect(spec).await?))),
            "mongodb" => Ok(Engine::Other(Box::new(mongo::MongoConn::connect(spec).await?))),
            "elasticsearch" => Ok(Engine::Other(Box::new(elastic::ElasticConn::connect(spec).await?))),
            other => Err(DbError::Invalid(format!("unsupported database type \"{other}\""))),
        }
    }

    pub async fn server_version(&self) -> DbResult<String> {
        match self {
            Engine::Mysql(c) => c.server_version().await,
            Engine::Postgres(c) => c.server_version().await,
            Engine::Other(c) => c.server_version().await,
        }
    }

    pub async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let limit = limit.clamp(1, MAX_ROW_LIMIT);
        match self {
            Engine::Mysql(c) => c.query(qid, sql, limit).await,
            Engine::Postgres(c) => c.query(qid, sql, limit).await,
            Engine::Other(c) => c.query(qid, sql, limit).await,
        }
    }

    /// Stop a running query. Quietly does nothing if it already finished.
    pub async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        match self {
            Engine::Mysql(c) => c.cancel(qid).await,
            Engine::Postgres(c) => c.cancel(qid).await,
            Engine::Other(c) => c.cancel(qid).await,
        }
    }

    pub async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        match self {
            Engine::Mysql(c) => c.children(path).await,
            Engine::Postgres(c) => c.children(path).await,
            Engine::Other(c) => c.children(path).await,
        }
    }

    pub async fn table_info(&self, database: &str, table: &str) -> DbResult<TableInfo> {
        match self {
            Engine::Mysql(c) => c.table_info(database, table).await,
            Engine::Postgres(c) => c.table_info(database, table).await,
            Engine::Other(c) => c.table_info(database, table).await,
        }
    }

    /// The `UPDATE` an edit would run, with values written in, for showing
    /// to the user. What actually runs uses bound parameters.
    pub fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        edit.validate()?;
        match self {
            Engine::Mysql(_) => Ok(mysql::preview_update(edit)),
            Engine::Postgres(_) => pg::preview_update(edit),
            Engine::Other(c) => c.preview_update(edit),
        }
    }

    /// Run an edit. Returns the rows changed; anything but 1 is reported so
    /// the caller can warn.
    pub async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        edit.validate()?;
        match self {
            Engine::Mysql(c) => c.apply_update(edit).await,
            Engine::Postgres(c) => c.apply_update(edit).await,
            Engine::Other(c) => c.apply_update(edit).await,
        }
    }

    /// Whether an open transaction at the end of a run is refused.
    pub fn checks_transactions(&self) -> bool {
        match self {
            Engine::Mysql(_) | Engine::Postgres(_) => true,
            Engine::Other(c) => c.checks_transactions(),
        }
    }

    /// Whether statements for this engine are SQL.
    pub fn is_sql(&self) -> bool {
        match self {
            Engine::Mysql(_) | Engine::Postgres(_) => true,
            Engine::Other(c) => c.is_sql(),
        }
    }

    /// Why this text is destructive, or none.
    pub fn destructive_reason(&self, text: &str) -> Option<String> {
        match self {
            Engine::Mysql(_) | Engine::Postgres(_) => safety::destructive_reason(text),
            Engine::Other(c) => {
                if c.is_sql() {
                    safety::destructive_reason(text).or_else(|| c.destructive_reason(text))
                } else {
                    c.destructive_reason(text)
                }
            }
        }
    }

    pub async fn close(&self) {
        match self {
            Engine::Mysql(c) => c.close().await,
            Engine::Postgres(c) => c.close().await,
            Engine::Other(c) => c.close().await,
        }
    }
}

/// One open connection, plus the tunnel it rides on, if any.
pub struct DbSession {
    pub engine: Engine,
    /// Held so the tunnel is torn down when the session is dropped.
    _tunnel: Option<crate::forward::LocalTunnel>,
}

#[derive(Default)]
pub struct DbManager {
    sessions: Mutex<HashMap<Uuid, Arc<DbSession>>>,
}

impl DbManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connect and keep the session. With `via`, the connection goes through
    /// that SSH target to the database's host and port.
    pub async fn open(&self, spec: ConnectSpec, via: Option<crate::ssh::Target>) -> DbResult<(Uuid, String)> {
        let mut spec = spec;
        let tunnel = match via {
            Some(target) => {
                let t = crate::forward::open_local_tunnel(&target, spec.host.clone(), spec.port).await?;
                spec.tunnel_port = Some(t.port());
                Some(t)
            }
            None => None,
        };
        let engine = Engine::connect(&spec).await?;
        let version = engine.server_version().await.unwrap_or_default();
        let id = Uuid::new_v4();
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, Arc::new(DbSession { engine, _tunnel: tunnel }));
        Ok((id, version))
    }

    pub fn get(&self, id: Uuid) -> DbResult<Arc<DbSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
            .cloned()
            .ok_or(DbError::NoSession)
    }

    pub async fn close(&self, id: Uuid) {
        let s = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        if let Some(s) = s {
            s.engine.close().await;
        }
    }

    pub async fn close_all(&self) {
        let all: Vec<Arc<DbSession>> = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .map(|(_, s)| s)
            .collect();
        for s in all {
            s.engine.close().await;
        }
    }

    pub fn open_count(&self) -> usize {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// Run a statement, refusing destructive ones the caller hasn't confirmed.
pub async fn run_query(
    session: &DbSession,
    qid: Uuid,
    sql: &str,
    limit: Option<usize>,
    confirmed: bool,
) -> DbResult<QueryResult> {
    if sql.trim().is_empty() {
        return Err(DbError::Invalid("type a statement to run".into()));
    }
    if session.engine.checks_transactions() && safety::leaves_transaction_open(sql) {
        return Err(DbError::Invalid(
            "This opens a transaction without ending it. Each run is its own session slice, so the transaction could not be \
             continued in the next run. Put BEGIN ... COMMIT (or ROLLBACK) in the same run."
                .into(),
        ));
    }
    if !confirmed {
        if let Some(reason) = session.engine.destructive_reason(sql) {
            return Err(DbError::NeedsConfirmation(reason));
        }
    }
    session.engine.query(qid, sql, limit.unwrap_or(DEFAULT_ROW_LIMIT)).await
}

// -- helpers shared by the engines that speak JSON ---------------------------------------------

/// Text, cut on a character boundary when it is longer than a cell may be.
pub(crate) fn cut_text(text: &str) -> Value {
    if text.len() <= MAX_CELL_BYTES {
        return Value::String(text.to_string());
    }
    let mut end = MAX_CELL_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    serde_json::json!({ "truncated": true, "preview": &text[..end], "bytes": text.len() })
}

/// A JSON value as a grid cell: scalars stay what they are, objects and arrays become compact JSON text.
pub(crate) fn cell_from_json(v: &Value) -> Value {
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => v.clone(),
        Value::String(s) => cut_text(s),
        Value::Array(_) | Value::Object(_) => cut_text(&v.to_string()),
    }
}

/// The kind of a column of JSON values.
fn kind_of_values<'a>(values: impl Iterator<Item = &'a Value>) -> ColumnKind {
    let mut kind: Option<ColumnKind> = None;
    for v in values {
        let k = match v {
            Value::Null => continue,
            Value::Number(_) => ColumnKind::Number,
            Value::Bool(_) | Value::String(_) => ColumnKind::Text,
            Value::Array(_) | Value::Object(_) => ColumnKind::Json,
        };
        kind = Some(match kind {
            None => k,
            Some(old) if old == k => k,
            Some(_) => ColumnKind::Other,
        });
    }
    kind.unwrap_or(ColumnKind::Text)
}

/// Most columns a document result shows; the rest are left out and `truncated` says so.
pub(crate) const MAX_COLUMNS: usize = 200;

/// Rows from JSON objects: one column per top-level key (the keys in `first` leading, then in the order they
/// were first seen), a cell per object, empty where an object lacks the key.
pub(crate) fn table_from_objects(objs: &[serde_json::Map<String, Value>], first: &[&str]) -> (Vec<Column>, Vec<Vec<Value>>) {
    let mut names: Vec<String> = first.iter().filter(|f| objs.iter().any(|o| o.contains_key(**f))).map(|f| f.to_string()).collect();
    for o in objs {
        for k in o.keys() {
            if names.len() >= MAX_COLUMNS {
                break;
            }
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
    }
    let columns = names
        .iter()
        .map(|n| {
            let kind = kind_of_values(objs.iter().filter_map(|o| o.get(n)));
            let data_type = match kind {
                ColumnKind::Number => "number",
                ColumnKind::Json => "json",
                ColumnKind::Other => "mixed",
                _ => "text",
            };
            Column { name: n.clone(), data_type: data_type.into(), kind }
        })
        .collect();
    let rows = objs.iter().map(|o| names.iter().map(|n| o.get(n).map(cell_from_json).unwrap_or(Value::Null)).collect()).collect();
    (columns, rows)
}

/// A small table: the given columns and rows of text.
pub(crate) fn text_table(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: columns.iter().map(|n| Column { name: (*n).into(), data_type: "text".into(), kind: ColumnKind::Text }).collect(),
        rows,
        ..Default::default()
    }
}
