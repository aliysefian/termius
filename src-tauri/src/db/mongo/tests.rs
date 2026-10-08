use super::*;
use bson::oid::ObjectId;
use std::collections::BTreeMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn spec(port: u16) -> ConnectSpec {
    ConnectSpec { engine: "mongodb".into(), host: "127.0.0.1".into(), port, user: String::new(), password: None, database: Some("shop".into()), tls: TlsMode::Disable, tunnel_port: None, options: BTreeMap::new() }
}

// -- values and statements -----------------------------------------------------------------------------

#[test]
fn extended_json_wrappers_become_plain_values() {
    let oid = ObjectId::parse_str("65f1a2b3c4d5e6f708192a3b").unwrap();
    let d = doc! {
        "_id": oid,
        "n": 5_i32,
        "big": 9_007_199_254_740_993_i64,
        "small": 42_i64,
        "when": bson::DateTime::from_millis(1_714_521_600_000),
        "dec": bson::Decimal128::from_bytes([0u8; 16]),
        "bin": Bson::Binary(bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: vec![1, 2, 3] }),
        "nested": { "ref": oid, "list": [1_i64, { "$x": 1 }] },
        "re": Bson::RegularExpression(bson::Regex { pattern: "^a".into(), options: "i".into() }),
    };
    let v = to_json(&d);
    assert_eq!(v["_id"], "65f1a2b3c4d5e6f708192a3b");
    assert_eq!(v["n"], 5);
    assert_eq!(v["big"], "9007199254740993", "beyond 2^53 stays exact text");
    assert_eq!(v["small"], 42);
    assert!(v["when"].as_str().unwrap().starts_with("2024-05-01"), "{}", v["when"]);
    assert_eq!(v["bin"], "Binary(3 bytes)");
    assert_eq!(v["nested"]["ref"], "65f1a2b3c4d5e6f708192a3b");
    assert_eq!(v["nested"]["list"][0], 42 - 41);
    assert_eq!(v["re"], "/^a/i");
}

#[test]
fn typed_text_becomes_the_value_it_looks_like() {
    assert_eq!(parse_value("42"), Bson::Int32(42));
    assert_eq!(parse_value("4.5"), Bson::Double(4.5));
    assert_eq!(parse_value("true"), Bson::Boolean(true));
    assert_eq!(parse_value("null"), Bson::Null);
    assert_eq!(parse_value("plain words"), Bson::String("plain words".into()));
    assert_eq!(parse_value("\"quoted\""), Bson::String("quoted".into()));
    assert_eq!(parse_value("{\"a\": [1, 2]}"), Bson::Document(doc! { "a": [1, 2] }));
    assert_eq!(parse_value("{\"$oid\": \"65f1a2b3c4d5e6f708192a3b\"}"), Bson::ObjectId(ObjectId::parse_str("65f1a2b3c4d5e6f708192a3b").unwrap()));
    assert_eq!(parse_value("{ not json"), Bson::String("{ not json".into()));
}

#[test]
fn an_id_is_tried_as_each_thing_it_could_be() {
    let c = id_candidates("65f1a2b3c4d5e6f708192a3b");
    assert!(matches!(c[0], Bson::ObjectId(_)) && c[1] == Bson::String("65f1a2b3c4d5e6f708192a3b".into()));
    let n = id_candidates("42");
    assert_eq!(n, [Bson::String("42".into()), Bson::Int64(42), Bson::Int32(42)]);
    assert_eq!(id_candidates("abc"), [Bson::String("abc".into())]);
    assert_eq!(id_candidates("65f1a2b3c4d5e6f708192a3z").len(), 1, "not hex, not an ObjectId");
}

