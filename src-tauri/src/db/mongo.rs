//! MongoDB, over the official driver.
//!
//! The console takes a database command as JSON, the way `db.runCommand` does: `{"find": "users", "filter":
//! {"age": {"$gt": 30}}}`. A key `$db` picks the database (else the connection's). Documents come back as rows,
//! one column per top-level field, with ObjectIds, dates and big numbers shown as plain values. Opening a
//! collection from the tree writes its `find` for you. Names and values in an inline edit go in as BSON values,
//! never as text that is parsed again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bson::{doc, Bson, Document};
use mongodb::options::{ClientOptions, Credential, ServerAddress, Tls, TlsOptions};
use mongodb::Client;
use serde_json::{json, Value};
use tokio::sync::Notify;
use uuid::Uuid;

use super::{
    table_from_objects, Backend, ColumnInfo, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit, TableInfo, TlsMode, TreeNode,
    CONNECT_TIMEOUT, MAX_COLUMNS,
};

/// Documents read to find a collection's fields.
const SAMPLE: i64 = 200;
const SYSTEM_DATABASES: [&str; 3] = ["admin", "config", "local"];
const DEFAULT_DATABASE: &str = "admin";

pub struct MongoConn {
    client: Client,
    database: String,
    running: Mutex<HashMap<Uuid, Arc<Notify>>>,
}

fn server_err(e: mongodb::error::Error) -> DbError {
    use mongodb::error::ErrorKind as K;
    match &*e.kind {
        K::Command(c) => DbError::Server(format!("{} ({})", c.message, c.code)),
        K::Authentication { message, .. } => DbError::Server(format!("MongoDB refused the sign-in: {message}")),
        K::ServerSelection { message, .. } => DbError::Connect { addr: "the server".into(), reason: message.clone() },
        K::Io(io) => DbError::Connect { addr: "the server".into(), reason: io.to_string() },
        _ => DbError::Server(e.to_string()),
    }
}

// -- values -------------------------------------------------------------------------------------------

/// Extended JSON's wrappers (`{"$oid": …}`, `{"$date": …}`) as the plain value they stand for, at any depth, so the
/// grid shows `65f…` and `2024-05-01T…` and not objects.
pub fn simplify(v: Value) -> Value {
    match v {
        Value::Object(map) if map.len() == 1 => {
            let (k, inner) = map.iter().next().map(|(k, v)| (k.clone(), v.clone())).expect("one entry");
            match (k.as_str(), &inner) {
                ("$oid", Value::String(s)) => Value::String(s.clone()),
                ("$date", Value::String(s)) => Value::String(s.clone()),
                ("$date", Value::Object(o)) => o
                    .get("$numberLong")
                    .and_then(Value::as_str)
                    .and_then(|ms| ms.parse::<i64>().ok())
                    .and_then(|ms| bson::DateTime::from_millis(ms).try_to_rfc3339_string().ok())
                    .map_or(Value::Object(map), Value::String),
                ("$numberLong", Value::String(s)) => s.parse::<i64>().ok().filter(|n| n.abs() <= 9_007_199_254_740_991).map_or_else(|| Value::String(s.clone()), |n| json!(n)),
                ("$numberInt", Value::String(s)) => s.parse::<i64>().map_or_else(|_| Value::String(s.clone()), |n| json!(n)),
                ("$numberDouble", Value::String(s)) => s.parse::<f64>().ok().filter(|f| f.is_finite()).map_or_else(|| Value::String(s.clone()), |f| json!(f)),
                ("$numberDecimal", Value::String(s)) => Value::String(s.clone()),
                ("$uuid", Value::String(s)) => Value::String(s.clone()),
                ("$binary", Value::Object(b)) => Value::String(format!("Binary({} bytes)", b.get("base64").and_then(Value::as_str).map_or(0, |s| s.trim_end_matches('=').len() * 3 / 4))),
                ("$symbol", Value::String(s)) => Value::String(s.clone()),
                ("$regularExpression", Value::Object(o)) => Value::String(format!("/{}/{}", o.get("pattern").and_then(Value::as_str).unwrap_or(""), o.get("options").and_then(Value::as_str).unwrap_or(""))),
                ("$timestamp", Value::Object(o)) => Value::String(format!("Timestamp({}, {})", o.get("t").and_then(Value::as_u64).unwrap_or(0), o.get("i").and_then(Value::as_u64).unwrap_or(0))),
                ("$minKey" | "$maxKey", _) => Value::String(k.trim_start_matches('$').to_string()),
                _ => Value::Object(map.into_iter().map(|(k, v)| (k, simplify(v))).collect()),
            }
        }
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, simplify(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(simplify).collect()),
        other => other,
    }
}

