//! rqlite: SQLite replicated by Raft, spoken to over its HTTP API. It is SQL, so it fits the same tree, grid and
//! inline edit as the other SQL engines.
//!
//! Reads go to `/db/query` with the chosen consistency `level` (none, weak, linearizable or strong), writes to
//! `/db/execute`; which is which is decided from the statement's first word. A connection is to the cluster, so the
//! tree has one schema, `main`, with the tables and views in it.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use uuid::Uuid;

use super::http::{Auth, Http};
use super::{
    join_with, update_pieces, Backend, Column, ColumnInfo, ColumnKind, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit, TableInfo,
    TreeNode, MAX_CELL_BYTES,
};

const LEVELS: [&str; 4] = ["none", "weak", "linearizable", "strong"];
const DEFAULT_LEVEL: &str = "weak";
/// Longest a statement may run before it is given up on.
const TIMEOUT: Duration = Duration::from_secs(120);
/// The one schema SQLite has.
const SCHEMA: &str = "main";

pub struct RqliteConn {
    http: Http,
    level: &'static str,
}

/// A name for SQLite: double quotes, doubled inside.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn literal(value: &Option<String>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(v) => format!("'{}'", v.replace('\'', "''")),
    }
}

/// Whether a statement only reads, from its first word (after comments and parentheses).
pub fn is_read(sql: &str) -> bool {
    let mut rest = sql.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = r.split_once('\n').map_or("", |(_, tail)| tail).trim_start();
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, tail)| tail).trim_start();
        } else if let Some(r) = rest.strip_prefix('(') {
            rest = r.trim_start();
        } else {
            break;
        }
    }
    let word: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    // A WITH may end in an INSERT, UPDATE or DELETE; those are writes.
    if word == "WITH" {
        let upper = rest.to_ascii_uppercase();
        return !["INSERT", "UPDATE", "DELETE", "REPLACE"].iter().any(|w| upper.split(|c: char| !c.is_ascii_alphabetic()).any(|t| t == *w));
    }
    matches!(word.as_str(), "SELECT" | "VALUES" | "EXPLAIN" | "PRAGMA")
}

fn kind_of(declared: &str) -> ColumnKind {
    let t = declared.to_ascii_lowercase();
    if t.contains("int") || t.contains("real") || t.contains("floa") || t.contains("doub") || t.contains("numeric") || t.contains("decimal") {
        ColumnKind::Number
    } else if t.contains("blob") {
        ColumnKind::Binary
    } else if t.contains("date") || t.contains("time") {
        ColumnKind::DateTime
    } else if t.contains("json") {
        ColumnKind::Json
    } else if t.contains("char") || t.contains("text") || t.contains("clob") {
        ColumnKind::Text
    } else {
        ColumnKind::Other
    }
}

fn server_err(status: u16, body: &[u8]) -> DbError {
    let text = String::from_utf8_lossy(body);
    let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
    match status {
        401 => DbError::Server("rqlite refused the user name or password (401)".into()),
        403 => DbError::Server("rqlite says this user may not do that (403)".into()),
        _ => DbError::Server(msg.unwrap_or_else(|| format!("rqlite answered {status}: {}", text.trim().chars().take(300).collect::<String>()))),
    }
}

fn number_or_text(v: &Value) -> Value {
    match v {
        Value::String(s) => super::cut_text(s),
        // A BLOB arrives as base64 text from rqlite; show it as text, flagged by the column kind.
        other => other.clone(),
    }
}