#[test]
fn a_statement_is_one_json_command_with_an_optional_database() {
    let (db, cmd) = parse_statement("{\"find\": \"users\", \"filter\": {\"age\": {\"$gt\": 30}}}", "shop").unwrap();
    assert_eq!(db, "shop");
    assert_eq!(cmd.keys().next().map(String::as_str), Some("find"), "the command name stays first");
    assert_eq!(parse_statement("{\"$db\": \"other\", \"ping\": 1}", "shop").unwrap().0, "other");
    for bad in ["", "not json", "[1]", "42", "{}", "{\"$db\": \"x\"}", "{\"$db\": 3, \"ping\": 1}"] {
        assert!(parse_statement(bad, "shop").is_err(), "{bad}");
    }
    // An empty $db is no database at all.
    assert!(parse_statement("{\"$db\": \"\", \"ping\": 1}", "shop").is_err());
}

fn destructive_of(json: &str) -> Option<String> {
    destructive(&parse_statement(json, "shop").unwrap().1)
}

#[test]
fn what_deletes_or_rewrites_is_named() {
    for cmd in [
        r#"{"drop": "users"}"#,
        r#"{"dropDatabase": 1}"#,
        r#"{"dropIndexes": "users", "index": "*"}"#,
        r#"{"delete": "users", "deletes": [{"q": {}, "limit": 0}]}"#,
        r#"{"findAndModify": "users", "query": {}, "remove": true}"#,
        r#"{"renameCollection": "shop.a", "to": "shop.b"}"#,
        r#"{"dropUser": "ann"}"#,
        r#"{"shutdown": 1}"#,
        r#"{"update": "users", "updates": [{"q": {}, "u": {"$set": {"a": 1}}, "multi": true}]}"#,
        r#"{"aggregate": "users", "pipeline": [{"$match": {}}, {"$out": "copy"}], "cursor": {}}"#,
        r#"{"aggregate": "users", "pipeline": [{"$merge": {"into": "x"}}], "cursor": {}}"#,
        r#"{"collMod": "users", "validator": {}}"#,
    ] {
        assert!(destructive_of(cmd).is_some(), "{cmd}");
    }
    for cmd in [
        r#"{"find": "users", "filter": {}}"#,
        r#"{"aggregate": "users", "pipeline": [{"$group": {"_id": "$a"}}], "cursor": {}}"#,
        r#"{"insert": "users", "documents": [{"a": 1}]}"#,
        r#"{"update": "users", "updates": [{"q": {"_id": 1}, "u": {"$set": {"a": 1}}}]}"#,
        r#"{"findAndModify": "users", "query": {}, "update": {"$set": {"a": 1}}}"#,
        r#"{"count": "users"}"#,
        r#"{"listCollections": 1}"#,
        r#"{"createIndexes": "users", "indexes": []}"#,
    ] {
        assert!(destructive_of(cmd).is_none(), "{cmd}");
    }
}

fn edit(key: &str, changes: &[(&str, Option<&str>)]) -> RowEdit {
    use super::super::CellEdit;
    RowEdit { database: "shop".into(), table: "users".into(), key: vec![CellEdit { column: "_id".into(), value: Some(key.into()) }], changes: changes.iter().map(|(c, v)| CellEdit { column: c.to_string(), value: v.map(str::to_string) }).collect() }
}

#[test]
fn an_edit_sets_typed_values_by_id_and_refuses_what_it_can_not_do() {
    let (filter, set) = update_parts(&edit("65f1a2b3c4d5e6f708192a3b", &[("age", Some("31")), ("name", Some("Ada")), ("tags", Some("[\"a\", \"b\"]")), ("gone", None)])).unwrap();
    assert_eq!(set.get("age"), Some(&Bson::Int32(31)));
    assert_eq!(set.get("name"), Some(&Bson::String("Ada".into())));
    assert_eq!(set.get("tags"), Some(&Bson::Array(vec!["a".into(), "b".into()])));
    assert_eq!(set.get("gone"), Some(&Bson::Null));
    assert_eq!(filter.get_document("_id").unwrap().get_array("$in").unwrap().len(), 2);
    for bad in [edit("1", &[("_id", Some("2"))]), edit("1", &[("$set", Some("x"))]), edit("1", &[("", Some("x"))])] {
        assert!(matches!(update_parts(&bad), Err(DbError::Invalid(_))));
    }
    let mut no_key = edit("1", &[("a", Some("b"))]);
    no_key.key[0].column = "name".into();
    assert!(matches!(update_parts(&no_key), Err(DbError::Invalid(_))));
    no_key.key.clear();
    assert!(matches!(update_parts(&no_key), Err(DbError::Invalid(_))));
}

