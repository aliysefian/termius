//! Tests against a real MySQL or MariaDB server. They are skipped unless
//! `SSHVAULT_TEST_MYSQL` is set, as `user:password@host:port/database`, and
//! the database must hold the `items` table the setup note below creates.
//!
//! ```text
//! docker run -d -e MARIADB_ROOT_PASSWORD=rootpw -e MARIADB_DATABASE=shop \
//!     -p 127.0.0.1:53306:3306 mariadb:11
//! # then, once it is up:
//! CREATE TABLE items(id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50),
//!   price DECIMAL(10,2), note TEXT, blob_col BLOB,
//!   created DATETIME DEFAULT CURRENT_TIMESTAMP);
//! INSERT INTO items(name,price,note) VALUES('a',1.50,'héllo'),('b',2.25,NULL);
//! CREATE VIEW v_items AS SELECT id,name FROM items;
//! SSHVAULT_TEST_MYSQL=root:rootpw@127.0.0.1:53306/shop cargo test live -- --ignored
//! ```

use serde_json::json;
use uuid::Uuid;

use super::*;

fn spec() -> Option<ConnectSpec> {
    let raw = std::env::var("SSHVAULT_TEST_MYSQL").ok()?;
    let (cred, rest) = raw.split_once('@')?;
    let (user, pass) = cred.split_once(':')?;
    let (addr, db) = rest.split_once('/')?;
    let (host, port) = addr.rsplit_once(':')?;
    Some(ConnectSpec {
        engine: "mysql".into(),
        host: host.into(),
        port: port.parse().ok()?,
        user: user.into(),
        password: Some(pass.into()),
        database: Some(db.into()),
        tls: TlsMode::Disable,
        tunnel_port: None,
    })
}

async fn engine() -> Option<(Engine, ConnectSpec)> {
    let s = spec()?;
    Some((Engine::connect(&s).await.expect("connect"), s))
}

#[tokio::test]
#[ignore]
async fn live_browse_and_describe() {
    let Some((e, _)) = engine().await else { return };
    assert!(!e.server_version().await.unwrap().is_empty());

    let roots = e.children(&[]).await.unwrap();
    let shop = roots.iter().position(|n| n.name == "shop").expect("shop listed");
    let sys = roots.iter().position(|n| n.name == "information_schema").expect("system schema listed");
    assert!(shop < sys, "user databases sort before system ones");

    let tables = e.children(&["shop".into()]).await.unwrap();
    let items = tables.iter().find(|n| n.name == "items").unwrap();
    assert_eq!(items.kind, NodeKind::Table);
    assert_eq!(tables.iter().find(|n| n.name == "v_items").unwrap().kind, NodeKind::View);

    let inner = e.children(&["shop".into(), "items".into()]).await.unwrap();
    let id = inner.iter().find(|n| n.name == "id" && n.kind == NodeKind::Column).unwrap();
    assert!(id.detail.as_deref().unwrap().contains("PK"));
    assert!(inner.iter().any(|n| n.name == "PRIMARY" && n.kind == NodeKind::Index));

    let info = e.table_info("shop", "items").await.unwrap();
    assert_eq!(info.primary_key, vec!["id".to_string()]);
    assert!(info.columns.iter().any(|c| c.name == "note" && c.nullable));
    assert!(e.table_info("shop", "v_items").await.unwrap().primary_key.is_empty());
}

