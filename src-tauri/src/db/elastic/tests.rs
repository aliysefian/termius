use super::*;
use crate::db::http::fake::{serve, Fake, Request as Seen};
use crate::db::{CellEdit, TlsMode};
use std::collections::BTreeMap;

fn spec(f: &Fake, user: &str, password: Option<&str>, options: &[(&str, &str)]) -> ConnectSpec {
    ConnectSpec {
        engine: "elasticsearch".into(),
        host: "127.0.0.1".into(),
        port: f.port,
        user: user.into(),
        password: password.map(str::to_string),
        database: None,
        tls: TlsMode::Disable,
        tunnel_port: None,
        options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

const ROOT: &str = r#"{"name":"n1","cluster_name":"prod-cluster","version":{"number":"8.11.1"},"tagline":"You Know, for Search"}"#;

fn cluster(r: &Seen) -> (u16, String) {
    let path = r.path.split('?').next().unwrap();
    match (r.method.as_str(), path) {
        ("GET", "/") => (200, ROOT.into()),
        ("GET", "/_cat/indices") => (200, r#"[{"index":".kibana","docs.count":"4","store.size":"1024","health":"green","status":"open"},{"index":"logs","docs.count":"1200","store.size":"5242880","health":"yellow","status":"open"},{"index":"books","docs.count":"3","store.size":"2048","health":"green","status":"open"}]"#.into()),
        ("GET", "/books/_mapping") => (200, r#"{"books":{"mappings":{"properties":{"title":{"type":"text"},"year":{"type":"integer"},"author":{"properties":{"name":{"type":"keyword"}}}}}}}"#.into()),
        (_, "/books/_search") => (200, r#"{"took":2,"hits":{"total":{"value":3,"relation":"eq"},"hits":[{"_index":"books","_id":"1","_score":1.0,"_source":{"title":"Dune","year":1965,"author":{"name":"Herbert"}}},{"_index":"books","_id":"2","_score":1.0,"_source":{"title":"Emma","year":1815}},{"_index":"books","_id":"3","_score":null,"_source":{"title":"Ulysses"}}]}}"#.into()),
        ("GET", "/_cat/health") => (200, r#"[{"epoch":"1","status":"green","node.total":"1"}]"#.into()),
        ("POST", "/books/_update/1") => (200, r#"{"result":"updated"}"#.into()),
        ("POST", "/books/_update/missing") => (404, r#"{"error":{"type":"document_missing_exception","reason":"[_doc][missing]: document missing"},"status":404}"#.into()),
        ("POST", "/books/_update/bad") => (400, r#"{"error":{"root_cause":[{"type":"mapper_parsing_exception","reason":"failed to parse field [year] of type [integer]"}],"type":"mapper_parsing_exception","reason":"failed to parse","caused_by":{}},"status":400}"#.into()),
        ("GET", "/nope/_search") | ("POST", "/nope/_search") => (404, r#"{"error":{"root_cause":[{"type":"index_not_found_exception","reason":"no such index [nope]"}],"type":"index_not_found_exception","reason":"no such index [nope]"},"status":404}"#.into()),
        ("HEAD", "/books") => (200, String::new()),
        ("HEAD", "/nothing") => (404, String::new()),
        ("GET", "/_cluster/health") => (200, r#"{"cluster_name":"prod-cluster","status":"green","number_of_nodes":3}"#.into()),
        _ => (404, r#"{"error":"no handler"}"#.into()),
    }
}

#[test]
fn a_request_is_a_method_a_path_and_a_json_body() {
    assert_eq!(parse_request("GET /books/_search").unwrap(), Request { method: "GET".into(), path: "/books/_search".into(), body: None });
    let r = parse_request("\n  post books/_search?pretty\n{\"query\": {\"match_all\": {}}}\n").unwrap();
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/books/_search?pretty"));
    assert_eq!(r.body.as_deref(), Some("{\"query\": {\"match_all\": {}}}"));
    assert_eq!(parse_request("POST /_bulk\n{\"index\":{}}\n{\"a\":1}\n").unwrap().body.as_deref(), Some("{\"index\":{}}\n{\"a\":1}"));
    for bad in ["", "   ", "FETCH /x", "GET", "GET /a b", "GET //evil.example/x"] {
        assert!(parse_request(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn ids_stay_one_path_segment() {
    assert_eq!(encode_segment("abc-1_2.3~"), "abc-1_2.3~");
    assert_eq!(encode_segment("a/b c?d"), "a%2Fb%20c%3Fd");
    assert_eq!(encode_segment("é"), "%C3%A9");
}

fn danger(text: &str) -> Option<String> {
    destructive(&parse_request(text).unwrap())
}

#[test]
fn what_deletes_or_rewrites_is_named() {
    for r in [
        "DELETE /books",
        "DELETE /books/_doc/1",
        "DELETE /logs-*",
        "POST /books/_delete_by_query\n{\"query\":{\"match_all\":{}}}",
        "POST /books/_update_by_query",
        "POST /books/_close",
        "POST /books/_forcemerge",
        "POST /_reindex\n{\"source\":{\"index\":\"a\"},\"dest\":{\"index\":\"b\"}}",
        "PUT /books/_settings\n{\"index\":{\"number_of_replicas\":0}}",
        "PUT /_cluster/settings\n{\"persistent\":{}}",
        "POST /_aliases\n{\"actions\":[{\"remove_index\":{\"index\":\"a\"}}]}",
        "POST /_bulk\n{\"delete\":{\"_index\":\"a\",\"_id\":\"1\"}}",
        "POST /_snapshot/repo/snap/_restore",
    ] {
        assert!(danger(r).is_some(), "{r}");
    }
    for r in ["GET /books/_search", "POST /books/_search\n{\"size\":1}", "GET /_cat/indices", "HEAD /books", "PUT /books\n{}", "POST /books/_doc\n{\"a\":1}", "POST /books/_update/1\n{\"doc\":{}}", "GET /_cluster/settings", "POST /_bulk\n{\"index\":{}}\n{\"a\":1}"] {
        assert!(danger(r).is_none(), "{r}");
    }
}

#[test]
fn replies_become_grids() {
    let search: Value = serde_json::from_str(r#"{"hits":{"total":{"value":2},"hits":[{"_index":"a","_id":"1","_score":2.5,"_source":{"x":1,"tags":["p","q"]}},{"_index":"a","_id":"2","_score":null,"_source":{"x":2,"_id":"shadow"}}]}}"#).unwrap();
    let r = lay_out(&search, 10);
    assert_eq!(r.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["_id", "_score", "x", "tags", "_source._id"]);
    assert_eq!(r.rows[0][3], json!("[\"p\",\"q\"]"));
    assert!(!r.truncated);
    let more: Value = serde_json::from_str(r#"{"hits":{"total":{"value":500},"hits":[{"_index":"a","_id":"1","_source":{"x":1}},{"_index":"b","_id":"2","_source":{"x":2}}]}}"#).unwrap();
    let r = lay_out(&more, 10);
    assert!(r.truncated, "more matched than came back");
    assert_eq!(r.columns[1].name, "_index", "several indices show which one each hit is from");
    let capped = lay_out(&search, 1);
    assert_eq!((capped.rows.len(), capped.truncated), (1, true));
    let fields: Value = serde_json::from_str(r#"{"hits":{"total":{"value":1},"hits":[{"_id":"1","fields":{"title":["Dune"],"n":[1,2]}}]}}"#).unwrap();
    let f = lay_out(&fields, 10);
    assert_eq!(f.rows[0][1], json!("Dune"));
    assert_eq!(f.rows[0][2], json!("[1,2]"));
    let aggs: Value = serde_json::from_str(r#"{"hits":{"total":{"value":0},"hits":[]},"aggregations":{"by_year":{"buckets":[{"key":1965}]}}}"#).unwrap();
    assert_eq!(lay_out(&aggs, 10).columns[0].name, "by_year");
    let list = lay_out(&json!([{"a": 1}, {"a": 2, "b": "x"}]), 10);
    assert_eq!((list.columns.len(), list.rows.len()), (2, 2));
    let one = lay_out(&json!({"acknowledged": true, "shards": {"total": 1}}), 10);
    assert_eq!(one.rows[0][0], json!(true));
    assert_eq!(lay_out(&json!(42), 10).rows[0][0], json!(42));
    assert!(lay_out(&json!([]), 10).rows.len() == 1, "an empty list is one row holding it");
}

#[tokio::test]
async fn connects_and_names_the_cluster_and_version() {
    let f = serve(cluster).await;
    let c = ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap();
    assert_eq!(c.server_version().await.unwrap(), "Elasticsearch 8.11.1");
    assert_eq!(c.children(&[]).await.unwrap()[0].name, "prod-cluster");
    let open = serve(|_r| (200, r#"{"cluster_name":"os","version":{"number":"2.11.0","distribution":"opensearch"}}"#.into())).await;
    assert_eq!(ElasticConn::connect(&spec(&open, "", None, &[])).await.unwrap().server_version().await.unwrap(), "OpenSearch 2.11.0");
}

#[tokio::test]
async fn each_kind_of_sign_in_sends_its_header_and_a_refusal_is_explained() {
    let f = serve(cluster).await;
    ElasticConn::connect(&spec(&f, "elastic", Some("pw"), &[])).await.unwrap();
    ElasticConn::connect(&spec(&f, "", Some("KEYVALUE=="), &[("auth", "api_key")])).await.unwrap();
    ElasticConn::connect(&spec(&f, "", Some("tok"), &[("auth", "bearer")])).await.unwrap();
    ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap();
    let auth: Vec<_> = f.seen.lock().unwrap().iter().map(|r| r.authorization.clone().unwrap_or_default().to_ascii_lowercase()).collect();
    assert!(auth[0].contains("basic zwxhc3rpyzpwdw=="), "{auth:?}");
    assert!(auth[1].contains("apikey keyvalue=="), "{auth:?}");
    assert!(auth[2].contains("bearer tok"), "{auth:?}");
    assert_eq!(auth[3], "", "no sign-in at all sends no header");
    let denied = serve(|_r| (401, r#"{"error":{"type":"security_exception","reason":"missing authentication credentials"},"status":401}"#.into())).await;
    assert!(matches!(ElasticConn::connect(&spec(&denied, "", None, &[])).await, Err(DbError::Server(m)) if m.contains("401") && m.contains("API key")));
    assert!(matches!(ElasticConn::connect(&spec(&f, "", None, &[("auth", "api_key")])).await, Err(DbError::Invalid(_))));
    assert!(matches!(ElasticConn::connect(&spec(&f, "", Some("x"), &[("auth", "kerberos")])).await, Err(DbError::Invalid(_))));
    let forbidden = serve(|r| if r.path == "/" { (200, ROOT.into()) } else { (403, r#"{"error":{"type":"security_exception","reason":"action [indices:data/read/search] is unauthorized"}}"#.into()) }).await;
    let c = ElasticConn::connect(&spec(&forbidden, "u", Some("p"), &[])).await.unwrap();
    assert!(matches!(c.query(Uuid::new_v4(), "GET /x/_search", 10).await, Err(DbError::Server(m)) if m.contains("403") && m.contains("unauthorized")));
}

#[tokio::test]
async fn a_search_is_asked_for_one_more_than_the_limit_and_a_real_total() {
    let f = serve(cluster).await;
    let c = ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap();
    let r = c.query(Uuid::new_v4(), "GET /books/_search\n{\"query\":{\"match_all\":{}}}", 100).await.unwrap();
    assert_eq!(r.rows.len(), 3);
    assert_eq!(r.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["_id", "_score", "title", "year", "author"]);
    assert_eq!(r.rows[0][4], json!("{\"name\":\"Herbert\"}"));
    assert_eq!(r.rows[2][1], Value::Null, "no score is an empty cell");
    let sent = f.seen.lock().unwrap().iter().rev().find(|r| r.path.starts_with("/books/_search")).map(|r| (r.method.clone(), r.body.clone())).unwrap();
    assert_eq!(sent.0, "POST", "a body goes with a POST");
    let body: Value = serde_json::from_str(&sent.1).unwrap();
    assert_eq!((body["size"].clone(), body["track_total_hits"].clone()), (json!(101), json!(true)));
    assert_eq!(body["query"], json!({"match_all": {}}));
    // A size the person chose below the limit is kept; one above is cut to it.
    c.query(Uuid::new_v4(), "POST /books/_search\n{\"size\": 5}", 100).await.unwrap();
    c.query(Uuid::new_v4(), "POST /books/_search\n{\"size\": 5000}", 100).await.unwrap();
    let sizes: Vec<_> = f.seen.lock().unwrap().iter().filter(|r| r.path.starts_with("/books/_search")).map(|r| serde_json::from_str::<Value>(&r.body).unwrap()["size"].clone()).collect();
    assert_eq!(sizes[1..], [json!(5), json!(101)]);
    let cut = c.query(Uuid::new_v4(), "GET /books/_search", 2).await.unwrap();
    assert_eq!((cut.rows.len(), cut.truncated), (2, true));
    assert!(matches!(c.query(Uuid::new_v4(), "POST /books/_search\n{ nope", 10).await, Err(DbError::Invalid(m)) if m.contains("valid JSON")));
}

#[tokio::test]
async fn cat_apis_are_asked_for_json_and_other_requests_pass_through() {
    let f = serve(cluster).await;
    let c = ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap();
    let h = c.query(Uuid::new_v4(), "GET /_cat/health", 10).await.unwrap();
    assert_eq!(h.columns[0].name, "epoch");
    assert!(f.seen.lock().unwrap().iter().any(|r| r.path == "/_cat/health?format=json"));
    let health = c.query(Uuid::new_v4(), "GET /_cluster/health", 10).await.unwrap();
    assert_eq!(health.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["cluster_name", "status", "number_of_nodes"]);
    let exists = c.query(Uuid::new_v4(), "HEAD /books", 10).await.unwrap();
    assert_eq!(exists.rows[0][1], json!(true));
    let missing = c.query(Uuid::new_v4(), "HEAD /nothing", 10).await.unwrap();
    assert_eq!(missing.rows[0][1], json!(false));
    match c.query(Uuid::new_v4(), "GET /nope/_search", 10).await {
        Err(DbError::Server(m)) => assert_eq!(m, "index_not_found_exception: no such index [nope]"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(c.query(Uuid::new_v4(), "SEARCH /x", 10).await, Err(DbError::Invalid(_))));
}

#[tokio::test]
async fn the_tree_shows_indices_with_counts_and_sizes_and_their_fields() {
    let f = serve(cluster).await;
    let c = ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap();
    let idx = c.children(&["prod-cluster".into()]).await.unwrap();
    assert_eq!(idx.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["books", "logs", ".kibana"], "system indices last");
    assert_eq!(idx[0].detail.as_deref(), Some("3 docs · 2.0 KB · green"));
    assert_eq!(idx[1].detail.as_deref(), Some("1200 docs · 5.0 MB · yellow"));
    let fields = c.children(&["prod-cluster".into(), "books".into()]).await.unwrap();
    assert_eq!(fields.iter().map(|n| (n.name.as_str(), n.detail.as_deref())).collect::<Vec<_>>(), [("author", Some("object")), ("title", Some("text")), ("year", Some("integer"))]);
    let info = c.table_info("prod-cluster", "books").await.unwrap();
    assert_eq!((info.primary_key.as_slice(), info.columns[0].name.as_str(), info.columns.len()), (&["_id".to_string()][..], "_id", 4));
}

fn edit(table: &str, id: &str, changes: &[(&str, Option<&str>)]) -> RowEdit {
    RowEdit { database: "prod-cluster".into(), table: table.into(), key: vec![CellEdit { column: "_id".into(), value: Some(id.into()) }], changes: changes.iter().map(|(c, v)| CellEdit { column: c.to_string(), value: v.map(str::to_string) }).collect() }
}

#[tokio::test]
async fn an_edit_is_a_partial_update_with_typed_values_and_a_safe_path() {
    let f = serve(cluster).await;
    let c = ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap();
    assert_eq!(c.apply_update(&edit("books", "1", &[("year", Some("1966")), ("title", Some("Dune Messiah")), ("tags", Some("[\"a\"]")), ("gone", None)])).await.unwrap(), 1);
    let sent = f.seen.lock().unwrap().iter().rev().find(|r| r.path.starts_with("/books/_update/1")).map(|r| (r.path.clone(), r.body.clone())).unwrap();
    assert_eq!(sent.0, "/books/_update/1?refresh=wait_for");
    assert_eq!(serde_json::from_str::<Value>(&sent.1).unwrap(), json!({"doc": {"year": 1966, "title": "Dune Messiah", "tags": ["a"], "gone": null}}));
    assert_eq!(c.apply_update(&edit("books", "missing", &[("a", Some("1"))])).await.unwrap(), 0, "a document that is gone changes nothing");
    assert!(matches!(c.apply_update(&edit("books", "bad", &[("year", Some("x"))])).await, Err(DbError::Server(m)) if m.contains("failed to parse")));
    let preview = c.preview_update(&edit("my books", "a/b", &[("n", Some("1"))])).unwrap();
    assert!(preview.starts_with("POST /my%20books/_update/a%2Fb?refresh=wait_for\n") && preview.contains("\"n\": 1"), "{preview}");
    for bad in [edit("books", "1", &[("_source", Some("x"))]), edit("books", "1", &[("", Some("x"))])] {
        assert!(matches!(c.preview_update(&bad), Err(DbError::Invalid(_))));
    }
    let mut keyless = edit("books", "1", &[("a", Some("b"))]);
    keyless.key[0].column = "title".into();
    assert!(matches!(c.preview_update(&keyless), Err(DbError::Invalid(_))));
    assert!(c.destructive_reason("DELETE /books").is_some() && c.destructive_reason("GET /books/_search").is_none() && !c.is_sql());
}

#[tokio::test]
async fn not_elasticsearch_and_not_there_are_explained_and_a_request_can_be_cancelled() {
    let html = serve(|_r| (200, "<html>hello</html>".into())).await;
    assert!(matches!(ElasticConn::connect(&spec(&html, "", None, &[])).await, Err(DbError::Server(m)) if m.contains("not like Elasticsearch")));
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut s = spec(&html, "", None, &[]);
    s.port = closed;
    assert!(matches!(ElasticConn::connect(&s).await, Err(DbError::Connect { .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_search_can_be_cancelled() {
    let f = serve(|r| {
        if r.path == "/" {
            return (200, ROOT.into());
        }
        std::thread::sleep(Duration::from_millis(1500));
        (200, r#"{"hits":{"total":{"value":0},"hits":[]}}"#.into())
    })
    .await;
    let c = std::sync::Arc::new(ElasticConn::connect(&spec(&f, "", None, &[])).await.unwrap());
    let qid = Uuid::new_v4();
    let running = {
        let c = c.clone();
        tokio::spawn(async move { c.query(qid, "GET /big/_search", 10).await })
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    c.cancel(qid).await.unwrap();
    assert!(matches!(running.await.unwrap(), Err(DbError::Cancelled)));
}
