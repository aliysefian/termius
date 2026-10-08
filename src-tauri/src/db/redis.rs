//! Redis, over its own protocol (RESP2) on a socket we open: plain, or TLS, with a password or an ACL user.
//!
//! Redis has no rows, so the Databases view shows it like this: the tree is the server's databases and, inside one,
//! its keys grouped by the part before a colon (found with `SCAN`, never `KEYS *`); opening a key runs the console
//! command `VIEW key`, which reads the key whatever its type is and lays it out as a grid; the console runs any
//! other command and lays the reply out the same way. A command may start with `@N` to run in database N.
//!
//! Every statement uses a connection of its own (kept for the next one), so cancelling one is closing its socket,
//! and a `SELECT` by one tab never changes the database of another.

use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use uuid::Uuid;

use super::{
    cut_text, text_table, tlsconf, Backend, Column, ColumnInfo, ColumnKind, ConnectSpec, DbError, DbResult, NodeKind, QueryResult, RowEdit, TableInfo,
    TreeNode, CONNECT_TIMEOUT, MAX_CELL_BYTES,
};

/// Keys a tree level reads before it stops and says so.
const MAX_TREE_KEYS: usize = 2000;
/// Leaf keys that get a type and a time to live in the tree.
const DETAIL_KEYS: usize = 200;
/// A reply nested deeper than this is refused.
const MAX_DEPTH: usize = 16;
/// A bulk string larger than this is refused (Redis allows 512 MB; a window can't show that).
const MAX_BULK: usize = 64 * 1024 * 1024;
const MAX_ELEMENTS: usize = 5_000_000;
const MAX_IDLE: usize = 4;

// -- the protocol ------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Resp {
    Simple(String),
    Error(String),
    Int(i64),
    Bulk(Option<Vec<u8>>),
    Array(Option<Vec<Resp>>),
}

/// A command as the bytes Redis wants: an array of bulk strings.
pub fn encode(args: &[Vec<u8>]) -> Vec<u8> {
    let mut out = format!("*{}\r\n", args.len()).into_bytes();
    for a in args {
        out.extend_from_slice(format!("${}\r\n", a.len()).as_bytes());
        out.extend_from_slice(a);
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn protocol(msg: &str) -> DbError {
    DbError::Server(format!("that isn't a Redis server's answer ({msg})"))
}

async fn read_line<R: AsyncBufReadExt + Unpin>(r: &mut R) -> DbResult<String> {
    let mut line = Vec::new();
    let n = r.read_until(b'\n', &mut line).await.map_err(|e| DbError::Server(e.to_string()))?;
    if n == 0 {
        return Err(DbError::Server("the server closed the connection".into()));
    }
    if line.len() > 64 * 1024 {
        return Err(protocol("a line is too long"));
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// One reply. Boxed because arrays nest.
pub fn read_reply<'a, R: AsyncBufReadExt + Unpin + Send + 'a>(r: &'a mut R, depth: usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = DbResult<Resp>> + Send + 'a>> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err(protocol("nested too deeply"));
        }
        let line = read_line(r).await?;
        let (tag, rest) = line.split_at(line.chars().next().map_or(0, char::len_utf8));
        match tag {
            "+" => Ok(Resp::Simple(rest.to_string())),
            "-" => Ok(Resp::Error(rest.to_string())),
            ":" => Ok(Resp::Int(rest.parse().map_err(|_| protocol("a number"))?)),
            "$" => {
                let n: i64 = rest.parse().map_err(|_| protocol("a length"))?;
                if n < 0 {
                    return Ok(Resp::Bulk(None));
                }
                if n as usize > MAX_BULK {
                    return Err(DbError::Server(format!("a value is larger than {} MB; read part of it with GETRANGE, LRANGE or SSCAN", MAX_BULK / (1024 * 1024))));
                }
                let mut buf = vec![0u8; n as usize + 2];
                tokio::io::AsyncReadExt::read_exact(r, &mut buf).await.map_err(|e| DbError::Server(e.to_string()))?;
                buf.truncate(n as usize);
                Ok(Resp::Bulk(Some(buf)))
            }
            "*" => {
                let n: i64 = rest.parse().map_err(|_| protocol("a count"))?;
                if n < 0 {
                    return Ok(Resp::Array(None));
                }
                if n as usize > MAX_ELEMENTS {
                    return Err(protocol("too many elements"));
                }
                let mut items = Vec::with_capacity((n as usize).min(1024));
                for _ in 0..n {
                    items.push(read_reply(r, depth + 1).await?);
                }
                Ok(Resp::Array(Some(items)))
            }
            _ => Err(protocol("an unknown reply type")),
        }
    })
}

// -- the command line --------------------------------------------------------------------------------

/// Split a line the way `redis-cli` does: spaces separate, `"double quotes"` take `\n \r \t \xNN \\ \"`
/// escapes, `'single quotes'` take `\'`.
pub fn split_command(line: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let mut cur: Vec<u8> = Vec::new();
        match b[i] {
            b'"' => {
                i += 1;
                loop {
                    let Some(&c) = b.get(i) else { return Err("a \" is never closed".into()) };
                    match c {
                        b'\\' => {
                            i += 1;
                            let Some(&e) = b.get(i) else { return Err("a \" is never closed".into()) };
                            match e {
                                b'n' => cur.push(b'\n'),
                                b'r' => cur.push(b'\r'),
                                b't' => cur.push(b'\t'),
                                b'b' => cur.push(8),
                                b'a' => cur.push(7),
                                b'x' if b.get(i + 1).is_some_and(u8::is_ascii_hexdigit) && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit) => {
                                    cur.push(u8::from_str_radix(&line[i + 1..i + 3], 16).unwrap_or(0));
                                    i += 2;
                                }
                                other => cur.push(other),
                            }
                            i += 1;
                        }
                        b'"' => {
                            i += 1;
                            if b.get(i).is_some_and(|n| !n.is_ascii_whitespace()) {
                                return Err("a closing \" must be followed by a space".into());
                            }
                            break;
                        }
                        other => {
                            cur.push(other);
                            i += 1;
                        }
                    }
                }
            }
            b'\'' => {
                i += 1;
                loop {
                    let Some(&c) = b.get(i) else { return Err("a ' is never closed".into()) };
                    match c {
                        b'\\' if b.get(i + 1) == Some(&b'\'') => {
                            cur.push(b'\'');
                            i += 2;
                        }
                        b'\'' => {
                            i += 1;
                            if b.get(i).is_some_and(|n| !n.is_ascii_whitespace()) {
                                return Err("a closing ' must be followed by a space".into());
                            }
                            break;
                        }
                        other => {
                            cur.push(other);
                            i += 1;
                        }
                    }
                }
            }
            _ => {
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    cur.push(b[i]);
                    i += 1;
                }
            }
        }
        out.push(cur);
    }
    Ok(out)
}

