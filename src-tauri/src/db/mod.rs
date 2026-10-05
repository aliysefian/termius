//! Database connections for the Databases view: a tree to browse, queries
//! to run, and rows to edit. Tauri-agnostic, like the other engine modules.
//!
//! Each engine (MySQL today; others later) lives in its own file and is
//! reached through [`Engine`], so the manager, the commands and the UI don't
//! care which one a connection uses. A connection can go through a saved
//! SSH host: the database port is then reached by a loopback tunnel that
//! lives exactly as long as the session, and is never exposed otherwise.

pub mod mysql;
pub mod safety;

#[cfg(test)]
mod live_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Database types a connection can be saved as.
pub const ENGINES: [&str; 1] = ["mysql"];

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
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: Option<String>,
    pub tls: TlsMode,
    /// The host and port are a loopback tunnel's, so the certificate is
    /// still checked but its name can't be matched against `host`.
    pub tunnelled: bool,
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

/// A connected engine.
pub enum Engine {
    Mysql(mysql::MysqlConn),
}

impl Engine {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Engine> {
        Ok(Engine::Mysql(mysql::MysqlConn::connect(spec).await?))
    }

    pub async fn server_version(&self) -> DbResult<String> {
        match self {
            Engine::Mysql(c) => c.server_version().await,
        }
    }

    pub async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let limit = limit.clamp(1, MAX_ROW_LIMIT);
        match self {
            Engine::Mysql(c) => c.query(qid, sql, limit).await,
        }
    }

    /// Stop a running query. Quietly does nothing if it already finished.
    pub async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        match self {
            Engine::Mysql(c) => c.cancel(qid).await,
        }
    }

    pub async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        match self {
            Engine::Mysql(c) => c.children(path).await,
        }
    }

    pub async fn table_info(&self, database: &str, table: &str) -> DbResult<TableInfo> {
        match self {
            Engine::Mysql(c) => c.table_info(database, table).await,
        }
    }

    /// The `UPDATE` an edit would run, with values written in, for showing
    /// to the user. What actually runs uses bound parameters.
    pub fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        edit.validate()?;
        match self {
            Engine::Mysql(_) => Ok(mysql::preview_update(edit)),
        }
    }

    /// Run an edit. Returns the rows changed; anything but 1 is reported so
    /// the caller can warn.
    pub async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        edit.validate()?;
        match self {
            Engine::Mysql(c) => c.apply_update(edit).await,
        }
    }

    pub async fn close(&self) {
        match self {
            Engine::Mysql(c) => c.close().await,
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
                spec.host = "127.0.0.1".into();
                spec.tunnelled = true;
                spec.port = t.port();
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
    if !confirmed {
        if let Some(reason) = safety::destructive_reason(sql) {
            return Err(DbError::NeedsConfirmation(reason));
        }
    }
    session.engine.query(qid, sql, limit.unwrap_or(DEFAULT_ROW_LIMIT)).await
}