#[tokio::test]
#[ignore]
async fn live_query_results_and_limits() {
    let Some((e, _)) = engine().await else { return };
    let r = e.query(Uuid::new_v4(), "SELECT id, name, price, note, blob_col, created FROM items ORDER BY id", 100).await.unwrap();
    assert_eq!(r.columns.iter().map(|c| c.kind).collect::<Vec<_>>(), vec![
        ColumnKind::Number, ColumnKind::Text, ColumnKind::Number, ColumnKind::Text, ColumnKind::Binary, ColumnKind::DateTime
    ]);
    assert_eq!(r.columns[3].data_type, "text");
    assert_eq!(r.rows[0][0], json!(1));
    assert_eq!(r.rows[0][2], json!("1.50"), "decimals keep their digits");
    assert_eq!(r.rows[0][3], json!("héllo"));
    assert_eq!(r.rows[1][3], json!(null));
    assert!(!r.truncated);

    // A big result is cut at the limit, quickly, and the session still works.
    let t = std::time::Instant::now();
    let big = e
        .query(
            Uuid::new_v4(),
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < 300000) SELECT i, REPEAT('x', 100) FROM n",
            1000,
        )
        .await
        .unwrap();
    assert_eq!(big.rows.len(), 1000);
    assert!(big.truncated);
    assert!(t.elapsed() < std::time::Duration::from_secs(10), "{:?}", t.elapsed());
    assert_eq!(e.query(Uuid::new_v4(), "SELECT 1", 10).await.unwrap().rows[0][0], json!(1));

    // Statements without a result set report what they changed.
    let w = e.query(Uuid::new_v4(), "UPDATE items SET note = note WHERE id = 1", 10).await.unwrap();
    assert!(w.columns.is_empty());
    assert!(w.affected_rows.is_some());

    // Huge text is cut and marked, never silently shortened.
    let long = e.query(Uuid::new_v4(), "SELECT REPEAT('é', 100000) AS t", 10).await.unwrap();
    assert_eq!(long.rows[0][0]["truncated"], json!(true));

    let bin = e.query(Uuid::new_v4(), "SELECT UNHEX('00ff10') AS b", 10).await.unwrap();
    assert_eq!(bin.rows[0][0], json!("0x00ff10"));
}

#[tokio::test]
#[ignore]
async fn live_errors_are_the_servers_own_words() {
    let Some((e, s)) = engine().await else { return };
    let err = e.query(Uuid::new_v4(), "SELEC 1", 10).await.unwrap_err().to_string();
    assert!(err.to_lowercase().contains("syntax"), "{err}");
    let err = e.query(Uuid::new_v4(), "SELECT * FROM nope", 10).await.unwrap_err().to_string();
    assert!(err.contains("nope"), "{err}");

    let bad = ConnectSpec { password: Some("wrong".into()), ..s.clone() };
    let err = Engine::connect(&bad).await.err().expect("must fail").to_string();
    assert!(err.to_lowercase().contains("access denied"), "{err}");

    let closed = ConnectSpec { port: 1, ..s };
    let err = Engine::connect(&closed).await.err().expect("must fail").to_string();
    assert!(err.contains("127.0.0.1:1"), "{err}");
}