/// A string as one argument on the console: quoted when it needs to be.
pub fn quote_arg(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:/@+=%".contains(c)) {
        return s.to_string();
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A statement's lines as commands, each with the database it runs in (`@N` in front, else the connection's).
pub struct Command {
    pub db: Option<u32>,
    pub args: Vec<Vec<u8>>,
}

pub fn parse_statements(text: &str) -> Result<Vec<Command>, String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (db, rest) = match line.strip_prefix('@') {
            Some(r) => {
                let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
                if digits.is_empty() || digits.len() > 4 {
                    return Err("@ must be followed by a database number, like @3 GET key".into());
                }
                (Some(digits.parse::<u32>().unwrap_or(0)), r[digits.len()..].trim_start())
            }
            None => (None, line),
        };
        let args = split_command(rest)?;
        if args.is_empty() {
            return Err("there is a database number but no command after it".into());
        }
        out.push(Command { db, args });
    }
    if out.is_empty() {
        return Err("type a command to run".into());
    }
    Ok(out)
}

fn name_of(args: &[Vec<u8>]) -> String {
    args.first().map(|a| String::from_utf8_lossy(a).to_ascii_uppercase()).unwrap_or_default()
}

fn arg_upper(args: &[Vec<u8>], i: usize) -> String {
    args.get(i).map(|a| String::from_utf8_lossy(a).to_ascii_uppercase()).unwrap_or_default()
}

/// Why a command is destructive or doesn't belong in a console, as asked before it runs.
pub fn destructive(args: &[Vec<u8>]) -> Option<String> {
    let name = name_of(args);
    let sub = arg_upper(args, 1);
    let reason = match name.as_str() {
        "FLUSHALL" => "FLUSHALL deletes every key in every database.",
        "FLUSHDB" => "FLUSHDB deletes every key in this database.",
        "DEL" | "UNLINK" => "This deletes keys.",
        "SHUTDOWN" => "SHUTDOWN stops the server.",
        "DEBUG" => "DEBUG can crash or stall the server.",
        "SLAVEOF" | "REPLICAOF" => "This changes what the server replicates from.",
        "MIGRATE" => "MIGRATE moves keys to another server and can delete them here.",
        "RESTORE" => "RESTORE writes a key (and can replace one).",
        "KEYS" => "KEYS reads every key name and blocks the server while it does; use SCAN instead.",
        "CONFIG" if matches!(sub.as_str(), "SET" | "REWRITE" | "RESETSTAT") => "This changes the server's configuration.",
        "ACL" if matches!(sub.as_str(), "SETUSER" | "DELUSER" | "LOAD" | "SAVE") => "This changes who may do what on the server.",
        "SCRIPT" if sub == "FLUSH" => "This forgets every cached script.",
        "FUNCTION" if matches!(sub.as_str(), "FLUSH" | "DELETE") => "This deletes functions.",
        "CLIENT" if matches!(sub.as_str(), "KILL" | "PAUSE") => "This affects other clients of the server.",
        "MODULE" if matches!(sub.as_str(), "LOAD" | "UNLOAD") => "This changes the server's code.",
        "EVAL" | "EVALSHA" | "FCALL" => "A script can do anything, including deleting keys.",
        _ => return None,
    };
    Some(reason.to_string())
}

