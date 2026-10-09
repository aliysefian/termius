//! Command-line control: `sshvault list`, `sshvault connect web-01`,
//! `sshvault run --group Production -- uptime`, `sshvault status`.
//!
//! The same binary is the client. It talks to the running app over a local
//! socket (a named pipe on Windows) using JSON lines, so commands use the
//! unlocked vault and nothing secret ever passes through the CLI.
//!
//! The socket only exists while the vault is unlocked *and* the user has
//! turned command-line access on for this computer. `run` additionally
//! needs approval in the app before it touches any server.

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::models::Host;

pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// Which hosts a request is about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selector {
    /// Labels (case-insensitive) or ID prefixes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// A group and everything under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    List {
        #[serde(flatten)]
        select: Selector,
    },
    Connect {
        host: String,
    },
    Run {
        #[serde(flatten)]
        select: Selector,
        command: String,
        timeout_secs: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Status {
        unlocked: bool,
        hosts: usize,
        agent: Option<String>,
    },
    Host {
        label: String,
        hostname: String,
        port: u16,
        group: String,
        environment: String,
        tags: Vec<String>,
    },
    Opened {
        label: String,
    },
    /// Approval is being asked for in the app.
    Waiting {
        message: String,
    },
    Started {
        host: String,
    },
    Output {
        host: String,
        stdout: String,
        stderr: String,
        exit_code: Option<u32>,
        duration_ms: u64,
    },
    Failed {
        host: String,
        message: String,
    },
    Done {
        ok: bool,
    },
    Error {
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Host selection (pure, tested)
// ---------------------------------------------------------------------------

fn norm_group(g: &str) -> String {
    g.split('/').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/").to_lowercase()
}

/// Hosts matching `sel`, in label order. Names that match nothing are
/// returned as errors so a typo never silently runs on fewer hosts.
pub fn select<'a>(
    hosts: &'a [(uuid::Uuid, Host)],
    sel: &Selector,
) -> Result<Vec<&'a (uuid::Uuid, Host)>, String> {
    let mut out: Vec<&(uuid::Uuid, Host)> = Vec::new();
    let mut push = |h: &'a (uuid::Uuid, Host)| {
        if !out.iter().any(|x| x.0 == h.0) {
            out.push(h);
        }
    };
    for name in &sel.hosts {
        let lower = name.to_lowercase();
        let exact: Vec<_> = hosts.iter().filter(|(_, h)| h.label.to_lowercase() == lower).collect();
        let found: Vec<_> = if !exact.is_empty() {
            exact
        } else if name.len() >= 4 {
            hosts.iter().filter(|(id, _)| id.to_string().starts_with(&lower)).collect()
        } else {
            Vec::new()
        };
        match found.as_slice() {
            [] => return Err(format!("no host called \"{name}\"")),
            [one] => push(one),
            many => {
                return Err(format!(
                    "\"{name}\" matches {} hosts; use a more specific name or an ID",
                    many.len()
                ))
            }
        }
    }
    let by_filter = sel.group.is_some() || sel.tag.is_some();
    if by_filter {
        let group = sel.group.as_deref().map(norm_group);
        for h in hosts {
            let g = norm_group(&h.1.group);
            let in_group = group.as_ref().is_none_or(|want| g == *want || g.starts_with(&format!("{want}/")));
            let has_tag = sel.tag.as_ref().is_none_or(|t| h.1.tags.iter().any(|x| x.eq_ignore_ascii_case(t)));
            if in_group && has_tag {
                push(h);
            }
        }
    }
    if sel.hosts.is_empty() && !by_filter {
        for h in hosts {
            push(h);
        }
    }
    out.sort_by_key(|h| h.1.label.to_lowercase());
    Ok(out)
}

// ---------------------------------------------------------------------------
// Command line (pure, tested)
// ---------------------------------------------------------------------------

pub const USAGE: &str = "\
Usage: sshvault <command> [options]

Commands (need SSHVault running, the vault unlocked, and
Settings → Command line turned on):

  status                         Is the vault unlocked? How many hosts?
  list [--group G] [--tag T]     List hosts
  connect <host>                 Open a terminal tab for a host in the app
  run [<host>...] [--group G] [--tag T] [--timeout SECS] -- <command>
                                 Run a command on hosts and print the output.
                                 The app asks you to approve it first.

Options:
  --json                         Print JSON lines instead of text
  -h, --help                     Show this help
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub request: Request,
    pub json: bool,
}

