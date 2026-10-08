use super::*;
use crate::db::{CellEdit, TlsMode};
use std::collections::BTreeMap;
use crate::db::http::fake::{serve, Fake, Request as Seen};
use std::sync::Arc;

fn spec(f: &Fake, extra: &[(&str, &str)]) -> ConnectSpec {
    ConnectSpec {
        engine: "rqlite".into(),
        host: "127.0.0.1".into(),
        port: f.port,
        user: String::new(),
        password: None,
        database: None,
        tls: TlsMode::Disable,
        tunnel_port: None,
        options: extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

const STATUS: &str = r#"{"build":{"version":"v8.36.1"}}"#;

fn standard(r: &Seen) -> (u16, String) {
    let (path, body) = (r.path.as_str(), r.body.as_str());
    if path.starts_with("/status") {
        return (200, STATUS.into());
    }
    if path.starts_with("/db/query") {
        if body.contains("sqlite_master") {
            return (200, r#"{"results":[{"columns":["name","type"],"types":["text","text"],"values":[["items","table"],["recent","view"]]}]}"#.into());
        }
        if body.contains("table_info") {
            return (200, r#"{"results":[{"columns":["cid","name","type","notnull","dflt_value","pk"],"types":["integer","text","text","integer","text","integer"],"values":[[0,"id","INTEGER",1,null,1],[1,"name","TEXT",0,null,0],[2,"qty","INTEGER",0,"0",0]]}]}"#.into());
        }
        if body.contains("index_list") {
            return (200, r#"{"results":[{"columns":["seq","name","unique","origin","partial"],"types":["integer","text","integer","text","integer"],"values":[[0,"items_name",1,"c",0]]}]}"#.into());
        }
        if body.contains("boom") {
            return (200, r#"{"results":[{"error":"no such table: boom"}]}"#.into());
        }
        return (200, r#"{"results":[{"columns":["id","name"],"types":["integer","text"],"values":[[1,"a"],[2,"b"],[3,"c"]]}]}"#.into());
    }
    if path.starts_with("/db/execute") {
        return (200, r#"{"results":[{"last_insert_id":7,"rows_affected":1}]}"#.into());
    }
    (404, "{}".into())
}

#[test]
fn reads_and_writes_are_told_apart_by_their_first_word() {
    for read in ["SELECT 1", "  select * from t", "-- note\nSELECT 1", "/* c */ select 1", "(SELECT 1)", "EXPLAIN QUERY PLAN SELECT 1", "PRAGMA table_info(t)", "VALUES (1)", "WITH x AS (SELECT 1) SELECT * FROM x"] {
        assert!(is_read(read), "{read}");
    }
    for write in ["INSERT INTO t VALUES (1)", "update t set a=1", "DELETE FROM t", "CREATE TABLE t(a)", "DROP TABLE t", "REPLACE INTO t VALUES (1)", "WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x", "BEGIN", "", "--only a comment"] {
        assert!(!is_read(write), "{write}");
    }
}

#[test]
fn names_and_values_are_quoted_for_sqlite() {
    assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    let edit = RowEdit {
        database: "main".into(),
        table: "it'ems".into(),
        key: vec![CellEdit { column: "id".into(), value: Some("1".into()) }],
        changes: vec![CellEdit { column: "name".into(), value: Some("it's ? {x}".into()) }, CellEdit { column: "note".into(), value: None }],
    };
    assert_eq!(preview_update(&edit), "UPDATE \"main\".\"it'ems\" SET \"name\" = 'it''s ? {x}', \"note\" = NULL WHERE \"id\" = '1'");
}

#[test]
fn declared_types_become_kinds() {
    assert_eq!(kind_of("INTEGER"), ColumnKind::Number);
    assert_eq!(kind_of("varchar(10)"), ColumnKind::Text);
    assert_eq!(kind_of("BLOB"), ColumnKind::Binary);
    assert_eq!(kind_of("DATETIME"), ColumnKind::DateTime);
    assert_eq!(kind_of(""), ColumnKind::Other);
}

#[tokio::test]
async fn connects_reports_its_version_and_reads_rows_at_the_chosen_level() {
    let f = serve(standard).await;
    let c = RqliteConn::connect(&spec(&f, &[("level", "STRONG")])).await.unwrap();
    assert_eq!(c.server_version().await.unwrap(), "rqlite 8.36.1");
    let r = c.query(Uuid::new_v4(), "SELECT * FROM items", 1000).await.unwrap();
    assert_eq!(r.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "name"]);
    assert_eq!(r.columns[0].kind, ColumnKind::Number);
    assert_eq!(r.rows.len(), 3);
    assert!(!r.truncated);
    assert!(f.seen.lock().unwrap().iter().any(|r| r.method == "POST" && r.path.starts_with("/db/query?level=strong")));
}

#[tokio::test]
async fn the_row_limit_cuts_and_says_so() {
    let f = serve(standard).await;
    let c = RqliteConn::connect(&spec(&f, &[])).await.unwrap();
    let r = c.query(Uuid::new_v4(), "SELECT * FROM items", 2).await.unwrap();
    assert_eq!((r.rows.len(), r.truncated), (2, true));
}

#[tokio::test]
async fn a_write_goes_to_execute_and_reports_what_changed() {
    let f = serve(standard).await;
    let c = RqliteConn::connect(&spec(&f, &[])).await.unwrap();
    let r = c.query(Uuid::new_v4(), "INSERT INTO items(name) VALUES ('x')", 1000).await.unwrap();
    assert_eq!((r.affected_rows, r.last_insert_id), (Some(1), Some(7)));
    assert!(r.columns.is_empty());
    assert!(f.seen.lock().unwrap().iter().any(|r| r.path.starts_with("/db/execute")));
}

#[tokio::test]
async fn a_statement_error_is_the_servers_own_words() {
    let f = serve(standard).await;
    let c = RqliteConn::connect(&spec(&f, &[])).await.unwrap();
    match c.query(Uuid::new_v4(), "SELECT * FROM boom", 10).await {
        Err(DbError::Server(m)) => assert_eq!(m, "no such table: boom"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_tree_has_one_schema_then_tables_and_views_then_columns_and_indexes() {
    let f = serve(standard).await;
    let c = RqliteConn::connect(&spec(&f, &[])).await.unwrap();
    let root = c.children(&[]).await.unwrap();
    assert_eq!((root.len(), root[0].name.as_str(), root[0].kind), (1, "main", NodeKind::Schema));
    let tables = c.children(&["main".into()]).await.unwrap();
    assert_eq!(tables.iter().map(|t| (t.name.as_str(), t.kind)).collect::<Vec<_>>(), [("items", NodeKind::Table), ("recent", NodeKind::View)]);
    let inside = c.children(&["main".into(), "items".into()]).await.unwrap();
    assert_eq!(inside[0].detail.as_deref(), Some("INTEGER · PK"));
    assert_eq!(inside[1].detail.as_deref(), Some("TEXT"));
    assert!(inside.iter().any(|n| n.kind == NodeKind::Index && n.name == "items_name" && n.detail.as_deref() == Some("unique")));
    let info = c.table_info("main", "items").await.unwrap();
    assert_eq!(info.primary_key, ["id"]);
    assert!(!info.columns[0].nullable && info.columns[1].nullable);
    assert_eq!(info.columns[2].default.as_deref(), Some("0"));
}

#[tokio::test]
async fn an_edit_binds_its_values_and_a_value_that_looks_like_sql_stays_a_value() {
    let f = serve(standard).await;
    let c = RqliteConn::connect(&spec(&f, &[])).await.unwrap();
    let edit = RowEdit {
        database: "main".into(),
        table: "items".into(),
        key: vec![CellEdit { column: "id".into(), value: Some("1".into()) }],
        changes: vec![CellEdit { column: "name".into(), value: Some("'; DROP TABLE items; --".into()) }],
    };
    assert_eq!(c.apply_update(&edit).await.unwrap(), 1);
    let body = f.seen.lock().unwrap().iter().rev().find(|r| r.path.starts_with("/db/execute")).map(|r| r.body.clone()).unwrap();
    let sent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent[0][0], "UPDATE \"main\".\"items\" SET \"name\" = ? WHERE \"id\" = ?");
    assert_eq!(sent[0][1], "'; DROP TABLE items; --");
    assert_eq!(sent[0][2], "1");
    let bad = RowEdit { changes: vec![CellEdit { column: "name".into(), value: Some("a\0b".into()) }], ..edit };
    assert!(matches!(c.apply_update(&bad).await, Err(DbError::Invalid(_))));
}

#[tokio::test]
async fn a_user_name_and_password_are_sent_as_basic_auth_and_a_refusal_is_explained() {
    let f = serve(|r| if r.path.starts_with("/status") { (401, String::new()) } else { (404, "{}".into()) }).await;
    let mut s = spec(&f, &[]);
    s.user = "admin".into();
    s.password = Some("secret".into());
    match RqliteConn::connect(&s).await {
        Err(DbError::Server(m)) => assert!(m.contains("401") && m.contains("user name or password"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("should have been refused"),
    }
    let auth = f.seen.lock().unwrap().iter().find_map(|r| r.authorization.clone()).unwrap();
    // admin:secret
    assert!(auth.contains("Basic YWRtaW46c2VjcmV0"), "{auth}");
}

#[tokio::test]
async fn something_that_is_not_rqlite_is_not_trusted_and_an_unreachable_server_says_so() {
    let f = serve(|_r| (200, "<html>hello</html>".into())).await;
    match RqliteConn::connect(&spec(&f, &[])).await {
        Err(DbError::Server(m)) => assert!(m.contains("not like rqlite"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("should have been refused"),
    }
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut s = spec(&f, &[]);
    s.port = closed;
    assert!(matches!(RqliteConn::connect(&s).await, Err(DbError::Connect { .. })));
    assert!(matches!(RqliteConn::connect(&spec(&f, &[("level", "sometimes")])).await, Err(DbError::Invalid(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_request_can_be_cancelled() {
    let f = serve(|r| {
        if r.path.starts_with("/status") {
            return (200, STATUS.into());
        }
        std::thread::sleep(Duration::from_millis(1500));
        (200, r#"{"results":[{"columns":["a"],"types":["integer"],"values":[[1]]}]}"#.into())
    })
    .await;
    let c = Arc::new(RqliteConn::connect(&spec(&f, &[])).await.unwrap());
    let qid = Uuid::new_v4();
    let running = {
        let c = Arc::clone(&c);
        tokio::spawn(async move { c.query(qid, "SELECT sleep()", 10).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    c.cancel(qid).await.unwrap();
    assert!(matches!(running.await.unwrap(), Err(DbError::Cancelled)));
    c.cancel(Uuid::new_v4()).await.unwrap();
}