#[test]
fn the_connection_options_follow_the_form() {
    let mut s = spec(27017);
    s.user = "ann".into();
    s.password = Some("pw".into());
    s.options.insert("auth_source".into(), "users".into());
    let (db, o) = options_for(&s).unwrap();
    assert_eq!(db, "shop");
    assert_eq!(o.credential.as_ref().unwrap().source.as_deref(), Some("users"));
    assert_eq!(o.direct_connection, Some(true));
    assert!(o.tls.is_none());
    // The authentication database defaults to the connection's database, then admin.
    s.options.clear();
    assert_eq!(options_for(&s).unwrap().1.credential.unwrap().source.as_deref(), Some("shop"));
    s.database = None;
    assert_eq!(options_for(&s).unwrap().1.credential.unwrap().source.as_deref(), Some("admin"));
    // No user: no credential at all.
    assert!(options_for(&spec(1)).unwrap().1.credential.is_none());
    s.tls = TlsMode::Require;
    assert!(matches!(options_for(&s).unwrap().1.tls, Some(Tls::Enabled(t)) if t.allow_invalid_certificates == Some(true)));
    s.tls = TlsMode::VerifyFull;
    assert!(matches!(options_for(&s).unwrap().1.tls, Some(Tls::Enabled(t)) if t.allow_invalid_certificates.is_none()));
    s.tunnel_port = Some(5555);
    let (_, o) = options_for(&s).unwrap();
    assert!(matches!(&o.hosts[0], ServerAddress::Tcp { host, port: Some(5555) } if host == "127.0.0.1"));
}

#[tokio::test]
async fn the_form_s_user_and_password_join_a_connection_string_that_lacks_them() {
    let mut o = ClientOptions::parse("mongodb://db.example/shop?authSource=admin").await.unwrap();
    let mut s = spec(1);
    s.user = "ann".into();
    s.password = Some("p w".into());
    apply_login(&mut o, &s);
    let c = o.credential.unwrap();
    assert_eq!((c.username.as_deref(), c.password.as_deref(), c.source.as_deref()), (Some("ann"), Some("p w"), Some("shop")), "the authentication database is the string's database, as MongoDB does it");
    // A string that already has its login, and a form with none, are left alone.
    let mut o = ClientOptions::parse("mongodb://bob:secret@db.example/shop").await.unwrap();
    apply_login(&mut o, &spec(1));
    assert_eq!(o.credential.unwrap().password.as_deref(), Some("secret"));
}

// -- through the real driver, against a stand-in server ------------------------------------------------------------

/// The slice of MongoDB's wire protocol the driver needs for these commands: OP_QUERY for the first `isMaster`,
/// OP_MSG after, answered from a script.
struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<Document>>>,
}