/// Is this a CLI invocation? The GUI starts for anything else (including
/// the flags desktop launchers and Tauri pass).
pub fn is_cli(args: &[String]) -> bool {
    matches!(
        args.get(1).map(String::as_str),
        Some("status" | "list" | "ls" | "connect" | "run" | "help" | "--help" | "-h")
    )
}

pub fn parse(args: &[String]) -> Result<Option<Cli>, String> {
    let mut it = args.iter().skip(1).map(String::as_str);
    let Some(cmd) = it.next() else { return Ok(None) };
    let mut rest: Vec<&str> = it.collect();
    // Everything after `--` is the remote command, untouched: its own
    // options (`df -h`) must not be read as ours.
    let command: Option<String> = rest
        .iter()
        .position(|a| *a == "--")
        .map(|i| {
            let words: Vec<&str> = rest.drain(i..).skip(1).collect();
            words.join(" ")
        });
    let json = rest.contains(&"--json");
    if matches!(cmd, "help" | "--help" | "-h") || rest.iter().any(|a| matches!(*a, "-h" | "--help")) {
        return Ok(None);
    }
    let mut sel = Selector::default();
    let mut timeout = 60u64;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i];
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            rest.get(i).map(|v| v.to_string()).ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "--json" => {}
            "--group" | "-g" => sel.group = Some(value(a)?),
            "--tag" | "-t" => sel.tag = Some(value(a)?),
            "--timeout" => {
                timeout = value(a)?.parse().map_err(|_| "--timeout needs a number of seconds".to_string())?;
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => positional.push(s.to_string()),
        }
        i += 1;
    }
    let request = match cmd {
        "status" => Request::Status,
        "list" | "ls" => Request::List {
            select: Selector { hosts: positional, ..sel },
        },
        "connect" => match positional.as_slice() {
            [h] => Request::Connect { host: h.clone() },
            _ => return Err("connect needs exactly one host".into()),
        },
        "run" => {
            let command = command
                .filter(|c| !c.trim().is_empty())
                .ok_or("run needs a command after --, e.g. sshvault run web-01 -- uptime")?;
            let select = Selector { hosts: positional, ..sel };
            if select.hosts.is_empty() && select.group.is_none() && select.tag.is_none() {
                return Err("run needs hosts, --group or --tag (it never defaults to every host)".into());
            }
            Request::Run {
                select,
                command,
                timeout_secs: timeout.clamp(1, 3600),
            }
        }
        other => return Err(format!("unknown command {other}")),
    };
    Ok(Some(Cli { request, json }))
}

// ---------------------------------------------------------------------------
// Where the socket lives
// ---------------------------------------------------------------------------

/// The control socket path (or pipe name). Client and app agree on it
/// without any shared state.
pub fn endpoint() -> String {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
        format!(r"\\.\pipe\sshvault-control-{user}")
    }
    #[cfg(unix)]
    {
        socket_dir().join("control.sock").to_string_lossy().into_owned()
    }
}

/// `$XDG_RUNTIME_DIR/sshvault`, else a per-user folder in the temp dir.
#[cfg(unix)]
pub fn socket_dir() -> PathBuf {
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(run).join("sshvault");
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    std::env::temp_dir().join(format!("sshvault-{uid}"))
}

/// Refuse a socket folder someone else owns or can write to: in a shared
/// temp dir another user could have created it first.
#[cfg(unix)]
fn check_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    crate::vault::atomic::create_private_dir(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: as above.
    let uid = unsafe { libc::getuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not a private folder owned by you", dir.display()),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Server (in the app)
// ---------------------------------------------------------------------------

/// Answers requests. Implemented by the app over the unlocked vault.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: Request, out: mpsc::UnboundedSender<Response>) -> BoxFuture<()>;
}

/// Longest request line accepted.
const MAX_REQUEST: usize = 64 * 1024;

async fn serve_conn<S>(stream: S, handler: Arc<dyn Handler>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    let (r, mut w) = tokio::io::split(stream);
    let mut r = tokio::io::BufReader::new(r);
    let mut line = String::new();
    let read = (&mut r).take(MAX_REQUEST as u64).read_line(&mut line).await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    match read.ok().filter(|n| *n > 0).and_then(|_| serde_json::from_str::<Request>(line.trim()).ok()) {
        Some(req) => {
            let fut = handler.handle(req, tx);
            tokio::spawn(fut);
        }
        None => {
            let _ = tx.send(Response::Error { message: "bad request".into() });
            drop(tx);
        }
    }
    while let Some(resp) = rx.recv().await {
        let Ok(mut json) = serde_json::to_vec(&resp) else { continue };
        json.push(b'\n');
        if w.write_all(&json).await.is_err() {
            return;
        }
    }
    let _ = w.shutdown().await;
}

use tokio::io::AsyncReadExt as _;

/// A running control socket; dropping it stops listening.
pub struct ControlHandle {
    task: tokio::task::JoinHandle<()>,
    #[cfg(unix)]
    socket: PathBuf,
}

impl Drop for ControlHandle {
    fn drop(&mut self) {
        self.task.abort();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Start listening at [`endpoint`]. Must run inside a tokio runtime.
#[cfg(unix)]
pub fn start(handler: Arc<dyn Handler>) -> std::io::Result<ControlHandle> {
    start_at(&socket_dir(), handler)
}

#[cfg(unix)]
pub fn start_at(dir: &std::path::Path, handler: Arc<dyn Handler>) -> std::io::Result<ControlHandle> {
    use std::os::unix::fs::PermissionsExt;
    check_private_dir(dir)?;
    let socket = dir.join("control.sock");
    if std::fs::symlink_metadata(&socket).is_ok() {
        std::fs::remove_file(&socket)?;
    }
    let listener = tokio::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    // spawn-ok: started from the async unlock and cli_set_enabled commands
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve_conn(stream, Arc::clone(&handler)));
        }
    });
    Ok(ControlHandle { task, socket })
}