/// Commands that put the connection in a mode a one-shot console can't follow.
pub fn unsupported(args: &[Vec<u8>]) -> Option<&'static str> {
    match name_of(args).as_str() {
        "SUBSCRIBE" | "PSUBSCRIBE" | "SSUBSCRIBE" => Some("Subscriptions keep the connection open for messages, which a console can't show. Use PUBLISH to send, or redis-cli to listen."),
        "MONITOR" => Some("MONITOR streams every command and never ends; use redis-cli for it."),
        "SYNC" | "PSYNC" => Some("SYNC is for replicas."),
        "MULTI" | "EXEC" | "DISCARD" | "WATCH" => Some("Each command runs on its own, so a transaction can't be built across lines. Put the commands in one EVAL, or use redis-cli."),
        "QUIT" | "RESET" => Some("The console closes its own connections."),
        "HELLO" | "AUTH" => Some("The console signs in for you."),
        "SELECT" => Some("Start the command with @N (like @3 GET key) to run it in database N."),
        _ => None,
    }
}

// -- the connection ----------------------------------------------------------------------------------

trait Wire: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Wire for T {}

struct Socket {
    io: BufReader<Box<dyn Wire>>,
    /// The database this socket last selected.
    db: u32,
}

impl Socket {
    async fn call(&mut self, args: &[Vec<u8>]) -> DbResult<Resp> {
        self.io.get_mut().write_all(&encode(args)).await.map_err(|e| DbError::Server(e.to_string()))?;
        self.io.get_mut().flush().await.map_err(|e| DbError::Server(e.to_string()))?;
        read_reply(&mut self.io, 0).await
    }

    /// A command that must work: an error reply becomes the server's message.
    async fn ok(&mut self, args: &[&str]) -> DbResult<Resp> {
        let args: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
        match self.call(&args).await? {
            Resp::Error(e) => Err(DbError::Server(e)),
            r => Ok(r),
        }
    }

    async fn select(&mut self, db: u32) -> DbResult<()> {
        if self.db != db {
            self.ok(&["SELECT", &db.to_string()]).await?;
            self.db = db;
        }
        Ok(())
    }
}

pub struct RedisConn {
    host: String,
    port: u16,
    name: String,
    user: String,
    password: Option<String>,
    db: u32,
    tls: Option<std::sync::Arc<rustls::ClientConfig>>,
    idle: Mutex<Vec<Socket>>,
    running: Mutex<std::collections::HashMap<Uuid, std::sync::Arc<Notify>>>,
}