fn reply_doc(cmd: &Document, docs: &Arc<Mutex<Vec<Document>>>) -> Document {
    docs.lock().unwrap().push(cmd.clone());
    let name = cmd.keys().next().cloned().unwrap_or_default();
    let ok = |mut d: Document| {
        d.insert("ok", 1.0);
        d
    };
    let id1 = ObjectId::parse_str("65f1a2b3c4d5e6f708192a3b").unwrap();
    match name.as_str() {
        "isMaster" | "ismaster" | "hello" => ok(doc! { "ismaster": true, "isWritablePrimary": true, "maxBsonObjectSize": 16_777_216, "maxMessageSizeBytes": 48_000_000, "maxWriteBatchSize": 100_000, "minWireVersion": 0, "maxWireVersion": 17, "logicalSessionTimeoutMinutes": 30 }),
        "ping" => ok(doc! {}),
        "buildInfo" => ok(doc! { "version": "7.0.5" }),
        "listDatabases" => ok(doc! { "databases": [{ "name": "local" }, { "name": "shop" }, { "name": "admin" }, { "name": "analytics" }] }),
        "listCollections" => ok(doc! { "cursor": { "id": 0_i64, "ns": "shop.$cmd.listCollections", "firstBatch": [{ "name": "users", "type": "collection" }, { "name": "system.views", "type": "collection" }, { "name": "recent", "type": "view" }] } }),
        "listIndexes" => ok(doc! { "cursor": { "id": 0_i64, "ns": "shop.users", "firstBatch": [{ "name": "_id_", "key": { "_id": 1 } }, { "name": "email_1", "key": { "email": 1 }, "unique": true }] } }),
        "find" => {
            let limit = cmd.get_i64("limit").unwrap_or(1000);
            let coll = cmd.get_str("find").unwrap_or("users").to_string();
            if coll == "empty" {
                return ok(doc! { "cursor": { "id": 0_i64, "ns": "shop.empty", "firstBatch": [] } });
            }
            let all = vec![
                doc! { "_id": id1, "name": "Ada", "age": 36_i32, "tags": ["a", "b"], "address": { "city": "London" } },
                doc! { "_id": ObjectId::parse_str("65f1a2b3c4d5e6f708192a3c").unwrap(), "name": "Alan", "age": 41_i32, "vip": true },
                doc! { "_id": ObjectId::parse_str("65f1a2b3c4d5e6f708192a3d").unwrap(), "name": "Grace" },
            ];
            let n = (limit as usize).min(all.len());
            let first: Vec<Bson> = all.into_iter().take(n).map(Bson::Document).collect();
            // The answer comes in two parts when the driver asked for more than one batch of two.
            let (now, later) = if coll == "paged" && first.len() > 2 { (first[..2].to_vec(), first[2..].to_vec()) } else { (first, vec![]) };
            let id = if later.is_empty() { 0_i64 } else { 77_i64 };
            ok(doc! { "cursor": { "id": id, "ns": format!("shop.{coll}"), "firstBatch": now } })
        }
        "getMore" => ok(doc! { "cursor": { "id": 0_i64, "ns": "shop.paged", "nextBatch": [{ "_id": ObjectId::parse_str("65f1a2b3c4d5e6f708192a3d").unwrap(), "name": "Grace" }] } }),
        "killCursors" => ok(doc! { "cursorsKilled": [1_i64] }),
        "count" => ok(doc! { "n": 3 }),
        "update" => ok(doc! { "n": 1, "nModified": 1 }),
        "insert" => ok(doc! { "n": 2 }),
        "boom" => doc! { "ok": 0.0, "errmsg": "no such command: 'boom'", "code": 59, "codeName": "CommandNotFound" },
        _ => ok(doc! {}),
    }
}