#[cfg(windows)]
pub fn start(handler: Arc<dyn Handler>) -> std::io::Result<ControlHandle> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = endpoint();
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    // spawn-ok: started from the async unlock and cli_set_enabled commands
    let task = tokio::spawn(async move {
        loop {
            if server.connect().await.is_err() {
                return;
            }
            let connected = server;
            server = match ServerOptions::new().reject_remote_clients(true).create(&name) {
                Ok(s) => s,
                Err(_) => return,
            };
            tokio::spawn(serve_conn(connected, Arc::clone(&handler)));
        }
    });
    Ok(ControlHandle { task })
}

// ---------------------------------------------------------------------------
// Client (the CLI)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn connect_endpoint(path: &str) -> std::io::Result<(Box<dyn std::io::Read>, Box<dyn Write>)> {
    let s = std::os::unix::net::UnixStream::connect(path)?;
    Ok((Box::new(s.try_clone()?), Box::new(s)))
}

#[cfg(windows)]
fn connect_endpoint(path: &str) -> std::io::Result<(Box<dyn std::io::Read>, Box<dyn Write>)> {
    let f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    Ok((Box::new(f.try_clone()?), Box::new(f)))
}

/// Render one response for a person. Returns (stdout text, stderr text).
pub fn render(resp: &Response) -> (String, String) {
    match resp {
        Response::Status { unlocked, hosts, agent } => (
            format!(
                "vault: {}\nhosts: {hosts}\nssh agent: {}\n",
                if *unlocked { "unlocked" } else { "locked" },
                agent.as_deref().map(|p| format!("on (SSH_AUTH_SOCK={p})")).unwrap_or_else(|| "off".into())
            ),
            String::new(),
        ),
        Response::Host { label, hostname, port, group, environment, tags } => {
            let addr = if *port == 22 { hostname.clone() } else { format!("{hostname}:{port}") };
            let mut extra = Vec::new();
            if !group.is_empty() {
                extra.push(group.clone());
            }
            if !environment.is_empty() {
                extra.push(environment.clone());
            }
            if !tags.is_empty() {
                extra.push(tags.join(","));
            }
            (format!("{label:<24} {addr:<28} {}\n", extra.join("  ")), String::new())
        }
        Response::Opened { label } => (format!("Opened {label} in SSHVault.\n"), String::new()),
        Response::Waiting { message } => (String::new(), format!("{message}\n")),
        Response::Started { .. } => (String::new(), String::new()),
        Response::Output { host, stdout, stderr, exit_code, duration_ms } => {
            let code = exit_code.map(|c| format!("exit {c}")).unwrap_or_else(|| "no exit code".into());
            let mut out = format!("── {host} ({code}, {duration_ms} ms)\n{stdout}");
            if !stdout.is_empty() && !stdout.ends_with('\n') {
                out.push('\n');
            }
            (out, stderr.clone())
        }
        Response::Failed { host, message } => (String::new(), format!("── {host}: {message}\n")),
        Response::Done { .. } => (String::new(), String::new()),
        Response::Error { message } => (String::new(), format!("sshvault: {message}\n")),
    }
}