fn text(r: &Resp) -> Option<String> {
    match r {
        Resp::Simple(s) => Some(s.clone()),
        Resp::Bulk(Some(b)) => Some(String::from_utf8_lossy(b).into_owned()),
        Resp::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

impl RedisConn {
    pub async fn connect(spec: &ConnectSpec) -> DbResult<Self> {
        let db = match spec.option("db") {
            None => spec.database.as_deref().filter(|d| !d.is_empty()).and_then(|d| d.parse().ok()).unwrap_or(0),
            Some(d) => d.parse().map_err(|_| DbError::Invalid("the database number must be a whole number, like 0".into()))?,
        };
        let (host, port) = spec.dial();
        let me = Self {
            host,
            port,
            // The name the certificate is checked against, which stays the server's even through a tunnel.
            name: spec.host.clone(),
            user: spec.user.clone(),
            password: spec.password.clone().filter(|p| !p.is_empty()),
            db,
            tls: tlsconf::client_config(spec.tls)?,
            idle: Mutex::default(),
            running: Mutex::default(),
        };
        // Fail now, with the real reason, rather than on the first command.
        let s = me.open().await?;
        me.release(s);
        Ok(me)
    }

    async fn open(&self) -> DbResult<Socket> {
        let addr = format!("{}:{}", self.host, self.port);
        let attempt = async {
            let tcp = TcpStream::connect((self.host.as_str(), self.port)).await.map_err(|e| DbError::Connect { addr: addr.clone(), reason: e.to_string() })?;
            let _ = tcp.set_nodelay(true);
            let io: Box<dyn Wire> = match &self.tls {
                None => Box::new(tcp),
                Some(cfg) => {
                    let name = rustls::pki_types::ServerName::try_from(self.name.clone()).map_err(|_| DbError::Invalid(format!("\"{}\" can't be checked as a server name", self.name)))?;
                    let tls = tokio_rustls::TlsConnector::from(cfg.clone()).connect(name, tcp).await.map_err(|e| DbError::Connect { addr: addr.clone(), reason: format!("TLS: {e}") })?;
                    Box::new(tls)
                }
            };
            let mut s = Socket { io: BufReader::new(io), db: 0 };
            if let Some(p) = &self.password {
                let mut args = vec!["AUTH"];
                if !self.user.is_empty() {
                    args.push(&self.user);
                }
                args.push(p);
                s.ok(&args).await.map_err(|e| match e {
                    DbError::Server(m) if m.contains("WRONGPASS") || m.contains("invalid username-password") => DbError::Server("Redis refused the user name or password (WRONGPASS)".into()),
                    other => other,
                })?;
            } else if !self.user.is_empty() {
                return Err(DbError::Invalid("a user name needs a password too (Redis ACL users sign in with both)".into()));
            }
            // The first command tells a server that needs a password, or isn't Redis at all.
            match s.call(&[b"PING".to_vec()]).await? {
                Resp::Error(e) if e.starts_with("NOAUTH") => return Err(DbError::Server("this Redis needs a password (NOAUTH); enter it in the connection".into())),
                Resp::Error(e) => return Err(DbError::Server(e)),
                _ => {}
            }
            Ok::<_, DbError>(s)
        };
        let mut s = match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
            Err(_) => return Err(DbError::Timeout(addr)),
            Ok(r) => r?,
        };
        s.select(self.db).await?;
        Ok(s)
    }

    async fn acquire(&self) -> DbResult<Socket> {
        let next = self.idle.lock().unwrap_or_else(|p| p.into_inner()).pop();
        match next {
            Some(s) => Ok(s),
            None => self.open().await,
        }
    }

    fn release(&self, s: Socket) {
        let mut idle = self.idle.lock().unwrap_or_else(|p| p.into_inner());
        if idle.len() < MAX_IDLE {
            idle.push(s);
        }
    }

    /// Run commands on a socket, keeping it only if all went well (a half-read socket is thrown away).
    async fn with<T>(&self, qid: Option<Uuid>, f: impl for<'a> FnOnce(&'a mut Socket) -> std::pin::Pin<Box<dyn std::future::Future<Output = DbResult<T>> + Send + 'a>>) -> DbResult<T> {
        let mut s = self.acquire().await?;
        let notify = qid.map(|q| {
            let n = std::sync::Arc::new(Notify::new());
            self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(q, n.clone());
            n
        });
        let out = match &notify {
            Some(n) => tokio::select! {
                r = f(&mut s) => r,
                _ = n.notified() => Err(DbError::Cancelled),
            },
            None => f(&mut s).await,
        };
        if let Some(q) = qid {
            self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&q);
        }
        if out.is_ok() {
            self.release(s);
        }
        out
    }
}

// -- laying replies out ------------------------------------------------------------------------------

/// A byte string as text: valid UTF-8 as it is, anything else with `\xNN` escapes (as `redis-cli` shows it).
pub fn show(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let mut out = String::new();
            for &b in bytes {
                match b {
                    b'\\' => out.push_str("\\\\"),
                    0x20..=0x7e | b'\n' | b'\r' | b'\t' => out.push(b as char),
                    _ => out.push_str(&format!("\\x{b:02x}")),
                }
            }
            out
        }
    }
}

fn cell(r: &Resp) -> Value {
    match r {
        Resp::Simple(s) => cut_text(s),
        Resp::Error(e) => cut_text(&format!("(error) {e}")),
        Resp::Int(i) => json!(i),
        Resp::Bulk(None) | Resp::Array(None) => Value::Null,
        Resp::Bulk(Some(b)) => cut_text(&show(b)),
        Resp::Array(Some(items)) => cut_text(&Value::Array(items.iter().map(json_of).collect()).to_string()),
    }
}

fn json_of(r: &Resp) -> Value {
    match r {
        Resp::Simple(s) => Value::String(s.clone()),
        Resp::Error(e) => Value::String(format!("(error) {e}")),
        Resp::Int(i) => json!(i),
        Resp::Bulk(None) | Resp::Array(None) => Value::Null,
        Resp::Bulk(Some(b)) => Value::String(show(b)),
        Resp::Array(Some(items)) => Value::Array(items.iter().map(json_of).collect()),
    }
}

fn two_columns(a: &str, b: &str, kind_b: ColumnKind, rows: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: vec![Column { name: a.into(), data_type: "string".into(), kind: ColumnKind::Text }, Column { name: b.into(), data_type: "string".into(), kind: kind_b }],
        rows,
        ..Default::default()
    }
}