async fn fake() -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<Document>>> = Arc::default();
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                loop {
                    let mut head = [0u8; 16];
                    if s.read_exact(&mut head).await.is_err() {
                        return;
                    }
                    let len = i32::from_le_bytes(head[0..4].try_into().unwrap()) as usize;
                    let req_id = i32::from_le_bytes(head[4..8].try_into().unwrap());
                    let opcode = i32::from_le_bytes(head[12..16].try_into().unwrap());
                    let mut body = vec![0u8; len - 16];
                    if s.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    let mut out: Vec<u8> = Vec::new();
                    if opcode == 2004 {
                        // OP_QUERY: flags, "db.$cmd\0", skip, return, then the command.
                        let end = body[4..].iter().position(|b| *b == 0).unwrap() + 4;
                        let cmd = Document::from_reader(&body[end + 1 + 8..]).unwrap();
                        let reply = reply_doc(&cmd, &log);
                        let bytes = bson::to_vec(&reply).unwrap();
                        out.extend_from_slice(&0i32.to_le_bytes());
                        out.extend_from_slice(&0i64.to_le_bytes());
                        out.extend_from_slice(&0i32.to_le_bytes());
                        out.extend_from_slice(&1i32.to_le_bytes());
                        out.extend_from_slice(&bytes);
                        let total = (16 + out.len()) as i32;
                        let mut msg = Vec::new();
                        for n in [total, 1, req_id, 1] {
                            msg.extend_from_slice(&n.to_le_bytes());
                        }
                        msg.extend_from_slice(&out);
                        let _ = s.write_all(&msg).await;
                    } else {
                        // OP_MSG: flags, then sections; kind 0 is the command document.
                        let flags = u32::from_le_bytes(body[0..4].try_into().unwrap());
                        assert_eq!(body[4], 0, "a plain command section");
                        let cmd = Document::from_reader(&body[5..]).unwrap();
                        let reply = reply_doc(&cmd, &log);
                        let bytes = bson::to_vec(&reply).unwrap();
                        let _ = flags;
                        let total = (16 + 4 + 1 + bytes.len()) as i32;
                        let mut msg = Vec::new();
                        for n in [total, 1, req_id, 2013] {
                            msg.extend_from_slice(&n.to_le_bytes());
                        }
                        msg.extend_from_slice(&0u32.to_le_bytes());
                        msg.push(0);
                        msg.extend_from_slice(&bytes);
                        let _ = s.write_all(&msg).await;
                    }
                }
            });
        }
    });
    Fake { port, seen }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn through_the_driver_documents_become_rows_with_the_id_first() {
    let f = fake().await;
    let c = MongoConn::connect(&spec(f.port)).await.unwrap();
    assert_eq!(c.server_version().await.unwrap(), "MongoDB 7.0.5");
    let r = c.query(Uuid::new_v4(), "{\"find\": \"users\", \"filter\": {}}", 100).await.unwrap();
    assert_eq!(r.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["_id", "name", "age", "tags", "address", "vip"]);
    assert_eq!(r.rows.len(), 3);
    assert_eq!(r.rows[0][0], json!("65f1a2b3c4d5e6f708192a3b"));
    assert_eq!(r.rows[0][3], json!("[\"a\",\"b\"]"), "an array is its JSON text");
    assert_eq!(r.rows[0][4], json!("{\"city\":\"London\"}"));
    assert_eq!(r.rows[2][2], Value::Null, "a document that lacks the field has an empty cell");
    assert_eq!(r.columns[2].kind, crate::db::ColumnKind::Number);
    assert_eq!(r.columns[3].kind, crate::db::ColumnKind::Json);
    assert!(!r.truncated);
    // The limit is asked for with one to spare, and said when it cuts.
    let cut = c.query(Uuid::new_v4(), "{\"find\": \"users\"}", 2).await.unwrap();
    assert_eq!((cut.rows.len(), cut.truncated), (2, true));
    let sent = f.seen.lock().unwrap().iter().rev().find(|d| d.contains_key("find")).cloned().unwrap();
    assert_eq!(sent.get_i64("limit").unwrap(), 3);
    let empty = c.query(Uuid::new_v4(), "{\"find\": \"empty\"}", 10).await.unwrap();
    assert!(empty.rows.is_empty() && empty.columns.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn through_the_driver_a_cursor_is_followed_and_commands_without_one_answer_in_one_row() {
    let f = fake().await;
    let c = MongoConn::connect(&spec(f.port)).await.unwrap();
    let paged = c.query(Uuid::new_v4(), "{\"find\": \"paged\"}", 100).await.unwrap();
    assert_eq!(paged.rows.len(), 3, "the getMore's document is there");
    assert!(f.seen.lock().unwrap().iter().any(|d| d.contains_key("getMore")));
    let count = c.query(Uuid::new_v4(), "{\"count\": \"users\"}", 10).await.unwrap();
    assert_eq!(count.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["n", "ok"]);
    assert_eq!(count.rows[0][0], json!(3));
    let write = c.query(Uuid::new_v4(), "{\"insert\": \"users\", \"documents\": [{\"a\": 1}, {\"a\": 2}]}", 10).await.unwrap();
    assert_eq!(write.affected_rows, Some(2));
    // A command that needs a cursor to be asked for gets one asked for.
    c.query(Uuid::new_v4(), "{\"aggregate\": \"users\", \"pipeline\": []}", 10).await.unwrap();
    let agg = f.seen.lock().unwrap().iter().rev().find(|d| d.contains_key("aggregate")).cloned().unwrap();
    assert!(agg.get_document("cursor").is_ok());
    // The database comes from the connection, or from $db in the command.
    c.query(Uuid::new_v4(), "{\"$db\": \"analytics\", \"count\": \"events\"}", 10).await.unwrap();
    // The server's own error is shown as it worded it.
    match c.query(Uuid::new_v4(), "{\"boom\": 1}", 10).await {
        Err(DbError::Server(m)) => assert!(m.contains("no such command") && m.contains("59"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(c.query(Uuid::new_v4(), "not json", 10).await, Err(DbError::Invalid(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn through_the_driver_the_tree_lists_databases_collections_and_indexes() {
    let f = fake().await;
    let c = MongoConn::connect(&spec(f.port)).await.unwrap();
    let dbs: Vec<String> = c.children(&[]).await.unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(dbs, ["analytics", "shop", "admin", "local"], "yours first, the server's own last");
    let colls = c.children(&["shop".into()]).await.unwrap();
    assert_eq!(colls.iter().map(|n| (n.name.as_str(), n.kind)).collect::<Vec<_>>(), [("recent", NodeKind::View), ("users", NodeKind::Table)], "system collections are left out");
    let idx = c.children(&["shop".into(), "users".into()]).await.unwrap();
    assert_eq!(idx[1].detail.as_deref(), Some("email: 1 · unique"));
    assert!(idx.iter().all(|n| n.kind == NodeKind::Index));
    let info = c.table_info("shop", "users").await.unwrap();
    assert_eq!(info.primary_key, ["_id"]);
    assert_eq!(info.columns[0].name, "_id");
    assert!(info.columns.iter().any(|c| c.name == "address") && info.columns[0].primary_key);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn through_the_driver_an_edit_updates_one_document_by_id() {
    let f = fake().await;
    let c = MongoConn::connect(&spec(f.port)).await.unwrap();
    let e = edit("65f1a2b3c4d5e6f708192a3b", &[("age", Some("37"))]);
    assert_eq!(c.apply_update(&e).await.unwrap(), 1);
    let sent = f.seen.lock().unwrap().iter().rev().find(|d| d.contains_key("update")).cloned().unwrap();
    let u = sent.get_array("updates").unwrap()[0].as_document().unwrap().clone();
    assert_eq!(u.get_bool("multi"), Ok(false), "one document, never many");
    assert_eq!(u.get_document("u").unwrap().get_document("$set").unwrap().get("age"), Some(&Bson::Int32(37)));
    let preview = c.preview_update(&e).unwrap();
    assert!(preview.contains("updateOne") && preview.contains("65f1a2b3c4d5e6f708192a3b") && preview.contains("\"age\":37"), "{preview}");
    assert!(c.destructive_reason("{\"drop\": \"users\"}").is_some());
    assert!(c.destructive_reason("{\"find\": \"users\"}").is_none());
    assert!(!c.is_sql());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_server_and_a_bad_connection_string_are_explained() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let started = Instant::now();
    assert!(matches!(MongoConn::connect(&spec(closed)).await, Err(DbError::Connect { .. }) | Err(DbError::Server(_))));
    assert!(started.elapsed() < std::time::Duration::from_secs(25));
    let mut s = spec(1);
    s.options.insert("uri".into(), "http://not-mongo".into());
    assert!(matches!(MongoConn::connect(&s).await, Err(DbError::Invalid(m)) if m.contains("connection string")));
}
