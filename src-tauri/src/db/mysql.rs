//! MySQL and MariaDB, over `mysql_async`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use mysql_async::consts::ColumnType;
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, SslOpts, Value as MyValue};
use serde_json::{json, Value};
use uuid::Uuid;

use super::meta::{self, ConstraintKind, ConstraintMeta, Ddl, ForeignRef, IncomingRef, IndexMeta, SearchHit, TableMeta};
use super::{
    change_pieces, join_with, Change, update_pieces, Column, ColumnInfo, ColumnKind, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit,
    TableInfo, TlsMode, TreeNode, CONNECT_TIMEOUT, MAX_CELL_BYTES,
};

/// The binary character set: a BLOB with this is bytes, without it is TEXT.
const BINARY_CHARSET: u16 = 63;
const SYSTEM_SCHEMAS: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];

pub struct MysqlConn {
    pool: Pool,
    /// Server connection id of each running query, for `KILL QUERY`.
    running: Mutex<HashMap<Uuid, u32>>,
    /// Queries a caller asked to stop, so their failure reads as a cancel.
    cancelled: Mutex<std::collections::HashSet<Uuid>>,
}

fn server_err(e: mysql_async::Error) -> DbError {
    match e {
        mysql_async::Error::Server(s) => DbError::Server(format!("{} ({})", s.message, s.code)),
        other => DbError::Server(other.to_string()),
    }
}

fn opts(spec: &ConnectSpec) -> Opts {
    let ssl = match spec.tls {
        TlsMode::Disable => None,
        TlsMode::Require => Some(
            SslOpts::default()
                .with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true),
        ),
        TlsMode::VerifyFull => {
            let o = SslOpts::default();
            // The tunnel dials loopback, so the name can't be matched; the chain is still checked.
            Some(if spec.tunnel_port.is_some() { o.with_danger_skip_domain_validation(true) } else { o })
        }
    };
    let (host, port) = match spec.tunnel_port {
        Some(p) => ("127.0.0.1".to_string(), p),
        None => (spec.host.clone(), spec.port),
    };
    let builder = OptsBuilder::default()
        .ip_or_hostname(host)
        .tcp_port(port)
        .user(Some(spec.user.clone()))
        .pass(spec.password.clone().filter(|p| !p.is_empty()))
        .db_name(spec.database.clone().filter(|d| !d.is_empty()))
        .ssl_opts(ssl)
        .prefer_socket(false)
        // Keep a spare connection so Cancel can always get through.
        .pool_opts(PoolOpts::default().with_constraints(PoolConstraints::new(1, 6).expect("1 <= 6")));
    // Runs on every new connection in the pool, so no session of this connection starts able to write.
    let builder = if spec.read_only() { builder.init(vec!["SET SESSION TRANSACTION READ ONLY"]) } else { builder };
    builder.into()
}

