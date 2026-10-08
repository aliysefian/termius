use super::*;
use std::collections::BTreeMap;
use std::process::{Child, Command, Stdio};

fn args(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|p| p.as_bytes().to_vec()).collect()
}

#[test]
fn a_command_is_sent_as_an_array_of_bulk_strings() {
    assert_eq!(encode(&args(&["SET", "k", "v w"])), b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$3\r\nv w\r\n");
    assert_eq!(encode(&[]), b"*0\r\n");
    assert_eq!(encode(&[b"".to_vec()]), b"*1\r\n$0\r\n\r\n");
}

async fn parse(bytes: &[u8]) -> DbResult<Resp> {
    let mut r = BufReader::new(bytes);
    read_reply(&mut r, 0).await
}

#[tokio::test]
async fn every_kind_of_reply_is_read() {
    assert_eq!(parse(b"+OK\r\n").await.unwrap(), Resp::Simple("OK".into()));
    assert_eq!(parse(b"-ERR boom\r\n").await.unwrap(), Resp::Error("ERR boom".into()));
    assert_eq!(parse(b":-42\r\n").await.unwrap(), Resp::Int(-42));
    assert_eq!(parse(b"$5\r\nhello\r\n").await.unwrap(), Resp::Bulk(Some(b"hello".to_vec())));
    assert_eq!(parse(b"$0\r\n\r\n").await.unwrap(), Resp::Bulk(Some(vec![])));
    assert_eq!(parse(b"$-1\r\n").await.unwrap(), Resp::Bulk(None));
    assert_eq!(parse(b"*-1\r\n").await.unwrap(), Resp::Array(None));
    assert_eq!(parse(b"$4\r\na\r\nb\r\n").await.unwrap(), Resp::Bulk(Some(b"a\r\nb".to_vec())), "a value may hold a line break");
    assert_eq!(
        parse(b"*3\r\n:1\r\n$1\r\nx\r\n*2\r\n+a\r\n$-1\r\n").await.unwrap(),
        Resp::Array(Some(vec![Resp::Int(1), Resp::Bulk(Some(b"x".to_vec())), Resp::Array(Some(vec![Resp::Simple("a".into()), Resp::Bulk(None)]))]))
    );
}

#[tokio::test]
async fn nonsense_and_hostile_replies_are_refused_not_followed() {
    assert!(matches!(parse(b"<html>\r\n").await, Err(DbError::Server(_))));
    assert!(matches!(parse(b"").await, Err(DbError::Server(_))));
    assert!(matches!(parse(b":abc\r\n").await, Err(DbError::Server(_))));
    assert!(matches!(parse(b"$abc\r\n").await, Err(DbError::Server(_))));
    assert!(matches!(parse(b"$5\r\nab").await, Err(DbError::Server(_))), "a value cut short");
    assert!(matches!(parse(b"$999999999999\r\n").await, Err(DbError::Server(m)) if m.contains("larger than")));
    assert!(matches!(parse(b"*999999999\r\n").await, Err(DbError::Server(_))));
    let deep = "*1\r\n".repeat(40);
    assert!(matches!(parse(deep.as_bytes()).await, Err(DbError::Server(m)) if m.contains("deeply")));
}

fn words(line: &str) -> Vec<String> {
    split_command(line).unwrap().iter().map(|w| String::from_utf8_lossy(w).into_owned()).collect()
}

#[test]
fn a_line_is_split_like_redis_cli_does() {
    assert_eq!(words("GET  key"), ["GET", "key"]);
    assert_eq!(words(r#"SET "my key" "a \"quoted\" \\ value\n""#), ["SET", "my key", "a \"quoted\" \\ value\n"]);
    assert_eq!(words(r"SET 'it\'s' 'a b'"), ["SET", "it's", "a b"]);
    assert_eq!(words(r#"SET k "\x41\x42""#), ["SET", "k", "AB"]);
    assert_eq!(words(r#"SET k """#), ["SET", "k", ""]);
    assert_eq!(words(""), Vec::<String>::new());
    assert_eq!(split_command(r#"SET "caf\xc3\xa9" x"#).unwrap()[1], "café".as_bytes());
    for bad in [r#"GET "abc"#, "GET 'abc", r#"GET "a"b"#, "GET 'a'b"] {
        assert!(split_command(bad).is_err(), "{bad}");
    }
}

#[test]
fn quoting_round_trips() {
    for s in ["plain", "with space", "q\"uote", "back\\slash", "new\nline", "tab\t", "", "ünï", "a'b", "\x01ctl"] {
        let line = format!("GET {}", quote_arg(s));
        assert_eq!(split_command(&line).unwrap()[1], s.as_bytes(), "{s:?} -> {line}");
    }
    assert_eq!(quote_arg("user:42:name"), "user:42:name");
}

#[test]
fn statements_may_name_a_database_and_skip_comments() {
    let c = parse_statements("# a note\n@3 GET k\n\nPING").unwrap();
    assert_eq!((c.len(), c[0].db, c[1].db), (2, Some(3), None));
    assert_eq!(c[0].args, args(&["GET", "k"]));
    assert!(parse_statements("").is_err());
    assert!(parse_statements("# only a comment").is_err());
    assert!(parse_statements("@ GET k").is_err());
    assert!(parse_statements("@3").is_err());
    assert!(parse_statements("GET \"x").is_err());
}

#[test]
fn what_is_destructive_or_unsupported_is_named() {
    for cmd in ["FLUSHALL", "flushdb", "DEL a b", "UNLINK a", "SHUTDOWN NOSAVE", "CONFIG SET maxmemory 1", "config rewrite", "ACL SETUSER x on", "KEYS *", "EVAL \"return 1\" 0", "REPLICAOF host 6379", "SCRIPT FLUSH", "CLIENT KILL ID 3", "MODULE LOAD x.so", "DEBUG SLEEP 1", "RESTORE k 0 x"] {
        let c = parse_statements(cmd).unwrap();
        assert!(destructive(&c[0].args).is_some(), "{cmd}");
    }
    for cmd in ["GET k", "SET k v", "HGETALL h", "SCAN 0", "INFO", "CONFIG GET *", "ACL LIST", "CLIENT LIST", "TTL k", "TYPE k", "SCRIPT EXISTS x"] {
        let c = parse_statements(cmd).unwrap();
        assert!(destructive(&c[0].args).is_none(), "{cmd}");
    }
    for cmd in ["SUBSCRIBE c", "MONITOR", "MULTI", "SELECT 2", "AUTH x", "QUIT"] {
        let c = parse_statements(cmd).unwrap();
        assert!(unsupported(&c[0].args).is_some(), "{cmd}");
    }
    assert!(unsupported(&parse_statements("GET k").unwrap()[0].args).is_none());
}

#[test]
fn bytes_that_are_not_text_are_shown_escaped() {
    assert_eq!(show(b"plain"), "plain");
    assert_eq!(show("é".as_bytes()), "é");
    assert_eq!(show(&[0x61, 0xff, 0x00, b'\\']), "a\\xff\\x00\\\\");
}

fn bulk(s: &str) -> Resp {
    Resp::Bulk(Some(s.as_bytes().to_vec()))
}

#[test]
fn replies_become_grids_the_way_the_command_means_them() {
    let ok = lay_out(&args(&["SET", "k", "v"]), &Resp::Simple("OK".into()), 10);
    assert_eq!(ok.rows[0][0], json!("OK"));
    let n = lay_out(&args(&["INCR", "c"]), &Resp::Int(5), 10);
    assert_eq!((n.columns[0].name.as_str(), &n.rows[0][0]), ("result", &json!(5)));
    let nil = lay_out(&args(&["GET", "nope"]), &Resp::Bulk(None), 10);
    assert_eq!(nil.rows[0][0], Value::Null);

    let h = lay_out(&args(&["HGETALL", "h"]), &Resp::Array(Some(vec![bulk("a"), bulk("1"), bulk("b"), bulk("2")])), 10);
    assert_eq!(h.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["field", "value"]);
    assert_eq!(h.rows.len(), 2);
    let z = lay_out(&args(&["ZRANGE", "z", "0", "-1", "WITHSCORES"]), &Resp::Array(Some(vec![bulk("m"), bulk("1.5")])), 10);
    assert_eq!(z.columns[1].name, "score");
    assert_eq!(z.columns[1].kind, ColumnKind::Number);
    let plain = lay_out(&args(&["ZRANGE", "z", "0", "-1"]), &Resp::Array(Some(vec![bulk("m")])), 10);
    assert_eq!(plain.columns[0].name, "#");
    let list = lay_out(&args(&["LRANGE", "l", "0", "-1"]), &Resp::Array(Some(vec![bulk("x"), bulk("y"), bulk("z")])), 2);
    assert_eq!((list.rows.len(), list.truncated, &list.rows[1]), (2, true, &vec![json!(2), json!("y")]));
    let nested = lay_out(&args(&["SCAN", "0"]), &Resp::Array(Some(vec![bulk("0"), Resp::Array(Some(vec![bulk("k1")]))])), 10);
    assert_eq!(nested.columns[1].kind, ColumnKind::Json);
    assert_eq!(nested.rows[1][1], json!("[\"k1\"]"));
    let info = lay_out(&args(&["INFO"]), &bulk("# Server\r\nredis_version:6.2.14\r\n\r\nrole:master\r\n"), 100);
    assert_eq!(info.columns[0].name, "line");
    assert_eq!(info.rows.len(), 3);
    let xr = lay_out(&args(&["XRANGE", "s", "-", "+"]), &Resp::Array(Some(vec![Resp::Array(Some(vec![bulk("1-0"), Resp::Array(Some(vec![bulk("f"), bulk("v")]))]))])), 10);
    assert_eq!((xr.columns[0].name.as_str(), &xr.rows[0][1]), ("id", &json!("[\"f\",\"v\"]")));
    let empty = lay_out(&args(&["SMEMBERS", "s"]), &Resp::Array(Some(vec![])), 10);
    assert!(empty.rows.is_empty() && !empty.truncated);
}

#[test]
fn keys_are_grouped_by_the_part_before_a_colon() {
    let keys: Vec<String> = ["user:1:name", "user:1:mail", "user:2:name", "user:last", "session:abc", "plain", "trail:"].iter().map(|s| s.to_string()).collect();
    let (folders, leaves) = group_keys("", &keys);
    assert_eq!(folders, [("session:".to_string(), 1), ("user:".to_string(), 4)]);
    assert_eq!(leaves, ["plain", "trail:"], "a trailing colon is part of the name, not a folder");
    let (folders, leaves) = group_keys("user:", &keys.iter().filter(|k| k.starts_with("user:")).cloned().collect::<Vec<_>>());
    assert_eq!(folders, [("user:1:".to_string(), 2), ("user:2:".to_string(), 1)]);
    assert_eq!(leaves, ["user:last"]);
    assert_eq!(prefix_pattern("a*b?[c]\\"), "a\\*b\\?\\[c\\]\\\\*");
    assert_eq!(prefix_pattern(""), "*");
}

#[test]
fn times_to_live_read_naturally() {
    assert_eq!(ttl_text(-1), None);
    assert_eq!(ttl_text(-2), None);
    assert_eq!(ttl_text(250).as_deref(), Some("250 ms"));
    assert_eq!(ttl_text(45_000).as_deref(), Some("45 s"));
    assert_eq!(ttl_text(600_000).as_deref(), Some("10 min"));
    assert_eq!(ttl_text(7_200_000).as_deref(), Some("2 h"));
    assert_eq!(ttl_text(3 * 86_400_000).as_deref(), Some("3 days"));
}

fn edit(table: &str, key: (&str, &str), change: (&str, Option<&str>)) -> RowEdit {
    RowEdit {
        database: "db0".into(),
        table: table.into(),
        key: vec![super::super::CellEdit { column: key.0.into(), value: Some(key.1.into()) }],
        changes: vec![super::super::CellEdit { column: change.0.into(), value: change.1.map(str::to_string) }],
    }
}

#[test]
fn an_edit_is_the_command_that_fits_the_type_and_nothing_else() {
    assert_eq!(edit_command("string", "k", &edit("k", ("key", "k"), ("value", Some("v")))).unwrap(), ["SET", "k", "v", "KEEPTTL"]);
    assert_eq!(edit_command("hash", "h", &edit("h", ("field", "f"), ("value", Some("v")))).unwrap(), ["HSET", "h", "f", "v"]);
    assert_eq!(edit_command("list", "l", &edit("l", ("index", "2"), ("value", Some("v")))).unwrap(), ["LSET", "l", "2", "v"]);
    assert_eq!(edit_command("zset", "z", &edit("z", ("member", "m"), ("score", Some("2.5")))).unwrap(), ["ZADD", "z", "2.5", "m"]);
    for (ty, e) in [
        ("string", edit("k", ("key", "k"), ("value", None))),
        ("list", edit("l", ("index", "x"), ("value", Some("v")))),
        ("zset", edit("z", ("member", "m"), ("score", Some("high")))),
        ("zset", edit("z", ("member", "m"), ("member", Some("n")))),
        ("set", edit("s", ("member", "m"), ("member", Some("n")))),
        ("stream", edit("s", ("id", "1-0"), ("fields", Some("x")))),
        ("hash", edit("h", ("field", "f"), ("nope", Some("x")))),
    ] {
        assert!(matches!(edit_command(ty, "k", &e), Err(DbError::Invalid(_))), "{ty}");
    }
}

#[test]
fn the_preview_shows_the_command_with_the_console_quoting() {
    let spec = ConnectSpec { engine: "redis".into(), host: "h".into(), port: 1, user: String::new(), password: None, database: None, tls: crate::db::TlsMode::Disable, tunnel_port: None, options: BTreeMap::new() };
    let _ = spec;
    let c = preview_of(&edit("my hash", ("field", "f 1"), ("value", Some("a \"b\""))));
    assert_eq!(c, r#"HSET "my hash" "f 1" "a \"b\"""#);
}

/// The preview doesn't need a connection; build one only for the call.
fn preview_of(e: &RowEdit) -> String {
    let ty = match e.changes.first().map(|c| c.column.as_str()) {
        Some("score") => "zset",
        _ => match e.key.first().map(|k| k.column.as_str()) {
            Some("key") => "string",
            Some("index") => "list",
            _ => "hash",
        },
    };
    edit_command(ty, &e.table, e).unwrap().iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" ")
}

// -- against a real redis-server -----------------------------------------------------------------------

/// The server binary: `SSHVAULT_REDIS_SERVER`, or `redis-server` on the PATH. Tests that need it are skipped when
/// there is none (the setup is `redis-server --port N --save "" --appendonly no`).
struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn server(extra: &[&str]) -> Option<Server> {
    let bin = std::env::var("SSHVAULT_REDIS_SERVER").unwrap_or_else(|_| "redis-server".into());
    // A port nobody holds when the server starts; try again if another test took it first.
    for _ in 0..5 {
        let port = std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
        let child = Command::new(&bin).args(["--port", &port.to_string(), "--bind", "127.0.0.1", "--save", "", "--appendonly", "no"]).args(extra).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
        let mut s = Server { child, port };
        for _ in 0..100 {
            // Ready when it answers a PING (with PONG, or NOAUTH when a password is required), and is ours.
            if let Ok(mut t) = TcpStream::connect(("127.0.0.1", port)).await {
                let mut line = String::new();
                if tokio::io::AsyncWriteExt::write_all(&mut t, b"PING\r\n").await.is_ok() && BufReader::new(&mut t).read_line(&mut line).await.is_ok() && !line.is_empty() && s.child.try_wait().ok()?.is_none() {
                    return Some(s);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    None
}

fn spec_for(port: u16) -> ConnectSpec {
    ConnectSpec { engine: "redis".into(), host: "127.0.0.1".into(), port, user: String::new(), password: None, database: None, tls: crate::db::TlsMode::Disable, tunnel_port: None, options: BTreeMap::new() }
}

async fn run(c: &RedisConn, text: &str) -> DbResult<QueryResult> {
    c.query(Uuid::new_v4(), text, 1000).await
}

macro_rules! live {
    ($extra:expr) => {
        match server($extra).await {
            Some(s) => s,
            None => {
                eprintln!("skipping: no redis-server on this machine");
                return;
            }
        }
    };
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_commands_keys_and_types_lay_out_as_grids() {
    let s = live!(&[]);
    let c = RedisConn::connect(&spec_for(s.port)).await.unwrap();
    assert!(c.server_version().await.unwrap().starts_with("Redis "));
    assert_eq!(run(&c, "SET greeting \"hello world\"").await.unwrap().rows[0][0], json!("OK"));
    assert_eq!(run(&c, "GET greeting").await.unwrap().rows[0][0], json!("hello world"));
    assert_eq!(run(&c, "GET nothing").await.unwrap().rows[0][0], Value::Null);
    assert_eq!(run(&c, "INCRBY counter 5").await.unwrap().rows[0][0], json!(5));
    run(&c, "HSET user:1 name Ada age 36").await.unwrap();
    run(&c, "HSET user:2 name Alan age 41").await.unwrap();
    run(&c, "RPUSH queue a b c").await.unwrap();
    run(&c, "SADD tags x y").await.unwrap();
    run(&c, "ZADD board 10 ann 20 bob").await.unwrap();
    run(&c, "XADD events * level info msg started").await.unwrap();

    let h = run(&c, "HGETALL user:1").await.unwrap();
    assert_eq!(h.columns[0].name, "field");
    assert_eq!(h.rows.len(), 2);
    let z = run(&c, "ZRANGE board 0 -1 WITHSCORES").await.unwrap();
    assert_eq!(z.rows[1], vec![json!("bob"), json!(20)]);

    // VIEW reads a key whatever its type is.
    assert_eq!(run(&c, "VIEW greeting").await.unwrap().rows, vec![vec![json!("greeting"), json!("hello world")]]);
    assert_eq!(run(&c, "VIEW user:1").await.unwrap().rows.len(), 2);
    let list = run(&c, "VIEW queue").await.unwrap();
    assert_eq!(list.rows, vec![vec![json!(0), json!("a")], vec![json!(1), json!("b")], vec![json!(2), json!("c")]]);
    assert_eq!(run(&c, "VIEW tags").await.unwrap().rows.len(), 2);
    assert_eq!(run(&c, "VIEW board").await.unwrap().columns[1].name, "score");
    assert_eq!(run(&c, "VIEW events").await.unwrap().columns[0].name, "id");
    assert!(matches!(run(&c, "VIEW missing").await, Err(DbError::Server(m)) if m.contains("no key named")));
    let capped = c.query(Uuid::new_v4(), "VIEW queue", 2).await.unwrap();
    assert_eq!((capped.rows.len(), capped.truncated), (2, true));

    let info = run(&c, "INFO server").await.unwrap();
    assert_eq!(info.columns[0].name, "line");
    // A server's own error is shown as the server worded it.
    assert!(matches!(run(&c, "NOSUCHCOMMAND").await, Err(DbError::Server(m)) if m.to_lowercase().contains("unknown")));
    assert!(matches!(run(&c, "GET a b c").await, Err(DbError::Server(_))));
    // Commands the console can't follow are refused before they are sent.
    assert!(matches!(run(&c, "SUBSCRIBE news").await, Err(DbError::Invalid(_))));
    assert!(matches!(run(&c, "GET \"x").await, Err(DbError::Invalid(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_databases_are_chosen_per_command_and_never_leak_between_runs() {
    let s = live!(&[]);
    let c = RedisConn::connect(&spec_for(s.port)).await.unwrap();
    run(&c, "SET only-in-zero 1").await.unwrap();
    run(&c, "@5 SET only-in-five 2").await.unwrap();
    // The run after a @5 command is back in the connection's own database, even on the same socket.
    assert_eq!(run(&c, "GET only-in-five").await.unwrap().rows[0][0], Value::Null);
    assert_eq!(run(&c, "@5 GET only-in-five").await.unwrap().rows[0][0], json!("2"));
    assert_eq!(run(&c, "@5 VIEW only-in-five").await.unwrap().rows[0][1], json!("2"));
    let root = c.children(&[]).await.unwrap();
    let names: Vec<_> = root.iter().map(|n| (n.name.as_str(), n.detail.as_deref())).collect();
    assert_eq!(names, [("db0", Some("1 keys")), ("db5", Some("1 keys"))]);
    assert!(matches!(run(&c, "@99999 GET k").await, Err(DbError::Invalid(_))));
    assert!(matches!(run(&c, "@1000 GET k").await, Err(DbError::Server(_))), "a database the server doesn't have");

    let mut other = spec_for(s.port);
    other.options.insert("db".into(), "5".into());
    let c5 = RedisConn::connect(&other).await.unwrap();
    assert_eq!(run(&c5, "GET only-in-five").await.unwrap().rows[0][0], json!("2"), "the connection's own database");
    other.options.insert("db".into(), "x".into());
    assert!(matches!(RedisConn::connect(&other).await, Err(DbError::Invalid(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_the_tree_scans_groups_by_colon_and_shows_types_and_expiry() {
    let s = live!(&[]);
    let c = RedisConn::connect(&spec_for(s.port)).await.unwrap();
    for cmd in ["SET user:1:name Ada", "SET user:1:mail a@x", "SET user:2:name Alan", "SET user:last 2", "SET plain 1 EX 3600", "HSET session:abc a 1", "SET we*ird:k 1"] {
        run(&c, cmd).await.unwrap();
    }
    let top = c.children(&["db0".into()]).await.unwrap();
    let folders: Vec<_> = top.iter().filter(|n| n.kind == NodeKind::Folder).map(|n| (n.name.as_str(), n.detail.as_deref())).collect();
    assert_eq!(folders, [("session:", Some("1")), ("user:", Some("4")), ("we*ird:", Some("1"))]);
    let plain = top.iter().find(|n| n.name == "plain").unwrap();
    assert_eq!(plain.kind, NodeKind::Table);
    assert!(plain.detail.as_deref().unwrap().starts_with("string · expires in 59 min") || plain.detail.as_deref().unwrap().starts_with("string · expires in 1 h"), "{:?}", plain.detail);
    let user = c.children(&["db0".into(), "user:".into()]).await.unwrap();
    assert_eq!(user.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["user:1:", "user:2:", "user:last"]);
    let weird = c.children(&["db0".into(), "we*ird:".into()]).await.unwrap();
    assert_eq!(weird[0].name, "we*ird:k", "a glob character in a prefix is not a wildcard");
    let session = c.children(&["db0".into(), "session:".into()]).await.unwrap();
    assert_eq!(session[0].detail.as_deref(), Some("hash"));
    assert!(matches!(c.children(&["nope".into()]).await, Err(DbError::Invalid(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_a_big_keyspace_is_cut_and_says_so() {
    let s = live!(&[]);
    let c = RedisConn::connect(&spec_for(s.port)).await.unwrap();
    let eval = format!("EVAL \"for i=1,{} do redis.call('SET','bulk'..i,i) end return 1\" 0", MAX_TREE_KEYS + 500);
    // The console asks before running a script; the engine itself just runs what it is given.
    run(&c, &eval).await.unwrap();
    let nodes = c.children(&["db0".into()]).await.unwrap();
    assert_eq!(nodes.len(), MAX_TREE_KEYS + 1, "the keys up to the cap and a note");
    assert!(nodes.last().unwrap().name.contains("showing the first"));
    assert!(nodes[..MAX_TREE_KEYS].iter().filter(|n| n.detail.is_some()).count() <= DETAIL_KEYS, "only some keys get a type read");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_inline_edits_change_the_value_and_keep_the_expiry() {
    let s = live!(&[]);
    let c = RedisConn::connect(&spec_for(s.port)).await.unwrap();
    run(&c, "SET k old EX 1000").await.unwrap();
    run(&c, "HSET h f 1").await.unwrap();
    run(&c, "RPUSH l a b").await.unwrap();
    run(&c, "ZADD z 1 m").await.unwrap();

    let info = c.table_info("db0", "h").await.unwrap();
    assert_eq!((info.primary_key.as_slice(), info.columns.len()), (&["field".to_string()][..], 2));
    assert_eq!(c.table_info("db0", "tags-none").await.unwrap().columns.len(), 0);

    assert_eq!(c.apply_update(&edit("k", ("key", "k"), ("value", Some("new")))).await.unwrap(), 1);
    assert_eq!(run(&c, "GET k").await.unwrap().rows[0][0], json!("new"));
    let ttl = run(&c, "TTL k").await.unwrap().rows[0][0].as_i64().unwrap();
    assert!((900..=1000).contains(&ttl), "KEEPTTL kept it: {ttl}");
    assert_eq!(c.apply_update(&edit("h", ("field", "f"), ("value", Some("2")))).await.unwrap(), 1);
    assert_eq!(run(&c, "HGET h f").await.unwrap().rows[0][0], json!("2"));
    assert_eq!(c.apply_update(&edit("l", ("index", "1"), ("value", Some("B")))).await.unwrap(), 1);
    assert_eq!(run(&c, "LINDEX l 1").await.unwrap().rows[0][0], json!("B"));
    assert_eq!(c.apply_update(&edit("z", ("member", "m"), ("score", Some("7")))).await.unwrap(), 1);
    assert_eq!(run(&c, "ZSCORE z m").await.unwrap().rows[0][0], json!("7"));
    // A key that is gone changes nothing; a bad list index is the server's own error.
    assert_eq!(c.apply_update(&edit("gone", ("key", "gone"), ("value", Some("x")))).await.unwrap(), 0);
    assert!(matches!(c.apply_update(&edit("l", ("index", "9"), ("value", Some("x")))).await, Err(DbError::Server(_))));
    // A value that looks like a command stays a value.
    c.apply_update(&edit("k", ("key", "k"), ("value", Some("FLUSHALL")))).await.unwrap();
    assert_eq!(run(&c, "GET k").await.unwrap().rows[0][0], json!("FLUSHALL"));
    assert_eq!(run(&c, "DBSIZE").await.unwrap().rows[0][0], json!(4));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_a_blocking_command_can_be_cancelled_and_the_connection_still_works() {
    let s = live!(&[]);
    let c = std::sync::Arc::new(RedisConn::connect(&spec_for(s.port)).await.unwrap());
    let qid = Uuid::new_v4();
    let running = {
        let c = c.clone();
        tokio::spawn(async move { c.query(qid, "BLPOP never 30", 10).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let started = Instant::now();
    c.cancel(qid).await.unwrap();
    assert!(matches!(running.await.unwrap(), Err(DbError::Cancelled)));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert_eq!(run(&c, "PING").await.unwrap().rows[0][0], json!("PONG"));
    c.cancel(Uuid::new_v4()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_a_password_an_acl_user_and_the_reasons_for_refusal() {
    let s = live!(&["--requirepass", "s3cret"]);
    // No password: the server's NOAUTH is explained.
    match RedisConn::connect(&spec_for(s.port)).await {
        Err(DbError::Server(m)) => assert!(m.contains("needs a password"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("should have been refused"),
    }
    let mut ok = spec_for(s.port);
    ok.password = Some("s3cret".into());
    let c = RedisConn::connect(&ok).await.unwrap();
    // An ACL user, made with a command.
    run(&c, "ACL SETUSER reader on >readpw ~* +get +ping +info +scan +type +pttl +hscan +sscan").await.unwrap();
    run(&c, "SET k v").await.unwrap();
    let mut user = spec_for(s.port);
    user.user = "reader".into();
    user.password = Some("readpw".into());
    let r = RedisConn::connect(&user).await.unwrap();
    assert_eq!(run(&r, "GET k").await.unwrap().rows[0][0], json!("v"));
    assert!(matches!(run(&r, "SET k x").await, Err(DbError::Server(m)) if m.to_uppercase().contains("NOPERM")));
    user.password = Some("wrong".into());
    assert!(matches!(RedisConn::connect(&user).await, Err(DbError::Server(m)) if m.contains("user name or password")));
    // A user name with no password is refused before anything is sent.
    let mut half = spec_for(s.port);
    half.user = "reader".into();
    assert!(matches!(RedisConn::connect(&half).await, Err(DbError::Invalid(_))));
}

#[tokio::test]
async fn something_that_is_not_redis_or_not_there_is_explained() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            // Read the client's request first and keep the socket open afterwards: closing with unread
            // data resets the connection on Windows, and the client would see that instead of the reply.
            let mut buf = [0u8; 256];
            let _ = tokio::io::AsyncReadExt::read(&mut s, &mut buf).await;
            let _ = tokio::io::AsyncWriteExt::write_all(&mut s, b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                drop(s);
            });
        }
    });
    assert!(matches!(RedisConn::connect(&spec_for(port)).await, Err(DbError::Server(m)) if m.contains("isn't a Redis")));
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(matches!(RedisConn::connect(&spec_for(closed)).await, Err(DbError::Connect { .. })));
}
