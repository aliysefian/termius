//! Tests against a real PostgreSQL server. Skipped unless `SSHVAULT_TEST_PG`
//! is set, as `user:password@host:port/database`. The server needs TLS on;
//! `SSHVAULT_TEST_PG_CERT` is the path of the CA that signed its certificate (the leaf must be CA:FALSE, with a SAN for localhost and 127.0.0.1), for
//! the verify-full tests. Setup:
//!
//! ```text
//! # a certificate for localhost / 127.0.0.1, key owned by uid 70 (postgres)
//! docker create --name pg -e POSTGRES_PASSWORD=pgpw -e POSTGRES_DB=shop \
//!   -p 127.0.0.1:53432:5432 postgres:16-alpine -c ssl=on \
//!   -c ssl_cert_file=/var/lib/postgresql/ssl/server.crt \
//!   -c ssl_key_file=/var/lib/postgresql/ssl/server.key
//! # copy ssl/server.{crt,key} in (mode 600 for the key), docker start pg, then:
//! CREATE SCHEMA warehouse;
//! CREATE TABLE public.items(id serial PRIMARY KEY, name varchar(50), price numeric(10,2),
//!   note text, data bytea, created timestamptz DEFAULT now(), active boolean DEFAULT true, meta jsonb);
//! INSERT INTO public.items(name, price, note, data, meta) VALUES
//!   ('a', 1.50, 'héllo', '\x00ff10', '{"k": 1}'), ('b', 2.25, NULL, NULL, NULL);
//! CREATE VIEW public.v_items AS SELECT id, name FROM public.items;
//! CREATE MATERIALIZED VIEW public.m_items AS SELECT id FROM public.items;
//! CREATE TABLE warehouse.pairs(a int, b text, v text, PRIMARY KEY (a, b));
//! INSERT INTO warehouse.pairs VALUES (1, 'x', 'one'), (1, 'y', 'two');
//! CREATE TABLE public."Odd ""Name"""(id int PRIMARY KEY, "Weird Col" text);
//! INSERT INTO public."Odd ""Name""" VALUES (1, 'w');
//! CREATE INDEX items_name_idx ON public.items (name, price);
//! SSHVAULT_TEST_PG=postgres:pgpw@127.0.0.1:53432/shop SSHVAULT_TEST_PG_CERT=server.crt \
//!   cargo test live_pg -- --ignored --test-threads=1
//! ```

use serde_json::json;
use uuid::Uuid;

use super::*;

fn spec(tls: TlsMode) -> Option<ConnectSpec> {
    let raw = std::env::var("SSHVAULT_TEST_PG").ok()?;
    let (cred, rest) = raw.split_once('@')?;
    let (user, pass) = cred.split_once(':')?;
    let (addr, db) = rest.split_once('/')?;
    let (host, port) = addr.rsplit_once(':')?;
    Some(ConnectSpec {
        engine: "postgres".into(),
        host: host.into(),
        port: port.parse().ok()?,
        user: user.into(),
        password: Some(pass.into()),
        database: Some(db.into()),
        tls,
        tunnel_port: None,
        options: Default::default(),
    })
}

async fn engine() -> Option<Engine> {
    Some(Engine::connect(&spec(TlsMode::Require)?).await.expect("connect"))
}

async fn q(e: &Engine, sql: &str) -> QueryResult {
    e.query(Uuid::new_v4(), sql, 1000).await.unwrap_or_else(|err| panic!("{sql}: {err}"))
}

