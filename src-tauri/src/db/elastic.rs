//! Elasticsearch (and OpenSearch), over its HTTP API, with basic, API-key or bearer-token sign-in.
//!
//! The console takes a request the way Kibana's does: a first line `METHOD /path?query`, then the JSON body. Search
//! hits come back as rows (`_id`, `_index`, `_score` and the `_source` fields as columns), `_cat` and other lists of
//! objects as rows too, anything else as one row of its top-level fields. The tree is the cluster, its indices (with
//! document counts and size), and each index's fields. Opening an index writes its search for you.

use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use uuid::Uuid;

use super::http::{Auth, Http};
use super::{table_from_objects, Backend, ColumnInfo, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit, TableInfo, TreeNode};

const TIMEOUT: Duration = Duration::from_secs(120);
const METHODS: [&str; 5] = ["GET", "POST", "PUT", "DELETE", "HEAD"];

pub struct ElasticConn {
    http: Http,
    cluster: String,
}

/// A request from the console.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: String,
    /// Always starts with `/`.
    pub path: String,
    pub body: Option<String>,
}

pub fn parse_request(text: &str) -> Result<Request, String> {
    let text = text.trim_start_matches(['\n', '\r', ' ', '\t']);
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let mut words = first.split_whitespace();
    let method = words.next().ok_or("type a request, like GET /_cat/indices")?.to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return Err(format!("the request starts with a method ({}), then a path: GET /my-index/_search", METHODS.join(", ")));
    }
    let path = words.next().ok_or("the method needs a path: GET /my-index/_search")?;
    if words.next().is_some() {
        return Err("the first line is only the method and the path; the JSON goes on the next lines".into());
    }
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    if path.contains(['\r', '\n', ' ']) || path.starts_with("//") {
        return Err("that path can't be used".into());
    }
    let body = rest.trim();
    Ok(Request { method, path, body: (!body.is_empty()).then(|| body.to_string()) })
}

/// A path segment, percent-encoded so an id with a slash or a space stays one segment.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path without its query string, and the query string.
fn split_path(path: &str) -> (&str, &str) {
    path.split_once('?').unwrap_or((path, ""))
}

/// Why a request deletes or rewrites data (asked before it runs), or none.
pub fn destructive(req: &Request) -> Option<String> {
    let (path, _) = split_path(&req.path);
    let last = path.rsplit('/').next().unwrap_or("");
    let body = req.body.as_deref().unwrap_or("");
    if req.method == "DELETE" {
        return Some(if path.contains("/_doc/") || path.contains("/_source/") { "This deletes a document." } else if path.starts_with("/_snapshot") { "This deletes a snapshot or repository." } else { "This deletes an index (or what the name matches) and everything in it." }.into());
    }
    let reason = match last {
        "_delete_by_query" => "This deletes every document that matches.",
        "_update_by_query" => "This rewrites every document that matches.",
        "_close" => "This closes the index, so it can't be searched or written to.",
        "_forcemerge" | "_shrink" | "_split" | "_rollover" => "This rewrites or replaces the index and can be slow on a big one.",
        "_reindex" => "This copies documents into another index and can overwrite what is there.",
        "_settings" | "_cluster" if req.method != "GET" => "This changes settings.",
        "_aliases" if body.contains("remove_index") => "This removes an index.",
        "_bulk" if body.lines().any(|l| l.trim_start().starts_with("{\"delete\"") || l.trim_start().starts_with("{ \"delete\"")) => "This deletes documents.",
        "_restore" => "This restores a snapshot over existing indices.",
        _ if path.starts_with("/_cluster/settings") && req.method != "GET" => "This changes settings of the whole cluster.",
        _ => return None,
    };
    Some(reason.to_string())
}

fn error_text(status: u16, body: &[u8]) -> DbError {
    let text = String::from_utf8_lossy(body);
    let v: Option<Value> = serde_json::from_str(&text).ok();
    match status {
        401 => return DbError::Server("Elasticsearch refused the sign-in (401): check the user name and password, or the API key".into()),
        403 => {
            let why = v.as_ref().and_then(|v| v.pointer("/error/reason")).and_then(Value::as_str).unwrap_or("this user may not do that");
            return DbError::Server(format!("Elasticsearch says no (403): {why}"));
        }
        _ => {}
    }
    let detail = v.as_ref().and_then(|v| {
        let e = v.get("error")?;
        if let Some(s) = e.as_str() {
            return Some(s.to_string());
        }
        let ty = e.get("type").and_then(Value::as_str).unwrap_or("error");
        let reason = e.get("reason").and_then(Value::as_str).unwrap_or("");
        // The root cause is usually the useful part (a parse error, a missing field).
        let cause = e.pointer("/root_cause/0/reason").and_then(Value::as_str).filter(|c| *c != reason);
        Some(match cause {
            Some(c) => format!("{ty}: {reason} ({c})"),
            None => format!("{ty}: {reason}"),
        })
    });
    DbError::Server(detail.unwrap_or_else(|| format!("Elasticsearch answered {status}: {}", text.trim().chars().take(300).collect::<String>())))
}