/// A reply as a result grid. `args` is the command, which decides how a flat list is read (field and value pairs,
/// members and scores).
pub fn lay_out(args: &[Vec<u8>], reply: &Resp, limit: usize) -> QueryResult {
    let name = name_of(args);
    let sub = arg_upper(args, 1);
    let pairs = matches!((name.as_str(), sub.as_str()), ("HGETALL", _) | ("CONFIG", "GET") | ("XINFO", _) | ("MEMORY", "STATS"));
    let scored = matches!(name.as_str(), "ZRANGE" | "ZREVRANGE" | "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZPOPMIN" | "ZPOPMAX" | "ZRANGEBYLEX") && args.iter().any(|a| a.eq_ignore_ascii_case(b"WITHSCORES")) || matches!(name.as_str(), "ZPOPMIN" | "ZPOPMAX");
    match reply {
        Resp::Simple(s) if s == "OK" => text_table(&["result"], vec![vec![json!("OK")]]),
        Resp::Simple(_) | Resp::Error(_) | Resp::Int(_) | Resp::Bulk(None) | Resp::Array(None) => text_table(&["result"], vec![vec![cell(reply)]]),
        Resp::Bulk(Some(b)) => {
            let s = show(b);
            // INFO, CLIENT LIST and friends are text with a line per fact.
            if s.contains('\n') && s.len() < MAX_CELL_BYTES * 8 && matches!(name.as_str(), "INFO" | "CLIENT" | "CLUSTER" | "COMMAND" | "MEMORY" | "LATENCY" | "SLOWLOG" | "DEBUG") {
                let rows: Vec<Vec<Value>> = s.lines().filter(|l| !l.is_empty()).map(|l| vec![cut_text(l)]).collect();
                let mut r = text_table(&["line"], rows);
                if r.rows.len() > limit {
                    r.rows.truncate(limit);
                    r.truncated = true;
                }
                return r;
            }
            text_table(&["result"], vec![vec![cut_text(&s)]])
        }
        Resp::Array(Some(items)) => {
            let flat = items.iter().all(|i| !matches!(i, Resp::Array(_)));
            let mut out = if (pairs || scored) && flat && items.len() % 2 == 0 {
                if pairs {
                    two_columns("field", "value", ColumnKind::Text, items.chunks(2).map(|p| vec![cell(&p[0]), cell(&p[1])]).collect())
                } else {
                    two_columns("member", "score", ColumnKind::Number, items.chunks(2).map(|p| vec![cell(&p[0]), score(&p[1])]).collect())
                }
            } else if matches!(name.as_str(), "XRANGE" | "XREVRANGE") && items.iter().all(|i| matches!(i, Resp::Array(Some(p)) if p.len() == 2)) {
                let rows = items.iter().filter_map(|i| if let Resp::Array(Some(p)) = i { Some(vec![cell(&p[0]), cell(&p[1])]) } else { None }).collect();
                two_columns("id", "fields", ColumnKind::Json, rows)
            } else {
                let kind = if items.iter().all(|i| matches!(i, Resp::Int(_))) { ColumnKind::Number } else if flat { ColumnKind::Text } else { ColumnKind::Json };
                QueryResult {
                    columns: vec![Column { name: "#".into(), data_type: "integer".into(), kind: ColumnKind::Number }, Column { name: "value".into(), data_type: "string".into(), kind }],
                    rows: items.iter().enumerate().map(|(i, v)| vec![json!(i + 1), cell(v)]).collect(),
                    ..Default::default()
                }
            };
            if out.rows.len() > limit {
                out.rows.truncate(limit);
                out.truncated = true;
            }
            out
        }
    }
}

// -- keys --------------------------------------------------------------------------------------------

/// A glob pattern that matches exactly `prefix` and anything after it.
pub fn prefix_pattern(prefix: &str) -> String {
    let mut p = String::new();
    for c in prefix.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('*');
    p
}

/// The next level below `prefix`: names with another colon after the prefix become folders (counted), the rest
/// are keys.
pub fn group_keys(prefix: &str, keys: &[String]) -> (Vec<(String, usize)>, Vec<String>) {
    let mut folders: Vec<(String, usize)> = Vec::new();
    let mut leaves = Vec::new();
    for k in keys {
        let rest = &k[prefix.len().min(k.len())..];
        match rest.find(':') {
            Some(i) if i + 1 < rest.len() => {
                let folder = format!("{prefix}{}", &rest[..=i]);
                match folders.iter_mut().find(|(f, _)| *f == folder) {
                    Some((_, n)) => *n += 1,
                    None => folders.push((folder, 1)),
                }
            }
            _ => leaves.push(k.clone()),
        }
    }
    folders.sort();
    leaves.sort();
    (folders, leaves)
}

fn ttl_text(ms: i64) -> Option<String> {
    match ms {
        -1 | -2 => None,
        ms if ms < 1000 => Some(format!("{ms} ms")),
        ms if ms < 60_000 => Some(format!("{} s", ms / 1000)),
        ms if ms < 3_600_000 => Some(format!("{} min", ms / 60_000)),
        ms if ms < 86_400_000 => Some(format!("{} h", ms / 3_600_000)),
        ms => Some(format!("{} days", ms / 86_400_000)),
    }
}

fn bulk_text(r: &Resp) -> Vec<String> {
    match r {
        Resp::Array(Some(items)) => items.iter().filter_map(text).collect(),
        _ => Vec::new(),
    }
}