fn to_json(d: &Document) -> Value {
    // Canonical form keeps every integer and double exact; `simplify` then shows the ones that fit as numbers.
    simplify(Bson::Document(d.clone()).into_canonical_extjson())
}

/// Text from the console or a cell as BSON: JSON (with extended-JSON wrappers) when it parses as JSON, else a string.
pub fn parse_value(text: &str) -> Bson {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(v) => Bson::try_from(v).unwrap_or_else(|_| Bson::String(text.to_string())),
        Err(_) => Bson::String(text.to_string()),
    }
}

/// What `_id` could be for the text a grid cell shows: an ObjectId when it looks like one, the text itself, a number.
pub fn id_candidates(text: &str) -> Vec<Bson> {
    let mut out = Vec::new();
    if text.len() == 24 {
        if let Ok(oid) = bson::oid::ObjectId::parse_str(text) {
            out.push(Bson::ObjectId(oid));
        }
    }
    out.push(Bson::String(text.to_string()));
    if let Ok(n) = text.parse::<i64>() {
        out.push(Bson::Int64(n));
        if let Ok(n32) = i32::try_from(n) {
            out.push(Bson::Int32(n32));
        }
    }
    out
}

/// A statement from the console: its database, and the command.
pub fn parse_statement(text: &str, default_db: &str) -> Result<(String, Document), String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("a command is JSON, like {{\"find\": \"users\", \"filter\": {{}}}} ({e})"))?;
    let Value::Object(mut map) = v else {
        return Err("a command is one JSON object, like {\"find\": \"users\", \"filter\": {}}".into());
    };
    let db = match map.remove("$db") {
        Some(Value::String(d)) if !d.is_empty() => d,
        Some(_) => return Err("$db is the database's name, as text".into()),
        None => default_db.to_string(),
    };
    if map.is_empty() {
        return Err("the command is empty".into());
    }
    let Bson::Document(d) = Bson::try_from(Value::Object(map)).map_err(|e| format!("that isn't valid MongoDB JSON: {e}"))? else {
        return Err("a command is one JSON object".into());
    };
    Ok((db, d))
}

/// Why a command deletes or rewrites data (asked before it runs), or none.
pub fn destructive(cmd: &Document) -> Option<String> {
    let name = cmd.keys().next().map(String::as_str).unwrap_or("");
    let lower = name.to_ascii_lowercase();
    let remove_flag = |k: &str| cmd.get_bool(k).unwrap_or(false);
    let multi_update = lower == "update" && cmd.get_array("updates").is_ok_and(|u| u.iter().any(|x| x.as_document().is_some_and(|d| d.get_bool("multi").unwrap_or(false))));
    let reason = match lower.as_str() {
        "drop" => "This drops the collection and everything in it.",
        "dropdatabase" => "This drops the whole database.",
        "dropindexes" => "This drops indexes.",
        "delete" => "This deletes documents.",
        "findandmodify" if remove_flag("remove") => "This deletes a document.",
        "renamecollection" => "This renames a collection and can replace another.",
        "dropuser" | "dropallusersfromdatabase" | "droprole" | "dropallrolesfromdatabase" => "This removes users or roles.",
        "shutdown" => "This stops the server.",
        "killop" | "killcursors" | "killallsessions" => "This stops operations other people may be running.",
        "collmod" => "This changes a collection's settings.",
        "compact" | "reindex" => "This rebuilds a collection and can be slow on a big one.",
        _ if multi_update => "This updates every document that matches.",
        "aggregate" if pipeline_writes(cmd) => "This pipeline writes its results into a collection ($out or $merge).",
        _ => return None,
    };
    Some(reason.to_string())
}

fn pipeline_writes(cmd: &Document) -> bool {
    cmd.get_array("pipeline").is_ok_and(|p| p.iter().any(|s| s.as_document().is_some_and(|d| d.contains_key("$out") || d.contains_key("$merge"))))
}

// -- the connection --------------------------------------------------------------------------------------