#[tokio::test]
#[ignore]
async fn live_cancel_stops_a_running_query() {
    let Some((e, _)) = engine().await else { return };
    let e = std::sync::Arc::new(e);
    let qid = Uuid::new_v4();
    let runner = {
        let e = e.clone();
        tokio::spawn(async move { e.query(qid, "SELECT SLEEP(30)", 10).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let t = std::time::Instant::now();
    e.cancel(qid).await.unwrap();
    let out = runner.await.unwrap();
    assert!(matches!(out, Err(DbError::Cancelled)), "{out:?}");
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    // Cancelling something that already finished is harmless.
    e.cancel(qid).await.unwrap();
    assert_eq!(e.query(Uuid::new_v4(), "SELECT 2", 10).await.unwrap().rows[0][0], json!(2));
}

#[tokio::test]
#[ignore]
async fn live_destructive_statements_need_confirmation() {
    let Some(s) = spec() else { return };
    let mgr = DbManager::new();
    let (id, version) = mgr.open(s, None).await.unwrap();
    assert!(!version.is_empty());
    let session = mgr.get(id).unwrap();

    let err = run_query(&session, Uuid::new_v4(), "DELETE FROM items", None, false).await.unwrap_err();
    assert!(matches!(err, DbError::NeedsConfirmation(ref r) if r == "DELETE without WHERE"), "{err}");
    let count = run_query(&session, Uuid::new_v4(), "SELECT COUNT(*) FROM items", None, false).await.unwrap();
    assert_eq!(count.rows[0][0], json!(2), "nothing was deleted");

    // Confirmed statements go through; blank input is refused.
    run_query(&session, Uuid::new_v4(), "SELECT 1", None, true).await.unwrap();
    assert!(matches!(run_query(&session, Uuid::new_v4(), "  ", None, false).await, Err(DbError::Invalid(_))));

    // A transaction can't be carried to the next run, so a dangling BEGIN is refused up front.
    assert!(matches!(run_query(&session, Uuid::new_v4(), "BEGIN", None, true).await, Err(DbError::Invalid(_))));
    run_query(&session, Uuid::new_v4(), "BEGIN; SELECT 1; COMMIT", None, false).await.unwrap();

    mgr.close(id).await;
    assert!(matches!(mgr.get(id), Err(DbError::NoSession)));
    assert_eq!(mgr.open_count(), 0);
}

#[tokio::test]
#[ignore]
async fn live_inline_edit_changes_one_row() {
    let Some((e, _)) = engine().await else { return };
    let before = e.query(Uuid::new_v4(), "SELECT name, note FROM items WHERE id = 2", 10).await.unwrap();
    let edit = RowEdit {
        database: "shop".into(),
        table: "items".into(),
        key: vec![CellEdit { column: "id".into(), value: Some("2".into()) }],
        changes: vec![
            CellEdit { column: "name".into(), value: Some("b'; DROP TABLE items; --".into()) },
            CellEdit { column: "note".into(), value: None },
        ],
    };
    let preview = e.preview_update(&edit).unwrap();
    assert!(preview.starts_with("UPDATE `shop`.`items` SET"), "{preview}");
    assert_eq!(e.apply_update(&edit).await.unwrap(), 1);

    let after = e.query(Uuid::new_v4(), "SELECT name, note FROM items WHERE id = 2", 10).await.unwrap();
    assert_eq!(after.rows[0][0], json!("b'; DROP TABLE items; --"));
    assert_eq!(after.rows[0][1], json!(null));
    assert_eq!(e.query(Uuid::new_v4(), "SELECT COUNT(*) FROM items", 10).await.unwrap().rows[0][0], json!(2));

    // Put it back, and a key that matches nothing changes nothing.
    let restore = RowEdit {
        changes: vec![
            CellEdit { column: "name".into(), value: before.rows[0][0].as_str().map(str::to_string) },
            CellEdit { column: "note".into(), value: None },
        ],
        ..edit.clone()
    };
    assert_eq!(e.apply_update(&restore).await.unwrap(), 1);
    let ghost = RowEdit { key: vec![CellEdit { column: "id".into(), value: Some("999".into()) }], ..restore };
    assert_eq!(e.apply_update(&ghost).await.unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_connects_through_an_ssh_tunnel_and_closes_it() {
    use crate::ssh::testutil::{spawn_sshd, target};
    let Some(s) = spec() else { return };
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let via = target(&sshd, &sshd.client_key, dir.path().join("kh"));

    // The tunnel on its own: reachable while held, gone once dropped.
    let tunnel = crate::forward::open_local_tunnel(&via, s.host.clone(), s.port).await.unwrap();
    let port = tunnel.port();
    let through = Engine::connect(&ConnectSpec { tunnel_port: Some(port), ..s.clone() }).await.unwrap();
    assert_eq!(through.query(Uuid::new_v4(), "SELECT 41 + 1", 10).await.unwrap().rows[0][0], json!(42));
    through.close().await;
    drop(tunnel);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err(), "the tunnel port must close with the session");

    // The same path through the manager, which owns the tunnel's lifetime.
    let mgr = DbManager::new();
    let (id, _) = mgr.open(s.clone(), Some(via.clone())).await.unwrap();
    let session = mgr.get(id).unwrap();
    assert_eq!(run_query(&session, Uuid::new_v4(), "SELECT 'hi'", None, false).await.unwrap().rows[0][0], json!("hi"));
    drop(session);
    mgr.close(id).await;

    // A database that isn't there fails with the reason, not a hang.
    let dead = ConnectSpec { port: 1, ..s };
    let err = mgr.open(dead, Some(via)).await.expect_err("must fail").to_string();
    assert!(err.contains("127.0.0.1"), "{err}");
}