impl RedisConn {
    /// Names matching a pattern, by `SCAN` until a limit.
    async fn scan_keys(s: &mut Socket, pattern: &str, max: usize) -> DbResult<(Vec<String>, bool)> {
        let mut cursor = "0".to_string();
        let mut keys = Vec::new();
        for _ in 0..10_000 {
            let r = s.ok(&["SCAN", &cursor, "MATCH", pattern, "COUNT", "1000"]).await?;
            let Resp::Array(Some(parts)) = r else { return Err(protocol("a SCAN reply")) };
            cursor = parts.first().and_then(text).unwrap_or_else(|| "0".into());
            keys.extend(parts.get(1).map(bulk_text).unwrap_or_default());
            if keys.len() > max {
                keys.truncate(max);
                return Ok((keys, true));
            }
            if cursor == "0" {
                break;
            }
        }
        // SCAN may return a name more than once.
        keys.sort();
        keys.dedup();
        Ok((keys, false))
    }

    /// A key's type: string, hash, list, set, zset, stream, or none.
    async fn type_of(s: &mut Socket, key: &str) -> DbResult<String> {
        Ok(text(&s.ok(&["TYPE", key]).await?).unwrap_or_else(|| "none".into()))
    }

    /// The grid for one key, whatever its type.
    async fn view(s: &mut Socket, key: &str, limit: usize) -> DbResult<QueryResult> {
        let ty = Self::type_of(s, key).await?;
        let n = limit.to_string();
        let last = limit.saturating_sub(1).to_string();
        let mut out = match ty.as_str() {
            "string" => {
                let v = s.ok(&["GET", key]).await?;
                QueryResult {
                    columns: vec![Column { name: "key".into(), data_type: "string".into(), kind: ColumnKind::Text }, Column { name: "value".into(), data_type: "string".into(), kind: ColumnKind::Text }],
                    rows: vec![vec![cut_text(key), cell(&v)]],
                    ..Default::default()
                }
            }
            "hash" => {
                let mut rows = Vec::new();
                let mut cursor = "0".to_string();
                let mut cut = false;
                loop {
                    let r = s.ok(&["HSCAN", key, &cursor, "COUNT", "1000"]).await?;
                    let Resp::Array(Some(parts)) = r else { return Err(protocol("an HSCAN reply")) };
                    cursor = parts.first().and_then(text).unwrap_or_else(|| "0".into());
                    if let Some(Resp::Array(Some(flat))) = parts.get(1) {
                        for p in flat.chunks(2) {
                            if rows.len() >= limit {
                                cut = true;
                                break;
                            }
                            if p.len() == 2 {
                                rows.push(vec![cell(&p[0]), cell(&p[1])]);
                            }
                        }
                    }
                    if cut || cursor == "0" {
                        break;
                    }
                }
                let mut r = two_columns("field", "value", ColumnKind::Text, rows);
                r.truncated = cut;
                r
            }
            "list" => {
                let r = s.ok(&["LRANGE", key, "0", &last]).await?;
                let len = text(&s.ok(&["LLEN", key]).await?).and_then(|l| l.parse::<usize>().ok()).unwrap_or(0);
                let rows = match r {
                    Resp::Array(Some(items)) => items.iter().enumerate().map(|(i, v)| vec![json!(i), cell(v)]).collect(),
                    _ => vec![],
                };
                let mut q = two_columns("index", "value", ColumnKind::Text, rows);
                q.columns[0].kind = ColumnKind::Number;
                q.truncated = len > limit;
                q
            }
            "set" => {
                let mut members: Vec<Vec<Value>> = Vec::new();
                let mut cursor = "0".to_string();
                let mut cut = false;
                loop {
                    let r = s.ok(&["SSCAN", key, &cursor, "COUNT", "1000"]).await?;
                    let Resp::Array(Some(parts)) = r else { return Err(protocol("an SSCAN reply")) };
                    cursor = parts.first().and_then(text).unwrap_or_else(|| "0".into());
                    if let Some(Resp::Array(Some(items))) = parts.get(1) {
                        for m in items {
                            if members.len() >= limit {
                                cut = true;
                                break;
                            }
                            members.push(vec![cell(m)]);
                        }
                    }
                    if cut || cursor == "0" {
                        break;
                    }
                }
                let mut r = text_table(&["member"], members);
                r.truncated = cut;
                r
            }
            "zset" => {
                let r = s.ok(&["ZRANGE", key, "0", &last, "WITHSCORES"]).await?;
                let card = text(&s.ok(&["ZCARD", key]).await?).and_then(|l| l.parse::<usize>().ok()).unwrap_or(0);
                let rows = match r {
                    Resp::Array(Some(items)) => items.chunks(2).filter(|p| p.len() == 2).map(|p| vec![cell(&p[0]), score(&p[1])]).collect(),
                    _ => vec![],
                };
                let mut q = two_columns("member", "score", ColumnKind::Number, rows);
                q.truncated = card > limit;
                q
            }
            "stream" => {
                let r = s.ok(&["XRANGE", key, "-", "+", "COUNT", &n]).await?;
                let len = text(&s.ok(&["XLEN", key]).await?).and_then(|l| l.parse::<usize>().ok()).unwrap_or(0);
                let mut q = lay_out(&[b"XRANGE".to_vec()], &r, limit);
                q.truncated = len > limit;
                q
            }
            "none" => return Err(DbError::Server(format!("there is no key named {}", quote_arg(key)))),
            other => return Err(DbError::Server(format!("this key's type ({other}) can't be shown here; use the console"))),
        };
        out.affected_rows = None;
        Ok(out)
    }
}