pub fn options_for(spec: &ConnectSpec) -> Result<(String, mongodb::options::ClientOptions), DbError> {
    let mut o = ClientOptions::default();
    let (host, port) = spec.dial();
    o.hosts = vec![ServerAddress::Tcp { host, port: Some(port) }];
    o.app_name = Some("SSHVault".into());
    o.connect_timeout = Some(CONNECT_TIMEOUT);
    o.server_selection_timeout = Some(CONNECT_TIMEOUT);
    // One server, as named: a replica set's members would be found by names that a tunnel can't reach.
    o.direct_connection = Some(true);
    let database = spec.database.clone().filter(|d| !d.is_empty()).unwrap_or_default();
    if !spec.user.trim().is_empty() {
        let source = spec.option("auth_source").map(str::to_string).or_else(|| (!database.is_empty()).then(|| database.clone())).unwrap_or_else(|| "admin".into());
        o.credential = Some(Credential::builder().username(spec.user.clone()).password(spec.password.clone()).source(source).build());
    }
    o.tls = match spec.tls {
        TlsMode::Disable => None,
        TlsMode::Require => {
            let mut t = TlsOptions::default();
            t.allow_invalid_certificates = Some(true);
            Some(Tls::Enabled(t))
        }
        TlsMode::VerifyFull => {
            let mut t = TlsOptions::default();
            // Through an SSH tunnel the driver would check the certificate against 127.0.0.1, which can never match;
            // the SSH connection is what identifies the server there.
            if spec.tunnel_port.is_some() {
                t.allow_invalid_certificates = Some(true);
            }
            t.ca_file_path = spec.option("ca_file").map(std::path::PathBuf::from);
            Some(Tls::Enabled(t))
        }
    };
    Ok((database, o))
}

/// The user and password from the form, put into a connection string that lacks them, so a password never has to be
/// written inside the string (where the window would show it).
pub fn apply_login(o: &mut ClientOptions, spec: &ConnectSpec) {
    let user = (!spec.user.trim().is_empty()).then(|| spec.user.clone());
    let password = spec.password.clone().filter(|p| !p.is_empty());
    if user.is_none() && password.is_none() {
        return;
    }
    let fallback = spec.option("auth_source").map(str::to_string).or_else(|| o.default_database.clone()).unwrap_or_else(|| "admin".into());
    let cred = o.credential.get_or_insert_with(Credential::default);
    if cred.source.is_none() {
        cred.source = Some(fallback);
    }
    if let Some(u) = user {
        cred.username = Some(u);
    }
    if let Some(p) = password {
        cred.password = Some(p);
    }
}

impl MongoConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        let (database, options) = match spec.option("uri") {
            // A connection string, for Atlas (mongodb+srv://…) and replica sets; used as it is.
            Some(uri) => {
                let mut o = ClientOptions::parse(uri).await.map_err(|e| DbError::Invalid(format!("that isn't a MongoDB connection string: {e}")))?;
                apply_login(&mut o, spec);
                (o.default_database.clone().unwrap_or_default(), o)
            }
            None => options_for(spec)?,
        };
        let client = Client::with_options(options).map_err(server_err)?;
        let me = Self { client, database, running: Mutex::default() };
        // Fail now, with the real reason, rather than on the first command.
        me.run("admin", doc! { "ping": 1 }, None).await?;
        Ok(me)
    }

    fn default_db(&self) -> &str {
        if self.database.is_empty() {
            DEFAULT_DATABASE
        } else {
            &self.database
        }
    }

    /// One command, cancellable.
    async fn run(&self, db: &str, cmd: Document, qid: Option<Uuid>) -> DbResult<Document> {
        let notify = qid.map(|q| {
            let n = Arc::new(Notify::new());
            self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(q, Arc::clone(&n));
            n
        });
        let database = self.client.database(db);
        let work = database.run_command(cmd);
        let out = match &notify {
            Some(n) => tokio::select! {
                r = work => r.map_err(server_err),
                _ = n.notified() => Err(DbError::Cancelled),
            },
            None => work.await.map_err(server_err),
        };
        if let Some(q) = qid {
            self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&q);
        }
        out
    }

    /// Documents of a cursor reply, following it with `getMore` until `limit`, then closing it.
    async fn read_cursor(&self, db: &str, first: &Document, limit: usize, qid: Option<Uuid>) -> DbResult<(Vec<Document>, bool)> {
        let cursor = first.get_document("cursor").map_err(|_| DbError::Server("the server's answer has no cursor".into()))?;
        let mut docs: Vec<Document> = cursor.get_array("firstBatch").map(|a| a.iter().filter_map(|b| b.as_document().cloned()).collect()).unwrap_or_default();
        let mut id = cursor.get_i64("id").unwrap_or(0);
        let ns = cursor.get_str("ns").unwrap_or("").to_string();
        let coll = ns.split_once('.').map(|(_, c)| c.to_string()).unwrap_or_default();
        while id != 0 && docs.len() <= limit && !coll.is_empty() {
            let more = self.run(db, doc! { "getMore": id, "collection": &coll, "batchSize": 1000 }, qid).await?;
            let c = more.get_document("cursor").map_err(|_| DbError::Server("the server's getMore has no cursor".into()))?;
            docs.extend(c.get_array("nextBatch").map(|a| a.iter().filter_map(|b| b.as_document().cloned()).collect::<Vec<_>>()).unwrap_or_default());
            id = c.get_i64("id").unwrap_or(0);
        }
        if id != 0 && !coll.is_empty() {
            let _ = self.run(db, doc! { "killCursors": &coll, "cursors": [id] }, None).await;
        }
        let truncated = docs.len() > limit;
        docs.truncate(limit);
        Ok((docs, truncated))
    }
}