impl MysqlConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        let addr = format!("{}:{}", spec.host, spec.port);
        let pool = Pool::new(opts(spec));
        // Fail now, with the real reason, rather than on the first query.
        let probe = async { pool.get_conn().await };
        match tokio::time::timeout(CONNECT_TIMEOUT, probe).await {
            Err(_) => {
                let _ = pool.disconnect().await;
                Err(DbError::Timeout(addr))
            }
            Ok(Err(e)) => {
                let _ = pool.disconnect().await;
                Err(match e {
                    mysql_async::Error::Server(s) => DbError::Server(format!("{} ({})", s.message, s.code)),
                    other => DbError::Connect { addr, reason: other.to_string() },
                })
            }
            Ok(Ok(_conn)) => Ok(Self { pool, running: Mutex::default(), cancelled: Mutex::default() }),
        }
    }

    pub async fn server_version(&self) -> DbResult<String> {
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        let v: Option<String> = c.query_first("SELECT VERSION()").await.map_err(server_err)?;
        Ok(v.unwrap_or_default())
    }

    pub async fn close(&self) {
        let _ = self.pool.clone().disconnect().await;
    }

    pub async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let mut conn = self.pool.get_conn().await.map_err(server_err)?;
        let thread_id = conn.id();
        self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(qid, thread_id);

        let outcome = self.run(&mut conn, sql, limit).await;

        self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&qid);
        let was_cancelled = self.cancelled.lock().unwrap_or_else(|p| p.into_inner()).remove(&qid);
        // A killed SLEEP() "succeeds" with 1, so a cancel overrides the outcome.
        if was_cancelled {
            return Err(DbError::Cancelled);
        }
        match outcome {
            Ok((mut result, cut_short)) => {
                result.elapsed_ms = started.elapsed().as_millis() as u64;
                if cut_short {
                    // The server is still sending rows we won't read. Ending
                    // the connection is quicker than draining them.
                    self.kill_connection(thread_id).await;
                    drop(conn);
                }
                Ok(result)
            }
            Err(e) => Err(server_err(e)),
        }
    }

    /// Returns the result and whether reading stopped before the end.
    async fn run(&self, conn: &mut mysql_async::Conn, sql: &str, limit: usize) -> Result<(QueryResult, bool), mysql_async::Error> {
        let mut res = conn.query_iter(sql).await?;
        let cols: Vec<Column> = res.columns_ref().iter().map(describe_column).collect();

        let mut out = QueryResult { columns: cols, ..Default::default() };
        let mut cut_short = false;
        if !out.columns.is_empty() {
            while let Some(row) = res.next().await? {
                if out.rows.len() >= limit {
                    out.truncated = true;
                    cut_short = true;
                    break;
                }
                out.rows.push(
                    (0..out.columns.len())
                        .map(|i| to_json(row.as_ref(i).unwrap_or(&MyValue::NULL), &out.columns[i]))
                        .collect(),
                );
            }
        }
        if !cut_short {
            out.affected_rows = if out.columns.is_empty() { Some(res.affected_rows()) } else { None };
            out.last_insert_id = res.last_insert_id().filter(|id| *id != 0);
            // Further result sets (multi-statement input) are not shown.
            res.drop_result().await?;
        }
        Ok((out, cut_short))
    }

    async fn kill_connection(&self, thread_id: u32) {
        if let Ok(mut other) = self.pool.get_conn().await {
            let _ = other.query_drop(format!("KILL {thread_id}")).await;
        }
    }

    pub async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        let id = self.running.lock().unwrap_or_else(|p| p.into_inner()).get(&qid).copied();
        let Some(thread_id) = id else { return Ok(()) };
        self.cancelled.lock().unwrap_or_else(|p| p.into_inner()).insert(qid);
        let mut other = self.pool.get_conn().await.map_err(server_err)?;
        other.query_drop(format!("KILL QUERY {thread_id}")).await.map_err(server_err)?;
        Ok(())
    }

    pub async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        match path {
            [] => {
                let names: Vec<String> = c.query("SHOW DATABASES").await.map_err(server_err)?;
                let mut nodes: Vec<TreeNode> = names
                    .into_iter()
                    .map(|name| {
                        let system = SYSTEM_SCHEMAS.contains(&name.to_ascii_lowercase().as_str());
                        TreeNode {
                            name,
                            kind: NodeKind::Database,
                            detail: system.then(|| "system".to_string()),
                            expandable: true,
                        }
                    })
                    .collect();
                // Your own databases first, the server's own last.
                nodes.sort_by_key(|n| (n.detail.is_some(), n.name.to_lowercase()));
                Ok(nodes)
            }
            [db] => {
                let rows: Vec<(String, String, Option<u64>)> = c
                    .exec(
                        "SELECT TABLE_NAME, TABLE_TYPE, TABLE_ROWS FROM information_schema.TABLES \
                         WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
                        (db,),
                    )
                    .await
                    .map_err(server_err)?;
                let mut nodes: Vec<TreeNode> = rows
                    .into_iter()
                    .map(|(name, ty, rows)| {
                        let view = ty.eq_ignore_ascii_case("VIEW");
                        TreeNode {
                            name,
                            kind: if view { NodeKind::View } else { NodeKind::Table },
                            detail: if view { Some("view".into()) } else { rows.map(|n| format!("~{n} rows")) },
                            expandable: true,
                        }
                    })
                    .collect();
                // Routines and events come after the tables: the window groups them by kind.
                let routines: Vec<(String, String)> = c
                    .exec("SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME", (db,))
                    .await
                    .map_err(server_err)?;
                nodes.extend(routines.into_iter().map(|(name, ty)| TreeNode {
                    name,
                    kind: if ty.eq_ignore_ascii_case("PROCEDURE") { NodeKind::Procedure } else { NodeKind::Function },
                    detail: None,
                    expandable: false,
                }));
                // Events don't exist on every server (or are not readable by every account): leave them out then.
                let events: Vec<String> = c
                    .exec("SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME", (db,))
                    .await
                    .unwrap_or_default();
                nodes.extend(events.into_iter().map(|name| TreeNode { name, kind: NodeKind::Event, detail: None, expandable: false }));
                Ok(nodes)
            }
            [db, table] => {
                let info = self.table_info_with(&mut c, db, table).await?;
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
                let idx: Vec<(String, u64, String)> = c
                    .exec(
                        "SELECT INDEX_NAME, MIN(NON_UNIQUE), GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) \
                         FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                         GROUP BY INDEX_NAME ORDER BY INDEX_NAME",
                        (db, table),
                    )
                    .await
                    .map_err(server_err)?;
                nodes.extend(idx.into_iter().map(|(name, non_unique, cols)| TreeNode {
                    name,
                    kind: NodeKind::Index,
                    detail: Some(format!("{}({cols})", if non_unique == 0 { "unique " } else { "" })),
                    expandable: false,
                }));
                let trg: Vec<(String, String, String)> = c
                    .exec(
                        "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION FROM information_schema.TRIGGERS \
                         WHERE EVENT_OBJECT_SCHEMA = ? AND EVENT_OBJECT_TABLE = ? ORDER BY TRIGGER_NAME",
                        (db, table),
                    )
                    .await
                    .map_err(server_err)?;
                nodes.extend(trg.into_iter().map(|(name, timing, event)| TreeNode {
                    name,
                    kind: NodeKind::Trigger,
                    detail: Some(format!("{timing} {event}")),
                    expandable: false,
                }));
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    pub async fn table_info(&self, database: &str, table: &str) -> DbResult<TableInfo> {
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        self.table_info_with(&mut c, database, table).await
    }

    async fn table_info_with(&self, c: &mut mysql_async::Conn, db: &str, table: &str) -> DbResult<TableInfo> {
        let cols: Vec<(String, String, String, Option<String>)> = c
            .exec(
                "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        let pk: Vec<String> = c
            .exec(
                "SELECT COLUMN_NAME FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY' ORDER BY SEQ_IN_INDEX",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        Ok(TableInfo {
            columns: cols
                .into_iter()
                .map(|(name, data_type, nullable, default)| ColumnInfo {
                    primary_key: pk.contains(&name),
                    name,
                    data_type,
                    nullable: nullable.eq_ignore_ascii_case("YES"),
                    default,
                })
                .collect(),
            primary_key: pk,
        })
    }

    pub async fn table_meta(&self, db: &str, table: &str) -> DbResult<TableMeta> {
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        let head: Option<(String, String, Option<u64>, Option<u64>, Option<u64>)> = c
            .exec_first(
                "SELECT TABLE_TYPE, TABLE_COMMENT, TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        let Some((ty, comment, rows, data, index)) = head else {
            return Err(DbError::Invalid(format!("{db}.{table} was not found; it may have been dropped")));
        };
        let view = ty.eq_ignore_ascii_case("VIEW");

        let indexes: Vec<IndexMeta> = c
            .exec::<(String, i64, String, String), _, _>(
                "SELECT INDEX_NAME, MIN(NON_UNIQUE), GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ','), MIN(INDEX_TYPE) \
                 FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? GROUP BY INDEX_NAME ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME",
                (db, table),
            )
            .await
            .map_err(server_err)?
            .into_iter()
            .map(|(name, non_unique, cols, method)| IndexMeta {
                primary: name == "PRIMARY",
                unique: non_unique == 0,
                columns: cols.split(',').map(str::to_string).collect(),
                method: Some(method.to_lowercase()),
                definition: None,
                size_bytes: None,
                name,
            })
            .collect();

        let kinds: Vec<(String, String)> = c
            .exec(
                "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        let usage: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = c
            .exec(
                "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        let rules: Vec<(String, String, String)> = c
            .exec(
                "SELECT CONSTRAINT_NAME, UPDATE_RULE, DELETE_RULE FROM information_schema.REFERENTIAL_CONSTRAINTS WHERE CONSTRAINT_SCHEMA = ? AND TABLE_NAME = ?",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        // CHECK constraints are only in newer servers; an older one just has none to show.
        let checks: Vec<(String, String)> = c
            .exec(
                "SELECT cc.CONSTRAINT_NAME, cc.CHECK_CLAUSE FROM information_schema.CHECK_CONSTRAINTS cc \
                 JOIN information_schema.TABLE_CONSTRAINTS tc ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA AND tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME \
                 WHERE tc.TABLE_SCHEMA = ? AND tc.TABLE_NAME = ?",
                (db, table),
            )
            .await
            .unwrap_or_default();
        let action = |a: &str| (!a.eq_ignore_ascii_case("NO ACTION") && !a.eq_ignore_ascii_case("RESTRICT")).then(|| a.to_uppercase());
        let constraints: Vec<ConstraintMeta> = kinds
            .iter()
            .map(|(name, ty)| {
                let rows: Vec<_> = usage.iter().filter(|u| &u.0 == name).collect();
                let columns: Vec<String> = rows.iter().map(|u| u.1.clone()).collect();
                let kind = match ty.as_str() {
                    "PRIMARY KEY" => ConstraintKind::PrimaryKey,
                    "FOREIGN KEY" => ConstraintKind::ForeignKey,
                    "UNIQUE" => ConstraintKind::Unique,
                    _ => ConstraintKind::Check,
                };
                let references = (kind == ConstraintKind::ForeignKey).then(|| {
                    let rule = rules.iter().find(|r| &r.0 == name);
                    ForeignRef {
                        schema: rows.first().and_then(|u| u.2.clone()).unwrap_or_default(),
                        table: rows.first().and_then(|u| u.3.clone()).unwrap_or_default(),
                        columns: rows.iter().filter_map(|u| u.4.clone()).collect(),
                        on_update: rule.and_then(|r| action(&r.1)),
                        on_delete: rule.and_then(|r| action(&r.2)),
                    }
                });
                let definition = checks.iter().find(|(n, _)| n == name).map(|(_, clause)| format!("CHECK {clause}"));
                ConstraintMeta { name: name.clone(), kind, columns, definition, references }
            })
            .collect();

        let incoming: Vec<(String, String, String, String, String, String)> = c
            .exec(
                "SELECT kcu.CONSTRAINT_NAME, kcu.TABLE_SCHEMA, kcu.TABLE_NAME, kcu.COLUMN_NAME, kcu.REFERENCED_COLUMN_NAME, kcu.CONSTRAINT_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE kcu \
                 WHERE kcu.REFERENCED_TABLE_SCHEMA = ? AND kcu.REFERENCED_TABLE_NAME = ? ORDER BY kcu.TABLE_SCHEMA, kcu.TABLE_NAME, kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION",
                (db, table),
            )
            .await
            .map_err(server_err)?;
        let mut referenced_by: Vec<IncomingRef> = Vec::new();
        for (name, schema, tbl, col, rcol, _) in incoming {
            match referenced_by.iter_mut().find(|r| r.name == name && r.schema == schema && r.table == tbl) {
                Some(r) => {
                    r.columns.push(col);
                    r.referenced_columns.push(rcol);
                }
                None => referenced_by.push(IncomingRef { name, schema, table: tbl, columns: vec![col], referenced_columns: vec![rcol], on_update: None, on_delete: None }),
            }
        }

        let column_notes: Vec<(String, String)> = c
            .exec::<(String, String), _, _>(
                "SELECT COLUMN_NAME, TRIM(EXTRA) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND TRIM(EXTRA) <> '' ORDER BY ORDINAL_POSITION",
                (db, table),
            )
            .await
            .map_err(server_err)?;

        // The server writes its own definition; it is shown as it came.
        let create: Option<(String, String)> = c
            .query_first(format!("SHOW CREATE {} {}.{}", if view { "VIEW" } else { "TABLE" }, quote_ident(db), quote_ident(table)))
            .await
            .map_err(server_err)
            .ok()
            .flatten();
        let ddl = create.map(|(_, text)| Ddl { text, reconstructed: false });

        Ok(TableMeta {
            supported: true,
            kind: Some(if view { "view" } else { "table" }.into()),
            comment: Some(comment).filter(|c| !c.is_empty()),
            approx_rows: if view { None } else { rows.map(|n| n as i64) },
            size_bytes: if view { None } else { Some((data.unwrap_or(0) + index.unwrap_or(0)) as i64) },
            indexes,
            constraints,
            referenced_by,
            column_notes,
            ddl,
        })
    }

    pub async fn search(&self, term: &str, limit: usize) -> DbResult<Vec<SearchHit>> {
        let pattern = format!("%{}%", meta::like_escape(term));
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        let tables: Vec<(String, String, String)> = c
            .exec(
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_NAME LIKE ? \
                 ORDER BY TABLE_SCHEMA IN ('information_schema', 'mysql', 'performance_schema', 'sys'), TABLE_SCHEMA, TABLE_NAME LIMIT ?",
                (&pattern, limit as u64),
            )
            .await
            .map_err(server_err)?;
        let mut hits: Vec<SearchHit> = tables
            .into_iter()
            .map(|(schema, name, ty)| {
                let view = ty.eq_ignore_ascii_case("VIEW");
                SearchHit { parent: vec![schema], name, kind: if view { NodeKind::View } else { NodeKind::Table }, detail: view.then(|| "view".to_string()) }
            })
            .collect();
        if hits.len() < limit {
            let routines: Vec<(String, String, String)> = c
                .exec(
                    "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_NAME LIKE ? \
                     ORDER BY ROUTINE_SCHEMA, ROUTINE_NAME LIMIT ?",
                    (&pattern, (limit - hits.len()) as u64),
                )
                .await
                .map_err(server_err)?;
            hits.extend(routines.into_iter().map(|(schema, name, ty)| SearchHit {
                parent: vec![schema],
                name,
                kind: if ty.eq_ignore_ascii_case("PROCEDURE") { NodeKind::Procedure } else { NodeKind::Function },
                detail: None,
            }));
        }
        Ok(hits)
    }

    /// Apply a reviewed set in one transaction; a change that doesn't touch exactly one row stops it and the
    /// transaction is rolled back. (A table on an engine without transactions, such as MyISAM, can't be undone.)
    pub async fn apply_changes(&self, changes: &[Change]) -> DbResult<Vec<u64>> {
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        let mut tx = c.start_transaction(mysql_async::TxOpts::default()).await.map_err(server_err)?;
        let mut touched = Vec::with_capacity(changes.len());
        for (i, ch) in changes.iter().enumerate() {
            let (pieces, cells) = change_pieces(ch, quote_ident, " LIMIT 1", "() VALUES ()");
            let sql = join_with(&pieces, |_| "?".to_string());
            let params: Vec<MyValue> = cells.into_iter().map(|c| c.value.clone().map_or(MyValue::NULL, |s| MyValue::Bytes(s.into_bytes()))).collect();
            if let Err(e) = tx.exec_drop(sql, params).await {
                let _ = tx.rollback().await;
                return Err(match server_err(e) {
                    DbError::Server(m) => DbError::Server(format!("change {} of {}: {m}", i + 1, changes.len())),
                    other => other,
                });
            }
            let n = tx.affected_rows();
            if n != 1 {
                let _ = tx.rollback().await;
                return Err(DbError::Invalid(format!(
                    "change {} of {} touched {n} rows instead of 1 (the row may have been changed or deleted since it was loaded). Nothing was saved.",
                    i + 1,
                    changes.len()
                )));
            }
            touched.push(n);
        }
        tx.commit().await.map_err(server_err)?;
        Ok(touched)
    }

    pub async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        let (sql, params) = build_update(edit);
        let mut c = self.pool.get_conn().await.map_err(server_err)?;
        c.exec_drop(sql, params).await.map_err(server_err)?;
        Ok(c.affected_rows())
    }
}

pub fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// A literal for display only. Escapes backslashes and quotes the way the
/// default SQL mode reads them.
pub fn quote_literal(value: &Option<String>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('\'');
            for ch in s.chars() {
                match ch {
                    '\'' => out.push_str("''"),
                    '\\' => out.push_str("\\\\"),
                    '\0' => out.push_str("\\0"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\x1a' => out.push_str("\\Z"),
                    c => out.push(c),
                }
            }
            out.push('\'');
            out
        }
    }
}

/// The statements of a set with the values written in, for showing. What runs uses bound parameters.
pub fn preview_changes(changes: &[Change]) -> Vec<String> {
    changes
        .iter()
        .map(|ch| {
            let (pieces, cells) = change_pieces(ch, quote_ident, " LIMIT 1", "() VALUES ()");
            format!("{};", join_with(&pieces, |i| quote_literal(&cells[i].value)))
        })
        .collect()
}

pub fn preview_update(edit: &RowEdit) -> String {
    let (pieces, cells) = update_pieces(edit, quote_ident, " LIMIT 1");
    join_with(&pieces, |i| quote_literal(&cells[i].value))
}

fn build_update(edit: &RowEdit) -> (String, Vec<MyValue>) {
    let (pieces, cells) = update_pieces(edit, quote_ident, " LIMIT 1");
    (
        join_with(&pieces, |_| "?".to_string()),
        cells.into_iter().map(|c| c.value.clone().map_or(MyValue::NULL, |s| MyValue::Bytes(s.into_bytes()))).collect(),
    )
}

fn type_name(t: ColumnType) -> &'static str {
    use ColumnType::*;
    match t {
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => "decimal",
        MYSQL_TYPE_TINY => "tinyint",
        MYSQL_TYPE_SHORT => "smallint",
        MYSQL_TYPE_LONG => "int",
        MYSQL_TYPE_INT24 => "mediumint",
        MYSQL_TYPE_LONGLONG => "bigint",
        MYSQL_TYPE_FLOAT => "float",
        MYSQL_TYPE_DOUBLE => "double",
        MYSQL_TYPE_YEAR => "year",
        MYSQL_TYPE_BIT => "bit",
        MYSQL_TYPE_NULL => "null",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "timestamp",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "date",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "time",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "datetime",
        MYSQL_TYPE_JSON => "json",
        MYSQL_TYPE_ENUM => "enum",
        MYSQL_TYPE_SET => "set",
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => "varchar",
        MYSQL_TYPE_STRING => "char",
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => "blob",
        MYSQL_TYPE_GEOMETRY => "geometry",
        _ => "other",
    }
}

fn is_binary(c: &mysql_async::Column) -> bool {
    c.character_set() == BINARY_CHARSET
}

fn kind_of(c: &mysql_async::Column) -> ColumnKind {
    use ColumnType::*;
    match c.column_type() {
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL | MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_LONG
        | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONGLONG | MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE | MYSQL_TYPE_YEAR => ColumnKind::Number,
        MYSQL_TYPE_BIT | MYSQL_TYPE_GEOMETRY => ColumnKind::Binary,
        MYSQL_TYPE_JSON => ColumnKind::Json,
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 | MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE | MYSQL_TYPE_TIME
        | MYSQL_TYPE_TIME2 | MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => ColumnKind::DateTime,
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB | MYSQL_TYPE_VARCHAR
        | MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_STRING => {
            if is_binary(c) {
                ColumnKind::Binary
            } else {
                ColumnKind::Text
            }
        }
        MYSQL_TYPE_ENUM | MYSQL_TYPE_SET => ColumnKind::Text,
        _ => ColumnKind::Other,
    }
}

fn describe_column(c: &mysql_async::Column) -> Column {
    let mut data_type = type_name(c.column_type()).to_string();
    // TEXT types share the BLOB wire type; the character set tells them apart.
    if data_type == "blob" && !is_binary(c) {
        data_type = "text".into();
    }
    Column { name: c.name_str().to_string(), data_type, kind: kind_of(c) }
}

/// Largest whole number a JSON reader (a browser) holds exactly.
const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;

fn to_json(v: &MyValue, col: &Column) -> Value {
    let bytes = match v {
        MyValue::NULL => return Value::Null,
        MyValue::Bytes(b) => b.as_slice(),
        // The text protocol sends everything as bytes; these only appear
        // for binary-protocol results, which queries here don't use.
        other => return Value::String(other.as_sql(true).trim_matches('\'').to_string()),
    };
    match col.kind {
        ColumnKind::Number => number(bytes, &col.data_type),
        ColumnKind::Binary => {
            let shown = &bytes[..bytes.len().min(256)];
            let mut hex = String::from("0x");
            for b in shown {
                hex.push_str(&format!("{b:02x}"));
            }
            if bytes.len() > shown.len() {
                hex.push_str(&format!("… ({} bytes)", bytes.len()));
            }
            // Binary is never editable, so it needs no truncation marker.
            Value::String(hex)
        }
        _ => text(bytes),
    }
}

fn number(bytes: &[u8], data_type: &str) -> Value {
    let s = String::from_utf8_lossy(bytes);
    // DECIMAL keeps its digits exactly, so it stays a string.
    if data_type != "decimal" {
        if let Ok(i) = s.parse::<i64>() {
            if i.abs() <= MAX_SAFE_INT {
                return json!(i);
            }
        } else if let Ok(f) = s.parse::<f64>() {
            if f.is_finite() && (data_type == "float" || data_type == "double") {
                return json!(f);
            }
        }
    }
    Value::String(s.into_owned())
}

fn text(bytes: &[u8]) -> Value {
    if bytes.len() <= MAX_CELL_BYTES {
        return Value::String(String::from_utf8_lossy(bytes).into_owned());
    }
    // Cut on a character boundary.
    let mut end = MAX_CELL_BYTES;
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    json!({ "truncated": true, "preview": String::from_utf8_lossy(&bytes[..end]), "bytes": bytes.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::CellEdit;

    fn edit(changes: &[(&str, Option<&str>)], key: &[(&str, Option<&str>)]) -> RowEdit {
        let cell = |(c, v): &(&str, Option<&str>)| CellEdit { column: c.to_string(), value: v.map(str::to_string) };
        RowEdit {
            database: "shop".into(),
            table: "items".into(),
            key: key.iter().map(cell).collect(),
            changes: changes.iter().map(cell).collect(),
        }
    }

    #[test]
    fn quotes_names_and_values() {
        assert_eq!(quote_ident("a`b"), "`a``b`");
        assert_eq!(quote_literal(&None), "NULL");
        assert_eq!(quote_literal(&Some("it's".into())), "'it''s'");
        assert_eq!(quote_literal(&Some("a\\b\n".into())), "'a\\\\b\\n'");
    }

    #[test]
    fn preview_writes_the_values_in() {
        let e = edit(&[("name", Some("x'y")), ("note", None)], &[("id", Some("7"))]);
        assert_eq!(
            preview_update(&e),
            "UPDATE `shop`.`items` SET `name` = 'x''y', `note` = NULL WHERE `id` = '7' LIMIT 1"
        );
    }

    #[test]
    fn execution_uses_bound_parameters_only() {
        let e = edit(&[("name", Some("'; DROP TABLE items; --"))], &[("id", Some("7")), ("sub", Some("2"))]);
        let (sql, params) = build_update(&e);
        assert_eq!(sql, "UPDATE `shop`.`items` SET `name` = ? WHERE `id` = ? AND `sub` = ? LIMIT 1");
        assert!(!sql.contains("DROP"));
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], MyValue::Bytes(b"'; DROP TABLE items; --".to_vec()));
    }

    #[test]
    fn a_placeholder_in_a_value_cannot_shift_the_parameters() {
        // The shape is built before any value is written in.
        let e = edit(&[("a", Some("{}")), ("b", Some("2"))], &[("id", Some("1"))]);
        assert_eq!(
            preview_update(&e),
            "UPDATE `shop`.`items` SET `a` = '{}', `b` = '2' WHERE `id` = '1' LIMIT 1"
        );
    }

    #[test]
    fn numbers_stay_exact() {
        assert_eq!(number(b"42", "int"), json!(42));
        assert_eq!(number(b"-7", "bigint"), json!(-7));
        assert_eq!(number(b"9007199254740993", "bigint"), json!("9007199254740993"));
        assert_eq!(number(b"1.50", "decimal"), json!("1.50"));
        assert_eq!(number(b"2.5", "double"), json!(2.5));
        assert_eq!(number(b"nan", "double"), json!("nan"));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        let s = "é".repeat(MAX_CELL_BYTES);
        let v = text(s.as_bytes());
        assert_eq!(v["truncated"], json!(true));
        assert_eq!(v["bytes"], json!(s.len()));
        assert!(v["preview"].as_str().unwrap().len() <= MAX_CELL_BYTES);
        assert_eq!(text(b"short"), json!("short"));
    }

    #[test]
    fn edits_without_a_key_or_changes_are_refused() {
        assert!(edit(&[("a", Some("1"))], &[]).validate().is_err());
        assert!(edit(&[("a", Some("1"))], &[("id", None)]).validate().is_err());
        assert!(edit(&[], &[("id", Some("1"))]).validate().is_err());
        assert!(edit(&[("a", Some("1"))], &[("id", Some("1"))]).validate().is_ok());
    }
}