/// A reply as a grid.
pub fn lay_out(reply: &Value, limit: usize) -> QueryResult {
    // A search: one row per hit.
    if let Some(hits) = reply.pointer("/hits/hits").and_then(Value::as_array) {
        let multi = hits.iter().filter_map(|h| h.get("_index").and_then(Value::as_str)).collect::<std::collections::HashSet<_>>().len() > 1;
        let objs: Vec<Map<String, Value>> = hits
            .iter()
            .take(limit)
            .map(|h| {
                let mut o = Map::new();
                o.insert("_id".into(), h.get("_id").cloned().unwrap_or(Value::Null));
                if multi {
                    o.insert("_index".into(), h.get("_index").cloned().unwrap_or(Value::Null));
                }
                if let Some(s) = h.get("_score").filter(|s| !s.is_null()) {
                    o.insert("_score".into(), s.clone());
                }
                if let Some(Value::Object(src)) = h.get("_source") {
                    for (k, v) in src {
                        o.insert(if o.contains_key(k) { format!("_source.{k}") } else { k.clone() }, v.clone());
                    }
                } else if let Some(Value::Object(f)) = h.get("fields") {
                    for (k, v) in f {
                        // `fields` values are always arrays; one value reads better as itself.
                        o.insert(k.clone(), match v.as_array() {
                            Some(a) if a.len() == 1 => a[0].clone(),
                            _ => v.clone(),
                        });
                    }
                }
                o
            })
            .collect();
        let (columns, rows) = table_from_objects(&objs, &["_id", "_index", "_score"]);
        let total = reply.pointer("/hits/total/value").and_then(Value::as_u64).or_else(|| reply.pointer("/hits/total").and_then(Value::as_u64)).unwrap_or(hits.len() as u64);
        if rows.is_empty() && reply.get("aggregations").is_some() {
            return aggregations(reply);
        }
        return QueryResult { columns, rows, truncated: total as usize > hits.len().min(limit) || hits.len() > limit, ..Default::default() };
    }
    match reply {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {
            let objs: Vec<Map<String, Value>> = items.iter().take(limit).filter_map(|i| i.as_object().cloned()).collect();
            let (columns, rows) = table_from_objects(&objs, &[]);
            QueryResult { columns, rows, truncated: items.len() > limit, ..Default::default() }
        }
        Value::Object(map) => {
            let (columns, rows) = table_from_objects(std::slice::from_ref(map), &[]);
            QueryResult { columns, rows, ..Default::default() }
        }
        other => {
            let (columns, rows) = table_from_objects(&[Map::from_iter([("result".to_string(), other.clone())])], &[]);
            QueryResult { columns, rows, ..Default::default() }
        }
    }
}

fn aggregations(reply: &Value) -> QueryResult {
    let mut o = Map::new();
    if let Some(Value::Object(a)) = reply.get("aggregations") {
        for (k, v) in a {
            o.insert(k.clone(), v.clone());
        }
    }
    let (columns, rows) = table_from_objects(&[o], &[]);
    QueryResult { columns, rows, ..Default::default() }
}