/// Turn one `results[i]` object into a [`QueryResult`], keeping at most `limit` rows.
fn result_of(r: &Value, limit: usize) -> DbResult<QueryResult> {
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        return Err(DbError::Server(e.to_string()));
    }
    let mut out = QueryResult::default();
    if let Some(cols) = r.get("columns").and_then(Value::as_array) {
        let types = r.get("types").and_then(Value::as_array);
        out.columns = cols
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let t = types.and_then(|t| t.get(i)).and_then(Value::as_str).unwrap_or("");
                Column { name: n.as_str().unwrap_or("").to_string(), data_type: t.to_string(), kind: kind_of(t) }
            })
            .collect();
        let values = r.get("values").and_then(Value::as_array).cloned().unwrap_or_default();
        out.truncated = values.len() > limit;
        out.rows = values.iter().take(limit).map(|row| row.as_array().map(|c| c.iter().map(number_or_text).collect()).unwrap_or_default()).collect();
    } else {
        out.affected_rows = Some(r.get("rows_affected").and_then(Value::as_u64).unwrap_or(0));
        out.last_insert_id = r.get("last_insert_id").and_then(Value::as_u64).filter(|id| *id > 0);
    }
    Ok(out)
}

impl RqliteConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        let auth = match (spec.user.is_empty(), &spec.password) {
            (false, Some(p)) => Auth::Basic { user: spec.user.clone(), password: p.clone() },
            (false, None) => Auth::Basic { user: spec.user.clone(), password: String::new() },
            _ => Auth::None,
        };
        let level = match spec.option("level") {
            None => DEFAULT_LEVEL,
            Some(l) => LEVELS.iter().copied().find(|x| x.eq_ignore_ascii_case(l)).ok_or_else(|| DbError::Invalid(format!("the read consistency must be one of: {}", LEVELS.join(", "))))?,
        };
        let me = Self { http: Http::new(spec, auth)?, level };
        // Fail now, with the real reason: the status page answers without touching the data.
        me.version().await?;
        Ok(me)
    }

    async fn version(&self) -> DbResult<String> {
        let r = self.http.send(None, "GET", "/status", None, Duration::from_secs(10)).await?;
        if r.status != 200 {
            return Err(server_err(r.status, &r.body));
        }
        let v: Value = serde_json::from_slice(&r.body).map_err(|_| DbError::Server("that address answered, but not like rqlite (the status page isn't JSON)".into()))?;
        v.pointer("/build/version").and_then(Value::as_str).map(|s| s.trim_start_matches('v').to_string()).ok_or_else(|| DbError::Server("that address answered, but not like rqlite (no version in its status)".into()))
    }

    /// Run one statement. `params` are bound, never written into the text.
    async fn run(&self, qid: Option<Uuid>, sql: &str, params: &[Value], limit: usize) -> DbResult<QueryResult> {
        let read = is_read(sql);
        let mut stmt = vec![Value::String(sql.to_string())];
        stmt.extend(params.iter().cloned());
        let body = serde_json::to_vec(&json!([stmt])).map_err(|e| DbError::Invalid(e.to_string()))?;
        let path = if read { format!("/db/query?level={}&timings", self.level) } else { "/db/execute?timings".to_string() };
        let reply = self.http.send(qid, "POST", &path, Some(body), TIMEOUT).await?;
        if reply.status != 200 {
            return Err(server_err(reply.status, &reply.body));
        }
        let v: Value = serde_json::from_slice(&reply.body).map_err(|e| DbError::Server(format!("rqlite's answer wasn't JSON: {e}")))?;
        let first = v.get("results").and_then(Value::as_array).and_then(|a| a.first()).ok_or_else(|| DbError::Server("rqlite's answer had no result".into()))?;
        result_of(first, limit)
    }
}

pub fn preview_update(edit: &RowEdit) -> String {
    let (pieces, cells) = update_pieces(edit, quote_ident, "");
    join_with(&pieces, |i| literal(&cells[i].value))
}

#[async_trait::async_trait]
impl Backend for RqliteConn {
    async fn server_version(&self) -> DbResult<String> {
        Ok(format!("rqlite {}", self.version().await?))
    }

    async fn query(&self, qid: Uuid, sql: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let mut out = self.run(Some(qid), sql, &[], limit).await?;
        out.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(out)
    }