#[tokio::test]
#[ignore]
async fn live_pg_browse_and_describe() {
    let Some(e) = engine().await else { return };
    assert!(e.server_version().await.unwrap().starts_with("16"));

    let roots = e.children(&[]).await.unwrap();
    let pos = |n: &str| roots.iter().position(|x| x.name == n).unwrap_or_else(|| panic!("{n} not listed"));
    assert!(pos("public") < pos("pg_catalog") && pos("warehouse") < pos("information_schema"), "user schemas sort first");
    assert!(roots.iter().all(|n| n.kind == NodeKind::Schema));
    assert!(!roots.iter().any(|n| n.name.starts_with("pg_toast")), "toast schemas are hidden");
    assert_eq!(roots[pos("pg_catalog")].detail.as_deref(), Some("system"));

    let rels = e.children(&["public".into()]).await.unwrap();
    let by = |n: &str| rels.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n} missing"));
    assert_eq!(by("items").kind, NodeKind::Table);
    assert_eq!(by("v_items").kind, NodeKind::View);
    assert_eq!(by("m_items").detail.as_deref(), Some("materialized view"));
    assert_eq!(by("Odd \"Name\"").kind, NodeKind::Table);

    let inner = e.children(&["public".into(), "items".into()]).await.unwrap();
    let id = inner.iter().find(|n| n.name == "id" && n.kind == NodeKind::Column).unwrap();
    assert!(id.detail.as_deref().unwrap().contains("PK") && id.detail.as_deref().unwrap().contains("integer"));
    let idx: Vec<_> = inner.iter().filter(|n| n.kind == NodeKind::Index).collect();
    assert!(idx.iter().any(|n| n.name == "items_pkey" && n.detail.as_deref() == Some("unique (id)")), "{idx:?}");
    assert!(idx.iter().any(|n| n.name == "items_name_idx" && n.detail.as_deref() == Some("(name,price)")), "{idx:?}");

    assert_eq!(e.table_info("public", "items").await.unwrap().primary_key, vec!["id".to_string()]);
    assert_eq!(e.table_info("warehouse", "pairs").await.unwrap().primary_key, vec!["a".to_string(), "b".to_string()]);
    assert!(e.table_info("public", "v_items").await.unwrap().primary_key.is_empty());
    let odd = e.table_info("public", "Odd \"Name\"").await.unwrap();
    assert!(odd.columns.iter().any(|c| c.name == "Weird Col"));
    let items = e.table_info("public", "items").await.unwrap();
    assert!(items.columns.iter().any(|c| c.name == "id" && c.default.as_deref().is_some_and(|d| d.contains("nextval"))));
    assert!(items.columns.iter().any(|c| c.name == "note" && c.nullable));
}

#[tokio::test]
#[ignore]
async fn live_pg_results_types_and_limits() {
    let Some(e) = engine().await else { return };
    let r = q(&e, "SELECT id, name, price, note, data, created, active, meta FROM items ORDER BY id").await;
    let kinds: Vec<_> = r.columns.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ColumnKind::Number, ColumnKind::Text, ColumnKind::Number, ColumnKind::Text,
            ColumnKind::Binary, ColumnKind::DateTime, ColumnKind::Other, ColumnKind::Json
        ]
    );
    assert_eq!(r.columns[0].data_type, "int4");
    assert_eq!(r.rows[0][0], json!(1));
    assert_eq!(r.rows[0][2], json!("1.50"), "numeric keeps its digits");
    assert_eq!(r.rows[0][3], json!("héllo"));
    assert_eq!(r.rows[1][3], json!(null));
    assert_eq!(r.rows[0][4], json!("0x00ff10"));
    assert!(r.rows[0][5].as_str().unwrap().starts_with("20"));
    assert_eq!(r.rows[0][6], json!(true));
    assert_eq!(r.rows[0][7], json!("{\"k\": 1}"));

    // No rows still describes the columns.
    let none = q(&e, "SELECT id, name FROM items WHERE false").await;
    assert_eq!(none.columns.len(), 2);
    assert!(none.rows.is_empty() && none.affected_rows.is_none());

    // A big result stops at the limit quickly, and the connection is fine afterwards.
    let t = std::time::Instant::now();
    let big = e
        .query(Uuid::new_v4(), "SELECT i, repeat('x', 100) FROM generate_series(1, 3000000) i", 1000)
        .await
        .unwrap();
    assert_eq!(big.rows.len(), 1000);
    assert!(big.truncated);
    assert!(t.elapsed() < std::time::Duration::from_secs(10), "{:?}", t.elapsed());
    for _ in 0..3 {
        assert_eq!(q(&e, "SELECT 1").await.rows[0][0], json!(1), "reused connection must be clean after a cut-short read");
    }

    // Statements without a result set report what they changed.
    let w = q(&e, "UPDATE items SET note = note WHERE id = 1").await;
    assert!(w.columns.is_empty());
    assert_eq!(w.affected_rows, Some(1));

    // The first result set of a script is shown.
    let multi = q(&e, "SELECT 1 AS a; SELECT 2 AS b").await;
    assert_eq!(multi.columns[0].name, "a");
    assert_eq!(multi.rows, vec![vec![json!("1")]], "no describe for scripts, so values stay text");

    // RETURNING comes back as rows.
    let ret = q(&e, "UPDATE items SET note = note WHERE id = 2 RETURNING id").await;
    assert_eq!(ret.rows[0][0], json!(2));

    let long = q(&e, "SELECT repeat('é', 100000) AS t").await;
    assert_eq!(long.rows[0][0]["truncated"], json!(true));
    let f = q(&e, "SELECT 'NaN'::float8 AS n, 9007199254740993::int8 AS big").await;
    assert_eq!(f.rows[0], vec![json!("NaN"), json!("9007199254740993")]);
}