impl ElasticConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        let secret = spec.password.clone().filter(|p| !p.is_empty());
        let auth = match (spec.option("auth").map(str::to_ascii_lowercase).as_deref(), &secret) {
            (Some("api_key"), Some(k)) => Auth::ApiKey(k.clone()),
            (Some("bearer"), Some(t)) => Auth::Bearer(t.clone()),
            (Some("api_key" | "bearer"), None) => return Err(DbError::Invalid("enter the API key or token as the password".into())),
            (None | Some("basic"), Some(p)) if !spec.user.is_empty() => Auth::Basic { user: spec.user.clone(), password: p.clone() },
            (None | Some("basic"), None) if !spec.user.is_empty() => Auth::Basic { user: spec.user.clone(), password: String::new() },
            (None | Some("basic"), _) => Auth::None,
            (Some(other), _) => return Err(DbError::Invalid(format!("the sign-in kind \"{other}\" isn't one of basic, api_key and bearer"))),
        };
        let mut me = Self { http: Http::new(spec, auth)?, cluster: String::new() };
        let info = me.get("/").await?;
        me.cluster = info.get("cluster_name").and_then(Value::as_str).unwrap_or("cluster").to_string();
        Ok(me)
    }

    async fn get(&self, path: &str) -> DbResult<Value> {
        self.call(None, "GET", path, None).await
    }

    async fn call(&self, qid: Option<Uuid>, method: &str, path: &str, body: Option<String>) -> DbResult<Value> {
        let r = self.http.send(qid, method, path, body.map(String::into_bytes), TIMEOUT).await?;
        if r.status >= 400 && !(method == "HEAD") {
            return Err(error_text(r.status, &r.body));
        }
        if method == "HEAD" {
            return Ok(json!({ "status": r.status, "exists": r.status < 400 }));
        }
        if r.body.is_empty() {
            return Ok(json!({ "status": r.status }));
        }
        serde_json::from_slice(&r.body).map_err(|_| match r.status {
            200..=299 => DbError::Server(format!("that address answered, but not like Elasticsearch (not JSON): {}", String::from_utf8_lossy(&r.body).chars().take(120).collect::<String>())),
            s => DbError::Server(format!("the server answered {s}")),
        })
    }

    async fn mapping_fields(&self, index: &str) -> DbResult<Vec<(String, String)>> {
        let m = self.get(&format!("/{}/_mapping", encode_segment(index))).await?;
        let mut fields: Vec<(String, String)> = Vec::new();
        if let Value::Object(by_index) = &m {
            for v in by_index.values() {
                if let Some(Value::Object(props)) = v.pointer("/mappings/properties") {
                    for (name, def) in props {
                        let ty = def.get("type").and_then(Value::as_str).unwrap_or(if def.get("properties").is_some() { "object" } else { "" });
                        if !fields.iter().any(|(n, _)| n == name) {
                            fields.push((name.clone(), ty.to_string()));
                        }
                    }
                }
            }
        }
        fields.sort();
        Ok(fields)
    }
}

fn size_text(bytes: Option<u64>) -> Option<String> {
    let b = bytes? as f64;
    Some(if b < 1024.0 { format!("{b:.0} B") } else if b < 1024.0 * 1024.0 { format!("{:.1} KB", b / 1024.0) } else if b < 1024.0f64.powi(3) { format!("{:.1} MB", b / 1024.0 / 1024.0) } else { format!("{:.1} GB", b / 1024.0f64.powi(3)) })
}

pub fn parse_value(text: &str) -> Value {
    serde_json::from_str(text.trim()).unwrap_or_else(|_| Value::String(text.to_string()))
}

#[async_trait::async_trait]
impl Backend for ElasticConn {
    async fn server_version(&self) -> DbResult<String> {
        let info = self.get("/").await?;
        let version = info.pointer("/version/number").and_then(Value::as_str).unwrap_or("");
        let product = if info.pointer("/version/distribution").and_then(Value::as_str) == Some("opensearch") { "OpenSearch" } else { "Elasticsearch" };
        Ok(format!("{product} {version}"))
    }

    async fn query(&self, qid: Uuid, text: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let mut req = parse_request(text).map_err(DbError::Invalid)?;
        let whole = req.path.clone();
        let (path, query) = split_path(&whole);
        let (path, mut query) = (path.to_string(), query.to_string());
        // _cat answers in text unless asked for JSON.
        if path.starts_with("/_cat") && !query.contains("format=") {
            query = if query.is_empty() { "format=json".into() } else { format!("{query}&format=json") };
        }
        // A search is asked for one more row than the limit and a real total, so truncation can be told.
        if path.ends_with("/_search") && req.method != "HEAD" {
            let mut body: Value = match &req.body {
                Some(b) => serde_json::from_str(b).map_err(|e| DbError::Invalid(format!("the body isn't valid JSON: {e}")))?,
                None => json!({}),
            };
            if let Value::Object(m) = &mut body {
                let asked = m.get("size").and_then(Value::as_u64);
                if asked.is_none_or(|s| s as usize > limit) {
                    m.insert("size".into(), json!(limit + 1));
                }
                m.entry("track_total_hits").or_insert(json!(true));
            }
            req.body = Some(body.to_string());
            if req.method == "GET" {
                req.method = "POST".into();
            }
        }
        let full = if query.is_empty() { path.clone() } else { format!("{path}?{query}") };
        let reply = self.call(Some(qid), &req.method, &full, req.body).await?;
        let mut out = lay_out(&reply, limit);
        out.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(out)
    }