fn result_from_docs(docs: &[Document]) -> QueryResult {
    let objs: Vec<serde_json::Map<String, Value>> = docs.iter().filter_map(|d| if let Value::Object(m) = to_json(d) { Some(m) } else { None }).collect();
    let (columns, rows) = table_from_objects(&objs, &["_id"]);
    QueryResult { columns, rows, ..Default::default() }
}

#[async_trait::async_trait]
impl Backend for MongoConn {
    async fn server_version(&self) -> DbResult<String> {
        let info = self.run("admin", doc! { "buildInfo": 1 }, None).await?;
        Ok(format!("MongoDB {}", info.get_str("version").unwrap_or("")))
    }

    async fn query(&self, qid: Uuid, text: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let (db, mut cmd) = parse_statement(text, self.default_db()).map_err(DbError::Invalid)?;
        let name = cmd.keys().next().cloned().unwrap_or_default();
        // Ask for one more than the limit, so there is a way to know there was more.
        if name == "find" {
            let asked = cmd.get("limit").and_then(|l| l.as_i64().or_else(|| l.as_i32().map(i64::from))).filter(|l| *l > 0);
            if asked.is_none_or(|l| l as usize > limit) {
                cmd.insert("limit", (limit as i64) + 1);
            }
            cmd.entry("batchSize".to_string()).or_insert(Bson::Int32(1000));
        } else if name == "aggregate" && !cmd.contains_key("cursor") {
            cmd.insert("cursor", doc! { "batchSize": 1000 });
        }
        let reply = self.run(&db, cmd, Some(qid)).await?;
        let mut out = if reply.get_document("cursor").is_ok() {
            let (docs, truncated) = self.read_cursor(&db, &reply, limit, Some(qid)).await?;
            QueryResult { truncated, ..result_from_docs(&docs) }
        } else {
            // A command with no cursor: its reply is the answer (a count, a write result, a status).
            let mut r = result_from_docs(std::slice::from_ref(&reply));
            if matches!(name.as_str(), "insert" | "update" | "delete" | "findAndModify") {
                r.affected_rows = reply.get("n").and_then(|n| n.as_i64().or_else(|| n.as_i32().map(i64::from))).map(|n| n.max(0) as u64);
            }
            r
        };
        out.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(out)
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
                let reply = self.run("admin", doc! { "listDatabases": 1, "nameOnly": true }, None).await?;
                let mut names: Vec<String> = reply.get_array("databases").map(|a| a.iter().filter_map(|d| d.as_document()?.get_str("name").ok().map(str::to_string)).collect()).unwrap_or_default();
                // Your databases first, the server's own last.
                names.sort_by_key(|n| (SYSTEM_DATABASES.contains(&n.as_str()), n.clone()));
                Ok(names.into_iter().map(|name| TreeNode { name, kind: NodeKind::Database, detail: None, expandable: true }).collect())
            }
            [db] => {
                let reply = self.run(db, doc! { "listCollections": 1 }, None).await?;
                let first = reply.get_document("cursor").map(|c| c.get_array("firstBatch").map(|a| a.to_vec()).unwrap_or_default()).unwrap_or_default();
                let mut nodes: Vec<TreeNode> = first
                    .iter()
                    .filter_map(|c| {
                        let c = c.as_document()?;
                        let name = c.get_str("name").ok()?.to_string();
                        let ty = c.get_str("type").unwrap_or("collection");
                        Some(TreeNode { name, kind: if ty == "view" { NodeKind::View } else { NodeKind::Table }, detail: (ty == "timeseries").then(|| "time series".to_string()), expandable: true })
                    })
                    .filter(|n| !n.name.starts_with("system."))
                    .collect();
                nodes.sort_by(|a, b| a.name.cmp(&b.name));
                Ok(nodes)
            }
            [db, coll] => {
                let reply = match self.run(db, doc! { "listIndexes": coll }, None).await {
                    Ok(r) => r,
                    // A view has no indexes.
                    Err(DbError::Server(_)) => return Ok(Vec::new()),
                    Err(e) => return Err(e),
                };
                let first = reply.get_document("cursor").map(|c| c.get_array("firstBatch").map(|a| a.to_vec()).unwrap_or_default()).unwrap_or_default();
                Ok(first
                    .iter()
                    .filter_map(|i| {
                        let i = i.as_document()?;
                        let name = i.get_str("name").ok()?.to_string();
                        let keys = i.get_document("key").map(|k| k.iter().map(|(f, d)| format!("{f}: {d}")).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                        let unique = i.get_bool("unique").unwrap_or(false);
                        Some(TreeNode { name, kind: NodeKind::Index, detail: Some(if unique { format!("{keys} · unique") } else { keys }), expandable: false })
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn table_info(&self, db: &str, coll: &str) -> DbResult<TableInfo> {
        let reply = self.run(db, doc! { "find": coll, "limit": SAMPLE, "batchSize": SAMPLE as i32 }, None).await?;
        let docs: Vec<Document> = reply.get_document("cursor").and_then(|c| c.get_array("firstBatch")).map(|a| a.iter().filter_map(|d| d.as_document().cloned()).collect()).unwrap_or_default();
        let sample = result_from_docs(&docs);
        let mut columns: Vec<ColumnInfo> = sample.columns.iter().take(MAX_COLUMNS).map(|c| ColumnInfo { name: c.name.clone(), data_type: c.data_type.clone(), nullable: c.name != "_id", primary_key: c.name == "_id", default: None }).collect();
        if !columns.iter().any(|c| c.name == "_id") {
            columns.insert(0, ColumnInfo { name: "_id".into(), data_type: "objectid".into(), nullable: false, primary_key: true, default: None });
        }
        Ok(TableInfo { columns, primary_key: vec!["_id".into()] })
    }

    fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        let (filter, set) = update_parts(edit)?;
        Ok(format!("db.getSiblingDB({}).{}.updateOne({}, {{\"$set\": {}}})", serde_json::to_string(&edit.database).unwrap_or_default(), edit.table, to_json(&filter), to_json(&set)))
    }

    async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        let (filter, set) = update_parts(edit)?;
        let reply = self
            .run(&edit.database, doc! { "update": &edit.table, "updates": [ { "q": filter, "u": { "$set": set }, "multi": false } ] }, None)
            .await?;
        if let Ok(errs) = reply.get_array("writeErrors") {
            if let Some(e) = errs.first().and_then(|e| e.as_document()).and_then(|e| e.get_str("errmsg").ok()) {
                return Err(DbError::Server(e.to_string()));
            }
        }
        Ok(reply.get("n").and_then(|n| n.as_i64().or_else(|| n.as_i32().map(i64::from))).unwrap_or(0).max(0) as u64)
    }

    async fn close(&self) {}

    fn is_sql(&self) -> bool {
        false
    }

    fn destructive_reason(&self, text: &str) -> Option<String> {
        let (_, cmd) = parse_statement(text, DEFAULT_DATABASE).ok()?;
        destructive(&cmd)
    }
}

/// The `_id` filter and the `$set` document of an edit.
pub fn update_parts(edit: &RowEdit) -> DbResult<(Document, Document)> {
    let key = edit.key.first().filter(|k| k.column == "_id").and_then(|k| k.value.clone()).ok_or_else(|| DbError::Invalid("a document is found by its _id".into()))?;
    let mut set = Document::new();
    for c in &edit.changes {
        if c.column == "_id" {
            return Err(DbError::Invalid("a document's _id can't be changed".into()));
        }
        if c.column.is_empty() || c.column.starts_with('$') || c.column.contains('\0') {
            return Err(DbError::Invalid(format!("\"{}\" isn't a field name that can be set here", c.column)));
        }
        set.insert(c.column.clone(), c.value.as_deref().map_or(Bson::Null, parse_value));
    }
    let filter = doc! { "_id": { "$in": id_candidates(&key) } };
    Ok((filter, set))
}

#[cfg(test)]
mod tests;