fn score(r: &Resp) -> Value {
    match text(r).and_then(|t| t.parse::<f64>().ok()) {
        Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
        Some(f) if f.is_finite() => json!(f),
        _ => cell(r),
    }
}

/// The command an inline edit runs, as arguments (shown with the same quoting the console takes).
pub fn edit_command(ty: &str, key: &str, edit: &RowEdit) -> DbResult<Vec<String>> {
    let change = edit.changes.first().ok_or_else(|| DbError::Invalid("nothing was changed".into()))?;
    let pk = edit.key.first().ok_or_else(|| DbError::Invalid("the row has no key".into()))?;
    let value = |c: &super::CellEdit| c.value.clone().ok_or_else(|| DbError::Invalid("Redis has no NULL: delete the field or key with a command instead".into()));
    let v = value(change)?;
    let k = value(pk)?;
    if edit.changes.len() != 1 {
        return Err(DbError::Invalid("change one value at a time".into()));
    }
    match (ty, change.column.as_str()) {
        ("string", "value") => Ok(vec!["SET".into(), key.into(), v, "KEEPTTL".into()]),
        ("hash", "value") => Ok(vec!["HSET".into(), key.into(), k, v]),
        ("list", "value") => {
            k.parse::<i64>().map_err(|_| DbError::Invalid("a list's index is a number".into()))?;
            Ok(vec!["LSET".into(), key.into(), k, v])
        }
        ("zset", "score") => {
            v.parse::<f64>().map_err(|_| DbError::Invalid("a score is a number".into()))?;
            Ok(vec!["ZADD".into(), key.into(), v, k])
        }
        ("zset", _) => Err(DbError::Invalid("a sorted set's member can't be renamed here; remove it and add the new one with a command".into())),
        ("set", _) => Err(DbError::Invalid("a set's members can't be edited in place; use SREM and SADD".into())),
        ("stream", _) => Err(DbError::Invalid("a stream's entries can't be edited; they are only added to".into())),
        (_, c) => Err(DbError::Invalid(format!("the column \"{c}\" can't be edited"))),
    }
}

#[async_trait::async_trait]
impl Backend for RedisConn {
    async fn server_version(&self) -> DbResult<String> {
        self.with(None, |s| {
            Box::pin(async move {
                let info = text(&s.ok(&["INFO", "server"]).await?).unwrap_or_default();
                let get = |k: &str| info.lines().find_map(|l| l.strip_prefix(&format!("{k}:"))).map(|v| v.trim().to_string());
                Ok(format!("Redis {}{}", get("redis_version").unwrap_or_default(), get("redis_mode").filter(|m| m != "standalone").map(|m| format!(" ({m})")).unwrap_or_default()))
            })
        })
        .await
    }