#[tokio::test]
#[ignore]
async fn live_pg_errors_are_the_servers_own_words() {
    let Some(e) = engine().await else { return };
    let err = e.query(Uuid::new_v4(), "SELEC 1", 10).await.unwrap_err().to_string();
    assert!(err.to_lowercase().contains("syntax"), "{err}");
    let err = e.query(Uuid::new_v4(), "SELECT * FROM nope", 10).await.unwrap_err().to_string();
    assert!(err.contains("nope") && err.contains("42P01"), "{err}");
    let err = e.query(Uuid::new_v4(), "SELECT 1/0", 10).await.unwrap_err().to_string();
    assert!(err.contains("division by zero"), "{err}");
    // A runtime error mid-script leaves the connection usable.
    let err = e.query(Uuid::new_v4(), "SELECT 1; SELECT 1/0", 10).await.unwrap_err().to_string();
    assert!(err.contains("division by zero"), "{err}");
    assert_eq!(q(&e, "SELECT 5").await.rows[0][0], json!(5));

    let s = spec(TlsMode::Require).unwrap();
    let bad = ConnectSpec { password: Some("wrong".into()), ..s.clone() };
    let err = Engine::connect(&bad).await.err().expect("must fail").to_string();
    assert!(err.contains("password authentication failed"), "{err}");
    let closed = ConnectSpec { port: 1, ..s.clone() };
    let err = Engine::connect(&closed).await.err().expect("must fail").to_string();
    assert!(err.contains("127.0.0.1:1"), "{err}");
    let nodb = ConnectSpec { database: Some("no_such_db".into()), ..s };
    let err = Engine::connect(&nodb).await.err().expect("must fail").to_string();
    assert!(err.contains("no_such_db"), "{err}");
}