    async fn cancel(&self, qid: Uuid) -> DbResult<()> {
        self.http.cancel(qid);
        Ok(())
    }

    async fn children(&self, path: &[String]) -> DbResult<Vec<TreeNode>> {
        match path {
            [] => Ok(vec![TreeNode { name: self.cluster.clone(), kind: NodeKind::Database, detail: None, expandable: true }]),
            [_] => {
                let list = self.get("/_cat/indices?format=json&bytes=b&h=index,docs.count,store.size,health,status").await?;
                let mut nodes: Vec<(bool, TreeNode)> = list
                    .as_array()
                    .map(|a| a.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|i| {
                        let name = i.get("index")?.as_str()?.to_string();
                        let docs = i.get("docs.count").and_then(Value::as_str).and_then(|d| d.parse::<u64>().ok());
                        let size = i.get("store.size").and_then(Value::as_str).and_then(|d| d.parse::<u64>().ok());
                        let health = i.get("health").and_then(Value::as_str).unwrap_or("");
                        let status = i.get("status").and_then(Value::as_str).unwrap_or("");
                        let parts = [docs.map(|d| format!("{d} docs")), size_text(size), Some(health.to_string()).filter(|h| !h.is_empty()), Some(status.to_string()).filter(|s| s == "close")];
                        let detail = parts.into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        Some((name.starts_with('.'), TreeNode { name, kind: NodeKind::Table, detail: Some(detail).filter(|d| !d.is_empty()), expandable: true }))
                    })
                    .collect();
                // Your indices first, the system ones (dot-named) last.
                nodes.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));
                Ok(nodes.into_iter().map(|(_, n)| n).collect())
            }
            [_, index] => Ok(self.mapping_fields(index).await?.into_iter().map(|(name, ty)| TreeNode { name, kind: NodeKind::Column, detail: Some(ty).filter(|t| !t.is_empty()), expandable: false }).collect()),
            _ => Ok(Vec::new()),
        }
    }

    async fn table_info(&self, _cluster: &str, index: &str) -> DbResult<TableInfo> {
        let mut columns = vec![ColumnInfo { name: "_id".into(), data_type: "keyword".into(), nullable: false, primary_key: true, default: None }];
        columns.extend(self.mapping_fields(index).await?.into_iter().map(|(name, data_type)| ColumnInfo { name, data_type, nullable: true, primary_key: false, default: None }));
        Ok(TableInfo { columns, primary_key: vec!["_id".into()] })
    }

    fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        let (path, body) = update_request(edit)?;
        Ok(format!("POST {path}\n{}", serde_json::to_string_pretty(&body).unwrap_or_default()))
    }

    async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        let (path, body) = update_request(edit)?;
        let r = self.http.send(None, "POST", &path, Some(body.to_string().into_bytes()), TIMEOUT).await?;
        match r.status {
            404 => Ok(0),
            s if s >= 400 => Err(error_text(s, &r.body)),
            _ => Ok(1),
        }
    }

    async fn close(&self) {}

    fn is_sql(&self) -> bool {
        false
    }

    fn destructive_reason(&self, text: &str) -> Option<String> {
        destructive(&parse_request(text).ok()?)
    }
}

/// The partial update an edit sends: the index and `_id` in the path, the changed fields as `doc`.
pub fn update_request(edit: &RowEdit) -> DbResult<(String, Value)> {
    let id = edit.key.first().filter(|k| k.column == "_id").and_then(|k| k.value.clone()).ok_or_else(|| DbError::Invalid("a document is found by its _id".into()))?;
    let mut doc = Map::new();
    for c in &edit.changes {
        if c.column.starts_with('_') {
            return Err(DbError::Invalid(format!("\"{}\" is a metadata field and can't be changed here", c.column)));
        }
        if c.column.is_empty() || c.column.contains(['\0', '\n']) {
            return Err(DbError::Invalid("that isn't a field name".into()));
        }
        doc.insert(c.column.clone(), c.value.as_deref().map_or(Value::Null, parse_value));
    }
    Ok((format!("/{}/_update/{}?refresh=wait_for", encode_segment(&edit.table), encode_segment(&id)), json!({ "doc": doc })))
}

#[cfg(test)]
mod tests;