    async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        // The request is dropped; rqlite finishes or abandons the statement on its own.
        self.http.cancel(qid);
        Ok(())
    }

    async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        match path {
            [] => Ok(vec![TreeNode { name: SCHEMA.into(), kind: NodeKind::Schema, detail: None, expandable: true }]),
            [_] => {
                let r = self.run(None, "SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY type, name", &[], 10_000).await?;
                Ok(r.rows
                    .iter()
                    .filter_map(|row| {
                        let name = row.first()?.as_str()?.to_string();
                        let view = row.get(1)?.as_str()? == "view";
                        Some(TreeNode { name, kind: if view { NodeKind::View } else { NodeKind::Table }, detail: None, expandable: true })
                    })
                    .collect())
            }
            [_, table] => {
                let cols = self.run(None, &format!("PRAGMA table_info({})", quote_ident(table)), &[], 10_000).await?;
                let mut nodes: Vec<TreeNode> = cols
                    .rows
                    .iter()
                    .filter_map(|r| {
                        let name = r.get(1)?.as_str()?.to_string();
                        let ty = r.get(2)?.as_str().unwrap_or("").to_string();
                        let pk = r.get(5).and_then(Value::as_i64).unwrap_or(0) > 0;
                        let detail = [Some(ty).filter(|t| !t.is_empty()), pk.then(|| "PK".to_string())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        Some(TreeNode { name, kind: NodeKind::Column, detail: Some(detail).filter(|d| !d.is_empty()), expandable: false })
                    })
                    .collect();
                let idx = self.run(None, &format!("PRAGMA index_list({})", quote_ident(table)), &[], 1000).await?;
                nodes.extend(idx.rows.iter().filter_map(|r| {
                    let name = r.get(1)?.as_str()?.to_string();
                    let unique = r.get(2).and_then(Value::as_i64).unwrap_or(0) == 1;
                    Some(TreeNode { name, kind: NodeKind::Index, detail: unique.then(|| "unique".to_string()), expandable: false })
                }));
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn table_info(&self, _schema: &str, table: &str) -> DbResult<TableInfo> {
        let r = self.run(None, &format!("PRAGMA table_info({})", quote_ident(table)), &[], 10_000).await?;
        let mut pk: Vec<(i64, String)> = Vec::new();
        let columns: Vec<ColumnInfo> = r
            .rows
            .iter()
            .filter_map(|row| {
                let name = row.get(1)?.as_str()?.to_string();
                let key = row.get(5).and_then(Value::as_i64).unwrap_or(0);
                if key > 0 {
                    pk.push((key, name.clone()));
                }
                Some(ColumnInfo {
                    name,
                    data_type: row.get(2)?.as_str().unwrap_or("").to_string(),
                    nullable: row.get(3).and_then(Value::as_i64).unwrap_or(0) == 0 && key == 0,
                    primary_key: key > 0,
                    default: row.get(4).and_then(Value::as_str).map(str::to_string),
                })
            })
            .collect();
        pk.sort();
        Ok(TableInfo { columns, primary_key: pk.into_iter().map(|(_, n)| n).collect() })
    }

    fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        Ok(preview_update(edit))
    }

    async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        for c in edit.changes.iter().chain(&edit.key) {
            if c.value.as_deref().is_some_and(|v| v.contains('\0') || v.len() > MAX_CELL_BYTES * 16) {
                return Err(DbError::Invalid("that value can't be sent".into()));
            }
        }
        // Values are bound, in the order of the gaps; `LIMIT 1` is not valid on every SQLite build, so the key decides.
        let (pieces, cells) = update_pieces(edit, quote_ident, "");
        let sql = join_with(&pieces, |_| "?".to_string());
        let params: Vec<Value> = cells.iter().map(|c| c.value.clone().map_or(Value::Null, Value::String)).collect();
        Ok(self.run(None, &sql, &params, 1).await?.affected_rows.unwrap_or(0))
    }

    async fn close(&self) {}

    fn is_sql(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