#[tokio::test]
#[ignore]
async fn live_pg_cancel_stops_a_running_query() {
    let Some(e) = engine().await else { return };
    let e = std::sync::Arc::new(e);
    let qid = Uuid::new_v4();
    let runner = {
        let e = e.clone();
        tokio::spawn(async move { e.query(qid, "SELECT pg_sleep(30)", 10).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let t = std::time::Instant::now();
    e.cancel(qid).await.unwrap();
    let out = runner.await.unwrap();
    assert!(matches!(out, Err(DbError::Cancelled)), "{out:?}");
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    e.cancel(qid).await.unwrap();
    for _ in 0..3 {
        assert_eq!(q(&e, "SELECT 2").await.rows[0][0], json!(2));
    }

    // Two queries at once each get their own connection.
    let a = {
        let e = e.clone();
        tokio::spawn(async move { e.query(Uuid::new_v4(), "SELECT pg_sleep(1), 'a'", 10).await })
    };
    let b = {
        let e = e.clone();
        tokio::spawn(async move { e.query(Uuid::new_v4(), "SELECT pg_sleep(1), 'b'", 10).await })
    };
    let t = std::time::Instant::now();
    let (a, b) = (a.await.unwrap().unwrap(), b.await.unwrap().unwrap());
    assert_eq!((a.rows[0][1].clone(), b.rows[0][1].clone()), (json!("a"), json!("b")));
    assert!(t.elapsed() < std::time::Duration::from_millis(1900), "overlapping queries must not queue: {:?}", t.elapsed());
}

#[tokio::test]
#[ignore]
async fn live_pg_session_state_survives_between_runs() {
    let Some(e) = engine().await else { return };
    q(&e, "SET search_path TO warehouse").await;
    assert_eq!(q(&e, "SELECT count(*) FROM pairs").await.rows[0][0], json!(2), "search_path set in one run applies to the next");
    q(&e, "CREATE TEMP TABLE scratch_t(x int)").await;
    q(&e, "INSERT INTO scratch_t VALUES (7)").await;
    assert_eq!(q(&e, "SELECT x FROM scratch_t").await.rows[0][0], json!(7));
    q(&e, "RESET search_path").await;
}

#[tokio::test]
#[ignore]
async fn live_pg_guards_destructive_statements_and_open_transactions() {
    let Some(s) = spec(TlsMode::Require) else { return };
    let mgr = DbManager::new();
    let (id, version) = mgr.open(s, None).await.unwrap();
    assert!(version.starts_with("16"));
    let session = mgr.get(id).unwrap();
    let run = |sql: &'static str, ok: bool| {
        let session = session.clone();
        async move { run_query(&session, Uuid::new_v4(), sql, None, ok).await }
    };

    let err = run("DELETE FROM items", false).await.unwrap_err();
    assert!(matches!(err, DbError::NeedsConfirmation(ref r) if r == "DELETE without WHERE"), "{err}");
    assert_eq!(run("SELECT count(*) FROM items", false).await.unwrap().rows[0][0], json!(2));

    // A transaction can't span runs, so a dangling BEGIN is refused before it reaches the server.
    assert!(matches!(run("BEGIN", true).await, Err(DbError::Invalid(_))));
    // One that ends in the same run is fine, and commits.
    run("BEGIN; INSERT INTO warehouse.pairs VALUES (9, 'z', 'tx'); COMMIT", false).await.unwrap();
    assert_eq!(run("SELECT v FROM warehouse.pairs WHERE a = 9", false).await.unwrap().rows[0][0], json!("tx"));
    run("DELETE FROM warehouse.pairs WHERE a = 9", false).await.unwrap();
    // A script that fails inside an explicit transaction doesn't poison the connection.
    assert!(run("BEGIN; SELECT 1/0; COMMIT", false).await.is_err());
    assert_eq!(run("SELECT 3", false).await.unwrap().rows[0][0], json!(3));

    // Dollar-quoted bodies run as one statement.
    run("DO $$ BEGIN PERFORM 1; PERFORM 2; END $$", false).await.unwrap();

    mgr.close(id).await;
    assert!(matches!(mgr.get(id), Err(DbError::NoSession)));
}

#[tokio::test]
#[ignore]
async fn live_pg_inline_edit_changes_one_row() {
    let Some(e) = engine().await else { return };
    let cell = |c: &str, v: Option<&str>| CellEdit { column: c.into(), value: v.map(str::to_string) };

    // Integer key given as text; a value that looks like SQL stays a value.
    let before = q(&e, "SELECT name, note FROM items WHERE id = 2").await;
    let edit = RowEdit {
        database: "public".into(),
        table: "items".into(),
        key: vec![cell("id", Some("2"))],
        changes: vec![cell("name", Some("b'; DROP TABLE items; --")), cell("note", None)],
    };
    let preview = e.preview_update(&edit).unwrap();
    assert!(preview.starts_with("UPDATE \"public\".\"items\" SET"), "{preview}");
    assert_eq!(e.apply_update(&edit).await.unwrap(), 1);
    let after = q(&e, "SELECT name, note FROM items WHERE id = 2").await;
    assert_eq!(after.rows[0], vec![json!("b'; DROP TABLE items; --"), json!(null)]);
    assert_eq!(q(&e, "SELECT count(*) FROM items").await.rows[0][0], json!(2));
    let restore = RowEdit {
        changes: vec![cell("name", before.rows[0][0].as_str()), cell("note", None)],
        ..edit.clone()
    };
    assert_eq!(e.apply_update(&restore).await.unwrap(), 1);

    // Typed columns take their text form: numeric, boolean, timestamp, jsonb.
    let typed = RowEdit {
        database: "public".into(),
        table: "items".into(),
        key: vec![cell("id", Some("1"))],
        changes: vec![cell("price", Some("12.30")), cell("active", Some("false")), cell("meta", Some("{\"z\": [1, 2]}"))],
    };
    assert_eq!(e.apply_update(&typed).await.unwrap(), 1);
    let r = q(&e, "SELECT price, active, meta FROM items WHERE id = 1").await;
    assert_eq!(r.rows[0], vec![json!("12.30"), json!(false), json!("{\"z\": [1, 2]}")]);
    let back = RowEdit { changes: vec![cell("price", Some("1.50")), cell("active", Some("true")), cell("meta", Some("{\"k\": 1}"))], ..typed };
    assert_eq!(e.apply_update(&back).await.unwrap(), 1);

    // A value the column can't hold is the server's error, and changes nothing.
    let bad = RowEdit {
        database: "public".into(),
        table: "items".into(),
        key: vec![cell("id", Some("1"))],
        changes: vec![cell("price", Some("not a number"))],
    };
    let err = e.apply_update(&bad).await.unwrap_err().to_string();
    assert!(err.contains("not a number") && err.contains("22P02"), "{err}");

    // Composite keys, odd names, and a key that matches nothing.
    let composite = RowEdit {
        database: "warehouse".into(),
        table: "pairs".into(),
        key: vec![cell("a", Some("1")), cell("b", Some("y"))],
        changes: vec![cell("v", Some("changed"))],
    };
    assert_eq!(e.apply_update(&composite).await.unwrap(), 1);
    let r = q(&e, "SELECT v FROM warehouse.pairs ORDER BY b").await;
    assert_eq!(r.rows, vec![vec![json!("one")], vec![json!("changed")]], "only the keyed row changed");
    let undo = RowEdit { changes: vec![cell("v", Some("two"))], ..composite };
    e.apply_update(&undo).await.unwrap();

    let odd = RowEdit {
        database: "public".into(),
        table: "Odd \"Name\"".into(),
        key: vec![cell("id", Some("1"))],
        changes: vec![cell("Weird Col", Some("w2"))],
    };
    assert_eq!(e.apply_update(&odd).await.unwrap(), 1);
    assert_eq!(e.apply_update(&RowEdit { changes: vec![cell("Weird Col", Some("w"))], ..odd.clone() }).await.unwrap(), 1);
    let ghost = RowEdit { key: vec![cell("id", Some("999"))], ..odd };
    assert_eq!(e.apply_update(&ghost).await.unwrap(), 0);
}

#[tokio::test]
#[ignore]
async fn live_pg_tls_modes() {
    let Some(base) = spec(TlsMode::Disable) else { return };
    let encrypted = |e: Engine| async move { q(&e, "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await.rows[0][0].clone() };

    // None: works, and the session really is unencrypted.
    let plain = Engine::connect(&base).await.unwrap();
    assert_eq!(encrypted(plain).await, json!(false));

    // Require: encrypted, certificate not checked.
    let req = Engine::connect(&ConnectSpec { tls: TlsMode::Require, ..base.clone() }).await.unwrap();
    assert_eq!(encrypted(req).await, json!(true));

    // Verify: a self-signed certificate nobody trusts is refused, with the reason.
    let err = Engine::connect(&ConnectSpec { tls: TlsMode::VerifyFull, ..base.clone() }).await.err().expect("untrusted cert must fail").to_string();
    assert!(err.to_lowercase().contains("certificate") || err.contains("UnknownIssuer"), "{err}");

    // Once the certificate is in the trust store (here, SSL_CERT_FILE), it connects, by name or address.
    let Ok(cert) = std::env::var("SSHVAULT_TEST_PG_CERT") else { return };
    std::env::set_var("SSL_CERT_FILE", &cert);
    for host in ["localhost", "127.0.0.1"] {
        let ok = Engine::connect(&ConnectSpec { tls: TlsMode::VerifyFull, host: host.into(), ..base.clone() }).await;
        let e = ok.unwrap_or_else(|e| panic!("verify-full to {host} with the cert trusted: {e}"));
        assert_eq!(encrypted(e).await, json!(true));
    }
    // A name the certificate doesn't cover is refused even though the chain is trusted.
    // 127.0.0.2 is also loopback on Linux, but nothing listens there, so dial the tunnel-style way.
    let wrong = ConnectSpec { tls: TlsMode::VerifyFull, host: "wrong.example".into(), tunnel_port: Some(base.port), ..base.clone() };
    let err = Engine::connect(&wrong).await.err().expect("name mismatch must fail").to_string();
    assert!(err.to_lowercase().contains("not valid for name") || err.to_lowercase().contains("certificate"), "{err}");
    std::env::remove_var("SSL_CERT_FILE");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn live_pg_connects_through_an_ssh_tunnel() {
    use crate::ssh::testutil::{spawn_sshd, target};
    let Some(base) = spec(TlsMode::Require) else { return };
    let dir = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(dir.path()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let via = target(&sshd, &sshd.client_key, dir.path().join("kh"));

    let mgr = DbManager::new();
    let (id, _) = mgr.open(base.clone(), Some(via.clone())).await.unwrap();
    let session = mgr.get(id).unwrap();
    assert_eq!(run_query(&session, Uuid::new_v4(), "SELECT 'hi'", None, false).await.unwrap().rows[0][0], json!("hi"));
    // The loopback port is private to this session: it closes with it.
    drop(session);
    mgr.close(id).await;

    // Certificate checked against the name the SSH host would use, not the loopback address.
    if let Ok(cert) = std::env::var("SSHVAULT_TEST_PG_CERT") {
        std::env::set_var("SSL_CERT_FILE", cert);
        let verified = ConnectSpec { tls: TlsMode::VerifyFull, host: "localhost".into(), ..base.clone() };
        let (id, _) = mgr.open(verified, Some(via.clone())).await.unwrap();
        mgr.close(id).await;
        std::env::remove_var("SSL_CERT_FILE");
    }

    let dead = ConnectSpec { port: 1, ..base };
    let err = mgr.open(dead, Some(via)).await.expect_err("must fail").to_string();
    assert!(!err.is_empty());
}

/// A connection without TLS, for a throwaway local server (`SSHVAULT_TEST_PG_PLAIN=1`).
async fn engine_plain() -> Option<Engine> {
    std::env::var("SSHVAULT_TEST_PG_PLAIN").ok()?;
    Some(Engine::connect(&spec(TlsMode::Disable)?).await.expect("connect"))
}

#[tokio::test]
#[ignore = "needs SSHVAULT_TEST_PG and SSHVAULT_TEST_PG_PLAIN"]
async fn live_pg_table_meta_reads_indexes_constraints_and_ddl() {
    let Some(e) = engine_plain().await else { return };
    let m = e.table_meta("public", "items").await.unwrap();
    assert!(m.supported);
    assert_eq!(m.kind.as_deref(), Some("table"));
    assert!(m.indexes.iter().any(|i| i.primary && i.columns == ["id"]));
    let by_name = m.indexes.iter().find(|i| i.name == "items_name_idx").expect("the name index");
    assert_eq!(by_name.columns, ["name", "price"]);
    assert!(!by_name.unique);
    assert_eq!(by_name.method.as_deref(), Some("btree"));
    let fk = m.constraints.iter().find(|c| c.kind == meta::ConstraintKind::ForeignKey).expect("a foreign key");
    let r = fk.references.as_ref().unwrap();
    assert_eq!((r.schema.as_str(), r.table.as_str()), ("public", "customers"));
    assert_eq!(fk.columns, ["customer_id"]);
    assert_eq!(r.on_delete.as_deref(), Some("SET NULL"));
    assert!(m.constraints.iter().any(|c| c.kind == meta::ConstraintKind::Check && c.definition.as_deref().is_some_and(|d| d.contains("price"))));
    let ddl = m.ddl.unwrap();
    assert!(ddl.reconstructed && ddl.text.starts_with("CREATE TABLE \"public\".\"items\""));
    assert!(ddl.text.contains("CREATE INDEX items_name_idx"));
    // The estimate is the server's, and the table was analysed.
    assert!(m.approx_rows.unwrap() > 1000 && m.size_bytes.unwrap() > 0);

    let c = e.table_meta("public", "customers").await.unwrap();
    let back = c.referenced_by.iter().find(|r| r.table == "items").expect("items points at customers");
    assert_eq!((back.columns.as_slice(), back.referenced_columns.as_slice()), (&["customer_id".to_string()][..], &["id".to_string()][..]));
    assert!(c.column_notes.is_empty());

    let v = e.table_meta("public", "v_items").await.unwrap();
    assert_eq!(v.kind.as_deref(), Some("view"));
    assert!(v.ddl.unwrap().text.starts_with("CREATE VIEW"));

    let missing = e.table_meta("public", "nope").await;
    assert!(matches!(missing, Err(DbError::Invalid(_))));
}

#[tokio::test]
#[ignore = "needs SSHVAULT_TEST_PG and SSHVAULT_TEST_PG_PLAIN"]
async fn live_pg_tree_has_routines_sequences_and_triggers() {
    let Some(e) = engine_plain().await else { return };
    let nodes = e.children(&["public".into()]).await.unwrap();
    assert!(nodes.iter().any(|n| n.kind == NodeKind::Function && n.name == "add_one" && n.detail.as_deref() == Some("(i integer)")));
    assert!(nodes.iter().any(|n| n.kind == NodeKind::Procedure && n.name == "noop"));
    assert!(nodes.iter().any(|n| n.kind == NodeKind::Sequence && n.name == "myseq"));
    let t = e.children(&["public".into(), "items".into()]).await.unwrap();
    assert!(t.iter().any(|n| n.kind == NodeKind::Trigger && n.name == "items_trg"));
}

#[tokio::test]
#[ignore = "needs SSHVAULT_TEST_PG and SSHVAULT_TEST_PG_PLAIN"]
async fn live_pg_search_finds_objects_across_schemas_and_treats_wildcards_literally() {
    let Some(e) = engine_plain().await else { return };
    let hits = e.search("ITEM", 50).await.unwrap();
    assert!(hits.iter().any(|h| h.name == "items" && h.kind == NodeKind::Table && h.parent == ["public"]));
    assert!(hits.iter().any(|h| h.name == "v_items" && h.kind == NodeKind::View));
    let pairs = e.search("pair", 50).await.unwrap();
    assert!(pairs.iter().any(|h| h.parent == ["warehouse"] && h.name == "pairs"));
    assert!(e.search("add_one", 50).await.unwrap().iter().any(|h| h.kind == NodeKind::Function));
    // An underscore or a percent sign is a character, not a wildcard.
    assert!(e.search("%", 50).await.unwrap().is_empty());
    assert!(e.search("ite_s", 50).await.unwrap().is_empty());
    assert_eq!(e.search("item", 2).await.unwrap().len(), 2);
}

fn cell(column: &str, value: Option<&str>) -> CellEdit {
    CellEdit { column: column.into(), value: value.map(str::to_string) }
}

#[tokio::test]
#[ignore = "needs SSHVAULT_TEST_PG and SSHVAULT_TEST_PG_PLAIN"]
async fn live_pg_a_reviewed_set_lands_together_or_not_at_all() {
    let Some(e) = engine_plain().await else { return };
    q(&e, "DROP TABLE IF EXISTS public.scratch").await;
    q(&e, "CREATE TABLE public.scratch(id int PRIMARY KEY, label text, n numeric)").await;
    q(&e, "INSERT INTO public.scratch VALUES (1, 'one', 1), (2, 'two', 2)").await;
    let ins = Change::Insert { database: "public".into(), table: "scratch".into(), values: vec![cell("id", Some("3")), cell("label", Some("it's; DROP TABLE x")), cell("n", None)] };
    let upd = Change::Update(RowEdit { database: "public".into(), table: "scratch".into(), key: vec![cell("id", Some("1"))], changes: vec![cell("label", Some("uno"))] });
    let del = Change::Delete { database: "public".into(), table: "scratch".into(), key: vec![cell("id", Some("2"))] };

    let preview = e.preview_changes(&[ins.clone(), upd.clone(), del.clone()]).unwrap();
    assert_eq!(preview[0], "INSERT INTO \"public\".\"scratch\" (\"id\", \"label\", \"n\") VALUES ('3', 'it''s; DROP TABLE x', NULL);");
    assert_eq!(preview[2], "DELETE FROM \"public\".\"scratch\" WHERE \"id\" = '2';");

    let touched = e.apply_changes(&[ins, upd.clone(), del.clone()]).await.unwrap();
    assert_eq!(touched, [1, 1, 1]);
    let r = q(&e, "SELECT id, label, n FROM public.scratch ORDER BY id").await;
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0][1], json!("uno"));
    assert_eq!(r.rows[1][1], json!("it's; DROP TABLE x"));
    assert_eq!(r.rows[1][2], Value::Null);

    // The second change matches no row (id 2 is gone), so the first, which is fine, is rolled back too.
    let again = Change::Update(RowEdit { database: "public".into(), table: "scratch".into(), key: vec![cell("id", Some("3"))], changes: vec![cell("label", Some("changed"))] });
    let err = e.apply_changes(&[again, del]).await.unwrap_err().to_string();
    assert!(err.contains("change 2 of 2") && err.contains("Nothing was saved"), "{err}");
    let r = q(&e, "SELECT label FROM public.scratch WHERE id = 3").await;
    assert_eq!(r.rows[0][0], json!("it's; DROP TABLE x"));

    // A constraint failure names the change and also rolls everything back.
    let dup = Change::Insert { database: "public".into(), table: "scratch".into(), values: vec![cell("id", Some("1"))] };
    let fine = Change::Insert { database: "public".into(), table: "scratch".into(), values: vec![cell("id", Some("9"))] };
    let err = e.apply_changes(&[fine, dup]).await.unwrap_err().to_string();
    assert!(err.starts_with("change 2 of 2") && err.contains("duplicate key"), "{err}");
    assert_eq!(q(&e, "SELECT count(*) FROM public.scratch WHERE id = 9").await.rows[0][0], json!(0));

    // Rows with no key can't be deleted or edited, and an empty set is refused.
    let keyless = Change::Delete { database: "public".into(), table: "nokey".into(), key: vec![] };
    assert!(matches!(e.apply_changes(&[keyless]).await, Err(DbError::Invalid(_))));
    assert!(matches!(e.apply_changes(&[]).await, Err(DbError::Invalid(_))));
    q(&e, "DROP TABLE public.scratch").await;
}

#[tokio::test]
#[ignore = "needs SSHVAULT_TEST_PG and SSHVAULT_TEST_PG_PLAIN"]
async fn live_pg_a_read_only_session_refuses_a_reviewed_set() {
    let Some(mut sp) = spec(TlsMode::Disable) else { return };
    if std::env::var("SSHVAULT_TEST_PG_PLAIN").is_err() {
        return;
    }
    sp.options.insert("read_only".into(), "true".into());
    let e = Engine::connect(&sp).await.unwrap();
    let ch = Change::Insert { database: "public".into(), table: "nokey".into(), values: vec![cell("a", Some("1"))] };
    let err = e.apply_changes(&[ch]).await.unwrap_err().to_string();
    assert!(err.contains("read-only"), "{err}");
}