    async fn query(&self, qid: Uuid, statement: &str, limit: usize) -> DbResult<QueryResult> {
        let started = Instant::now();
        let commands = parse_statements(statement).map_err(DbError::Invalid)?;
        for c in &commands {
            if let Some(why) = unsupported(&c.args) {
                return Err(DbError::Invalid(why.into()));
            }
        }
        let default_db = self.db;
        let mut out = self
            .with(Some(qid), move |s| {
                Box::pin(async move {
                    let mut last = QueryResult::default();
                    for c in commands {
                        s.select(c.db.unwrap_or(default_db)).await?;
                        // The console's own command: read one key, whatever it is.
                        if name_of(&c.args) == "VIEW" {
                            let key = c.args.get(1).map(|k| show(k)).ok_or_else(|| DbError::Invalid("VIEW needs a key: VIEW mykey".into()))?;
                            last = Self::view(s, &key, limit).await?;
                            continue;
                        }
                        let reply = s.call(&c.args).await?;
                        if let Resp::Error(e) = &reply {
                            return Err(DbError::Server(e.clone()));
                        }
                        last = lay_out(&c.args, &reply, limit);
                    }
                    Ok(last)
                })
            })
            .await?;
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
        let path = path.to_vec();
        self.with(None, move |s| {
            Box::pin(async move {
                match path.as_slice() {
                    [] => {
                        let info = text(&s.ok(&["INFO", "keyspace"]).await?).unwrap_or_default();
                        let mut dbs: Vec<(u32, String)> = info
                            .lines()
                            .filter_map(|l| {
                                let (name, rest) = l.split_once(':')?;
                                let n: u32 = name.strip_prefix("db")?.parse().ok()?;
                                let keys = rest.split(',').find_map(|p| p.strip_prefix("keys="))?;
                                Some((n, format!("{keys} keys")))
                            })
                            .collect();
                        // A fresh server has no keyspace lines, but database 0 is there.
                        if !dbs.iter().any(|(n, _)| *n == 0) {
                            dbs.insert(0, (0, "empty".into()));
                        }
                        dbs.sort();
                        Ok(dbs.into_iter().map(|(n, d)| TreeNode { name: format!("db{n}"), kind: NodeKind::Database, detail: Some(d), expandable: true }).collect())
                    }
                    [db, rest @ ..] => {
                        let n: u32 = db.strip_prefix("db").and_then(|d| d.parse().ok()).ok_or_else(|| DbError::Invalid("not a Redis database".into()))?;
                        s.select(n).await?;
                        let prefix = rest.last().cloned().unwrap_or_default();
                        let (keys, more) = Self::scan_keys(s, &prefix_pattern(&prefix), MAX_TREE_KEYS).await?;
                        let (folders, leaves) = group_keys(&prefix, &keys);
                        let mut nodes: Vec<TreeNode> = folders
                            .into_iter()
                            .map(|(f, n)| TreeNode { name: f, kind: NodeKind::Folder, detail: Some(format!("{n}{}", if more { "+" } else { "" })), expandable: true })
                            .collect();
                        for (i, k) in leaves.iter().enumerate() {
                            let mut detail = None;
                            if i < DETAIL_KEYS {
                                let ty = Self::type_of(s, k).await.unwrap_or_default();
                                let ttl = text(&s.ok(&["PTTL", k]).await?).and_then(|t| t.parse::<i64>().ok()).and_then(ttl_text);
                                detail = Some(match ttl {
                                    Some(t) => format!("{ty} · expires in {t}"),
                                    None => ty,
                                });
                            }
                            nodes.push(TreeNode { name: k.clone(), kind: NodeKind::Table, detail, expandable: false });
                        }
                        if more {
                            nodes.push(TreeNode { name: format!("(showing the first {MAX_TREE_KEYS}; open a folder or use SCAN MATCH)"), kind: NodeKind::Folder, detail: None, expandable: false });
                        }
                        Ok(nodes)
                    }
                }
            })
        })
        .await
    }

    async fn table_info(&self, database: &str, key: &str) -> DbResult<TableInfo> {
        let db: u32 = database.strip_prefix("db").and_then(|d| d.parse().ok()).unwrap_or(self.db);
        let key = key.to_string();
        self.with(None, move |s| {
            Box::pin(async move {
                s.select(db).await?;
                let ty = Self::type_of(s, &key).await?;
                let col = |name: &str, pk: bool| ColumnInfo { name: name.into(), data_type: "string".into(), nullable: false, primary_key: pk, default: None };
                let (columns, pk): (Vec<ColumnInfo>, Vec<&str>) = match ty.as_str() {
                    "string" => (vec![col("key", true), col("value", false)], vec!["key"]),
                    "hash" => (vec![col("field", true), col("value", false)], vec!["field"]),
                    "list" => (vec![col("index", true), col("value", false)], vec!["index"]),
                    "zset" => (vec![col("member", true), col("score", false)], vec!["member"]),
                    "set" => (vec![col("member", false)], vec![]),
                    _ => (vec![], vec![]),
                };
                Ok(TableInfo { columns, primary_key: pk.into_iter().map(String::from).collect() })
            })
        })
        .await
    }

    fn preview_update(&self, edit: &RowEdit) -> DbResult<String> {
        // The type isn't known here; the preview follows the shape the grid had (the column being changed).
        let ty = match edit.changes.first().map(|c| c.column.as_str()) {
            Some("score") => "zset",
            _ => match edit.key.first().map(|k| k.column.as_str()) {
                Some("key") => "string",
                Some("index") => "list",
                _ => "hash",
            },
        };
        let key = &edit.table;
        // A string's key column holds the key; the others hold the field, index or member.
        let cmd = edit_command(ty, key, edit)?;
        Ok(cmd.iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" "))
    }

    async fn apply_update(&self, edit: &RowEdit) -> DbResult<u64> {
        let db: u32 = edit.database.strip_prefix("db").and_then(|d| d.parse().ok()).unwrap_or(self.db);
        let (key, edit) = (edit.table.clone(), edit.clone());
        self.with(None, move |s| {
            Box::pin(async move {
                s.select(db).await?;
                let ty = Self::type_of(s, &key).await?;
                if ty == "none" {
                    return Ok(0);
                }
                let cmd = edit_command(&ty, &key, &edit)?;
                let args: Vec<Vec<u8>> = cmd.iter().map(|a| a.as_bytes().to_vec()).collect();
                match s.call(&args).await? {
                    Resp::Error(e) => Err(DbError::Server(e)),
                    _ => Ok(1),
                }
            })
        })
        .await
    }

    async fn close(&self) {
        self.idle.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    fn is_sql(&self) -> bool {
        false
    }

    fn destructive_reason(&self, text: &str) -> Option<String> {
        let commands = parse_statements(text).ok()?;
        commands.iter().find_map(|c| destructive(&c.args))
    }
}

#[cfg(test)]
mod tests;