/// Exit code for a finished conversation.
pub fn exit_code(responses: &[Response]) -> i32 {
    if responses.iter().any(|r| matches!(r, Response::Error { .. })) {
        return 1;
    }
    let failed = responses.iter().any(|r| match r {
        Response::Failed { .. } => true,
        Response::Output { exit_code, .. } => *exit_code != Some(0),
        Response::Done { ok } => !ok,
        _ => false,
    });
    i32::from(failed)
}

/// Entry point for `sshvault <command>`. Returns the process exit code.
pub fn client_main(args: &[String]) -> i32 {
    let cli = match parse(args) {
        Ok(Some(c)) => c,
        Ok(None) => {
            print!("{USAGE}");
            return 0;
        }
        Err(e) => {
            eprintln!("sshvault: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let path = endpoint();
    let (r, mut w) = match connect_endpoint(&path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!(
                "sshvault: can't reach the app. Start SSHVault, unlock the vault, and turn on \
                 Settings → Command line."
            );
            return 3;
        }
    };
    let Ok(mut line) = serde_json::to_vec(&cli.request) else { return 2 };
    line.push(b'\n');
    if w.write_all(&line).and_then(|_| w.flush()).is_err() {
        eprintln!("sshvault: the app closed the connection");
        return 3;
    }
    let mut all = Vec::new();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    for l in BufReader::new(r).lines() {
        let Ok(l) = l else { break };
        let Ok(resp) = serde_json::from_str::<Response>(&l) else { continue };
        if cli.json {
            let _ = writeln!(stdout.lock(), "{l}");
        } else {
            let (o, e) = render(&resp);
            let _ = stdout.lock().write_all(o.as_bytes());
            let _ = stderr.lock().write_all(e.as_bytes());
        }
        all.push(resp);
    }
    exit_code(&all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        std::iter::once("sshvault".to_string()).chain(s.split_whitespace().map(str::to_string)).collect()
    }

    #[test]
    fn parses_commands() {
        assert!(is_cli(&args("run x -- y")));
        assert!(!is_cli(&args("")));
        assert!(!is_cli(&args("--some-tauri-flag")));
        assert_eq!(parse(&args("status")).unwrap().unwrap().request, Request::Status);
        let c = parse(&args("run web-01 db-01 --timeout 5 --json -- df -h /")).unwrap().unwrap();
        assert!(c.json);
        assert_eq!(
            c.request,
            Request::Run {
                select: Selector { hosts: vec!["web-01".into(), "db-01".into()], group: None, tag: None },
                command: "df -h /".into(),
                timeout_secs: 5,
            }
        );
        // Options after -- belong to the remote command.
        let c = parse(&args("run -g Prod -- ls --color")).unwrap().unwrap();
        assert!(matches!(c.request, Request::Run { ref command, .. } if command == "ls --color"));
        assert!(parse(&args("run -- uptime")).unwrap_err().contains("never defaults"));
        assert!(parse(&args("run web-01")).is_err());
        assert!(parse(&args("connect")).is_err());
        assert!(parse(&args("list --bogus")).is_err());
        assert!(parse(&args("help")).unwrap().is_none());
        assert!(parse(&args("list -h")).unwrap().is_none());
        // A flag after -- belongs to the remote command, even --json.
        let c = parse(&args("run web-01 -- grep --json x")).unwrap().unwrap();
        assert!(!c.json);
    }

    fn host(label: &str, group: &str, tags: &[&str]) -> (uuid::Uuid, Host) {
        (
            uuid::Uuid::new_v4(),
            Host { label: label.into(), hostname: format!("{label}.example"), group: group.into(), tags: tags.iter().map(|t| t.to_string()).collect(), ..Default::default() },
        )
    }

    #[test]
    fn selects_hosts_and_refuses_typos() {
        let hosts = vec![
            host("web-01", "Production/Web", &["frontend"]),
            host("db-01", "Production/DB", &[]),
            host("dev", "Development", &["frontend"]),
        ];
        let labels = |sel: Selector| select(&hosts, &sel).unwrap().iter().map(|h| h.1.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(Selector { group: Some("production".into()), ..Default::default() }), ["db-01", "web-01"]);
        assert_eq!(labels(Selector { tag: Some("FRONTEND".into()), ..Default::default() }), ["dev", "web-01"]);
        assert_eq!(labels(Selector { group: Some("Production".into()), tag: Some("frontend".into()), ..Default::default() }), ["web-01"]);
        assert_eq!(labels(Selector { hosts: vec!["WEB-01".into()], ..Default::default() }), ["web-01"]);
        let id = hosts[1].0.to_string();
        assert_eq!(labels(Selector { hosts: vec![id[..8].into()], ..Default::default() }), ["db-01"]);
        assert!(select(&hosts, &Selector { hosts: vec!["web-1".into()], ..Default::default() }).unwrap_err().contains("no host"));
        // "Prod" is not "Production": groups match whole path segments.
        assert!(labels(Selector { group: Some("Prod".into()), ..Default::default() }).is_empty());
    }

    #[test]
    fn rendering_and_exit_codes() {
        let out = Response::Output { host: "web-01".into(), stdout: "up 3 days".into(), stderr: String::new(), exit_code: Some(0), duration_ms: 12 };
        assert_eq!(render(&out).0, "── web-01 (exit 0, 12 ms)\nup 3 days\n");
        assert_eq!(exit_code(&[out.clone(), Response::Done { ok: true }]), 0);
        let bad = Response::Output { host: "db".into(), stdout: String::new(), stderr: "boom".into(), exit_code: Some(2), duration_ms: 1 };
        assert_eq!(exit_code(&[bad]), 1);
        assert_eq!(exit_code(&[Response::Error { message: "x".into() }]), 1);
    }

    #[cfg(unix)]
    struct Echo;
    #[cfg(unix)]
    impl Handler for Echo {
        fn handle(&self, req: Request, out: mpsc::UnboundedSender<Response>) -> BoxFuture<()> {
            Box::pin(async move {
                if let Request::Connect { host } = req {
                    let _ = out.send(Response::Opened { label: host });
                }
                let _ = out.send(Response::Done { ok: true });
            })
        }
    }

    /// Answers `run` with one good and one failing host.
    #[cfg(unix)]
    struct Fleet;
    #[cfg(unix)]
    impl Handler for Fleet {
        fn handle(&self, req: Request, out: mpsc::UnboundedSender<Response>) -> BoxFuture<()> {
            Box::pin(async move {
                if let Request::Run { command, .. } = req {
                    let _ = out.send(Response::Output { host: "a".into(), stdout: format!("ran {command}"), stderr: String::new(), exit_code: Some(0), duration_ms: 1 });
                    if command.contains("fail") {
                        let _ = out.send(Response::Failed { host: "b".into(), message: "unreachable".into() });
                    }
                }
                let _ = out.send(Response::Done { ok: true });
            })
        }
    }

    /// The real `sshvault …` entry point against a live socket.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cli_exit_codes_end_to_end() {
        let dir = tempfile::TempDir::new().unwrap();
        // endpoint() follows XDG_RUNTIME_DIR, like the app does.
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        let cli = |a: &'static str| tokio::task::spawn_blocking(move || client_main(&args(a)));
        assert_eq!(cli("status").await.unwrap(), 3, "nothing listening yet");
        let handle = start(Arc::new(Fleet)).unwrap();
        assert_eq!(cli("run web-01 -- uptime").await.unwrap(), 0);
        assert_eq!(cli("run web-01 -- fail please").await.unwrap(), 1);
        assert_eq!(cli("run web-01").await.unwrap(), 2, "usage error");
        drop(handle);
        assert_eq!(cli("status").await.unwrap(), 3);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn socket_round_trip_and_private_folder() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let sock_dir = dir.path().join("ctl");
        let handle = start_at(&sock_dir, Arc::new(Echo)).unwrap();
        let path = sock_dir.join("control.sock");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

        let p = path.clone();
        let lines = tokio::task::spawn_blocking(move || {
            let (r, mut w) = connect_endpoint(p.to_str().unwrap()).unwrap();
            w.write_all(b"{\"cmd\":\"connect\",\"host\":\"web-01\"}\n").unwrap();
            BufReader::new(r).lines().map_while(Result::ok).collect::<Vec<_>>()
        })
        .await
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"opened\"") && lines[0].contains("web-01"));

        // Garbage gets an error, not a crash.
        let p = path.clone();
        let lines = tokio::task::spawn_blocking(move || {
            let (r, mut w) = connect_endpoint(p.to_str().unwrap()).unwrap();
            w.write_all(b"not json\n").unwrap();
            BufReader::new(r).lines().map_while(Result::ok).collect::<Vec<_>>()
        })
        .await
        .unwrap();
        assert!(lines[0].contains("bad request"));
        drop(handle);
        assert!(!path.exists());

        // A folder others can write to is refused.
        let open = dir.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        // create_private_dir tightens our own folder; simulate one we can't fix by checking a symlink.
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&open, &link).unwrap();
        assert!(check_private_dir(&link).is_err());
    }
}
