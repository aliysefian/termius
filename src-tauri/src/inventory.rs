//! Finding hosts to add: running a cloud or tool's own command-line program (so its sign-in stays in that
//! program and never reaches this app), and scanning a private network for SSH servers.
//!
//! Nothing is run that isn't in the table below, the only values passed on are checked against a strict pattern,
//! and what a program prints is parsed by the window (see `src/lib/inventory`), never executed.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

/// What a program may print, at most.
pub const MAX_OUTPUT: usize = 32 * 1024 * 1024;
pub const RUN_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InventoryError {
    #[error("{0} isn't installed here (or isn't on the PATH); install it and sign in with it, then try again")]
    NotInstalled(String),
    #[error("{program} failed: {why}")]
    Failed { program: String, why: String },
    #[error("{0} took longer than 90 seconds")]
    Timeout(String),
    #[error("{0} printed more than 32 MB")]
    TooLarge(String),
    #[error("{0}")]
    BadOption(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Tailscale,
    Aws,
    Gcp,
    Azure,
    DigitalOcean,
    Hetzner,
    Kubernetes,
    Terraform,
}

/// A value passed to a program (a region, a project, a context): letters, digits and `._:/@-` only, so it can
/// never be taken for an option or carry anything else.
pub fn check_option(value: &str, what: &str) -> Result<(), InventoryError> {
    let ok = !value.is_empty() && value.len() <= 100 && !value.starts_with('-') && value.chars().all(|c| c.is_ascii_alphanumeric() || "._:/@-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(InventoryError::BadOption(format!("{what} can only have letters, digits and . _ : / @ - in it")))
    }
}

/// The program, its arguments and the folder to run it in, for a source and its optional value.
pub fn command_for(source: Source, option: Option<&str>) -> Result<(String, Vec<String>, Option<String>), InventoryError> {
    let option = option.map(str::trim).filter(|o| !o.is_empty());
    let mut args: Vec<String> = Vec::new();
    let mut cwd = None;
    let program = match source {
        Source::Tailscale => {
            args.extend(["status", "--json"].map(String::from));
            "tailscale"
        }
        Source::Aws => {
            args.extend(["ec2", "describe-instances", "--output", "json"].map(String::from));
            if let Some(region) = option {
                check_option(region, "The region")?;
                args.extend(["--region".into(), region.into()]);
            }
            "aws"
        }
        Source::Gcp => {
            args.extend(["compute", "instances", "list", "--format=json"].map(String::from));
            if let Some(project) = option {
                check_option(project, "The project")?;
                args.push(format!("--project={project}"));
            }
            if cfg!(windows) { "gcloud.cmd" } else { "gcloud" }
        }
        Source::Azure => {
            args.extend(["vm", "list", "-d", "-o", "json"].map(String::from));
            if let Some(sub) = option {
                check_option(sub, "The subscription")?;
                args.extend(["--subscription".into(), sub.into()]);
            }
            if cfg!(windows) { "az.cmd" } else { "az" }
        }
        Source::DigitalOcean => {
            args.extend(["compute", "droplet", "list", "-o", "json"].map(String::from));
            if let Some(ctx) = option {
                check_option(ctx, "The context")?;
                args.extend(["--context".into(), ctx.into()]);
            }
            "doctl"
        }
        Source::Hetzner => {
            args.extend(["server", "list", "-o", "json"].map(String::from));
            if let Some(ctx) = option {
                check_option(ctx, "The context")?;
                args.extend(["--context".into(), ctx.into()]);
            }
            "hcloud"
        }
        Source::Kubernetes => {
            args.extend(["get", "nodes", "-o", "json", "--request-timeout=20s"].map(String::from));
            if let Some(ctx) = option {
                check_option(ctx, "The context")?;
                args.extend(["--context".into(), ctx.into()]);
            }
            "kubectl"
        }
        Source::Terraform => {
            args.extend(["show", "-json", "-no-color"].map(String::from));
            let dir = option.ok_or_else(|| InventoryError::BadOption("choose the folder with the Terraform configuration".into()))?;
            if dir.contains('\0') || !Path::new(dir).is_dir() {
                return Err(InventoryError::BadOption("that isn't a folder".into()));
            }
            cwd = Some(dir.to_string());
            "terraform"
        }
    };
    Ok((program.to_string(), args, cwd))
}

#[cfg(windows)]
fn no_window(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(0x0800_0000);
}
#[cfg(not(windows))]
fn no_window(_: &mut tokio::process::Command) {}

/// Run `program` and return what it printed. Standard input is closed, the time and the size are capped.
pub async fn run_program(program: &str, args: &[String], cwd: Option<&str>, timeout: Duration) -> Result<String, InventoryError> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    // Folders a program started from a desktop launcher lacks (Homebrew, Snap).
    if cfg!(unix) {
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{path}:/usr/local/bin:/opt/homebrew/bin:/snap/bin"));
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            InventoryError::NotInstalled(program.trim_end_matches(".cmd").to_string())
        } else {
            InventoryError::Failed { program: program.to_string(), why: e.to_string() }
        }
    })?;
    let mut stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    // What the program says about a failure is kept (its first 8 KB); the rest is drained so it can never block.
    let err_task = tokio::spawn(async move {
        let mut err = Vec::new();
        let mut stderr = stderr;
        let _ = (&mut stderr).take(8192).read_to_end(&mut err).await;
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        err
    });
    let name = program.trim_end_matches(".cmd").to_string();
    let work = async {
        let mut out = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let n = stdout.read(&mut chunk).await.map_err(|e| InventoryError::Failed { program: name.clone(), why: e.to_string() })?;
            if n == 0 {
                break;
            }
            if out.len() + n > MAX_OUTPUT {
                let _ = child.kill().await;
                return Err(InventoryError::TooLarge(name.clone()));
            }
            out.extend_from_slice(&chunk[..n]);
        }
        let status = child.wait().await.map_err(|e| InventoryError::Failed { program: name.clone(), why: e.to_string() })?;
        Ok((status, out))
    };
    let (status, out) = match tokio::time::timeout(timeout, work).await {
        Err(_) => return Err(InventoryError::Timeout(name)),
        Ok(r) => r?,
    };
    if status.success() {
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let err = tokio::time::timeout(Duration::from_secs(1), err_task).await.ok().and_then(Result::ok).unwrap_or_default();
    let why = String::from_utf8_lossy(&err).trim().to_string();
    let why = if why.is_empty() { format!("it exited with {}", status.code().map_or("a signal".to_string(), |c| c.to_string())) } else { why.chars().take(600).collect() };
    Err(InventoryError::Failed { program: name, why })
}

pub async fn run(source: Source, option: Option<&str>) -> Result<String, InventoryError> {
    let (program, args, cwd) = command_for(source, option)?;
    run_program(&program, &args, cwd.as_deref(), RUN_TIMEOUT).await
}

// -- scanning a network for SSH servers -------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScanError {
    #[error("{0}")]
    Range(String),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScanHit {
    pub ip: String,
    pub banner: String,
}

pub const MAX_HOSTS: u32 = 4096;

/// `a.b.c.d/n` (or one address) as the first address and how many there are, only for private networks: scanning
/// somebody else's addresses is not something this app does.
pub fn parse_range(text: &str) -> Result<(u32, u32), ScanError> {
    let text = text.trim();
    let (addr, prefix) = match text.split_once('/') {
        Some((a, p)) => (a, p.parse::<u32>().map_err(|_| ScanError::Range("the part after / must be a number from 0 to 32".into()))?),
        None => (text, 32),
    };
    let ip: Ipv4Addr = addr.parse().map_err(|_| ScanError::Range("that isn't an IPv4 address or range like 192.168.1.0/24".into()))?;
    if prefix > 32 {
        return Err(ScanError::Range("the part after / must be a number from 0 to 32".into()));
    }
    let size = if prefix == 0 { u64::from(u32::MAX) + 1 } else { 1u64 << (32 - prefix) };
    if size > u64::from(MAX_HOSTS) {
        return Err(ScanError::Range(format!("that is more than {MAX_HOSTS} addresses; use a /20 or smaller")));
    }
    let size = size as u32;
    let first = u32::from(ip) & !(size - 1);
    let last = first + (size - 1);
    // Both ends inside one private block.
    let inside = |net: u32, bits: u32| {
        let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
        first & mask == net && last & mask == net
    };
    let private = inside(u32::from(Ipv4Addr::new(10, 0, 0, 0)), 8)
        || inside(u32::from(Ipv4Addr::new(172, 16, 0, 0)), 12)
        || inside(u32::from(Ipv4Addr::new(192, 168, 0, 0)), 16)
        || inside(u32::from(Ipv4Addr::new(100, 64, 0, 0)), 10)
        || inside(u32::from(Ipv4Addr::new(169, 254, 0, 0)), 16)
        || inside(u32::from(Ipv4Addr::new(127, 0, 0, 0)), 8);
    if !private {
        return Err(ScanError::Range("only private networks can be scanned (10.x, 172.16 to 172.31, 192.168.x, 100.64 to 100.127, 169.254.x)".into()));
    }
    Ok((first, size))
}

/// Which addresses in `range` have an SSH server on `port`: a connection that answers with an `SSH-` banner.
pub async fn scan_ssh(range: &str, port: u16, wait: Duration) -> Result<Vec<ScanHit>, ScanError> {
    let (first, size) = parse_range(range)?;
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(128));
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..size {
        let ip = Ipv4Addr::from(first + n);
        let gate = gate.clone();
        tasks.spawn(async move {
            let _permit = gate.acquire().await.ok()?;
            let mut stream = tokio::time::timeout(wait, tokio::net::TcpStream::connect(SocketAddr::from((ip, port)))).await.ok()?.ok()?;
            let mut buf = [0u8; 255];
            let n = tokio::time::timeout(wait, stream.read(&mut buf)).await.ok()?.ok()?;
            let banner = String::from_utf8_lossy(&buf[..n]);
            let line = banner.lines().next().unwrap_or("").trim();
            line.starts_with("SSH-").then(|| ScanHit { ip: ip.to_string(), banner: line.chars().take(100).collect() })
        });
    }
    let mut hits = Vec::new();
    while let Some(done) = tasks.join_next().await {
        if let Ok(Some(hit)) = done {
            hits.push(hit);
        }
    }
    hits.sort_by_key(|h| h.ip.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0));
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_source_runs_one_fixed_program() {
        let (p, a, _) = command_for(Source::Tailscale, None).unwrap();
        assert_eq!((p.as_str(), a), ("tailscale", vec!["status".to_string(), "--json".into()]));
        let (p, a, _) = command_for(Source::Aws, Some("eu-west-1")).unwrap();
        assert_eq!(p, "aws");
        assert_eq!(a.join(" "), "ec2 describe-instances --output json --region eu-west-1");
        let (_, a, _) = command_for(Source::Gcp, Some("my-proj")).unwrap();
        assert!(a.contains(&"--project=my-proj".to_string()));
        let (_, a, _) = command_for(Source::Kubernetes, Some("prod")).unwrap();
        assert_eq!(&a[a.len() - 2..], ["--context", "prod"]);
        // An empty value is no value.
        assert_eq!(command_for(Source::Aws, Some("  ")).unwrap().1.len(), 4);
    }

    #[test]
    fn a_value_that_could_be_an_option_or_more_is_refused() {
        for bad in ["--profile", "-x", "a b", "a;b", "$(x)", "a\nb", "a`b`", "é", &"x".repeat(101)] {
            assert!(matches!(command_for(Source::Aws, Some(bad)), Err(InventoryError::BadOption(_))), "{bad:?}");
        }
        for ok in ["eu-west-1", "my-project-123", "arn:aws:x", "user@example.com", "prod.cluster"] {
            assert!(check_option(ok, "x").is_ok(), "{ok}");
        }
    }

    #[test]
    fn terraform_needs_a_real_folder() {
        assert!(matches!(command_for(Source::Terraform, None), Err(InventoryError::BadOption(_))));
        assert!(matches!(command_for(Source::Terraform, Some("/no/such/folder")), Err(InventoryError::BadOption(_))));
        let dir = tempfile::TempDir::new().unwrap();
        let (p, a, cwd) = command_for(Source::Terraform, Some(&dir.path().to_string_lossy())).unwrap();
        assert_eq!((p.as_str(), a.join(" ")), ("terraform", "show -json -no-color".to_string()));
        assert_eq!(cwd.as_deref(), Some(dir.path().to_string_lossy().as_ref()));
    }

    #[cfg(unix)]
    fn sh(script: &str) -> (String, Vec<String>) {
        ("sh".into(), vec!["-c".into(), script.into()])
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_program_is_run_and_what_it_prints_comes_back() {
        let (p, a) = sh("printf '{\"a\":1}'");
        assert_eq!(run_program(&p, &a, None, Duration::from_secs(5)).await.unwrap(), "{\"a\":1}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_program_says_why_and_a_missing_one_says_so() {
        let (p, a) = sh("echo 'not signed in' >&2; exit 3");
        match run_program(&p, &a, None, Duration::from_secs(5)).await {
            Err(InventoryError::Failed { why, .. }) => assert_eq!(why, "not signed in"),
            other => panic!("{other:?}"),
        }
        let (p, a) = sh("exit 4");
        assert!(matches!(run_program(&p, &a, None, Duration::from_secs(5)).await, Err(InventoryError::Failed { why, .. }) if why.contains('4')));
        assert_eq!(run_program("sshvault-no-such-program", &[], None, Duration::from_secs(5)).await, Err(InventoryError::NotInstalled("sshvault-no-such-program".into())));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_slow_program_is_stopped() {
        let (p, a) = sh("sleep 5");
        let started = std::time::Instant::now();
        assert!(matches!(run_program(&p, &a, None, Duration::from_millis(200)).await, Err(InventoryError::Timeout(_))));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn too_much_output_is_refused() {
        let (p, a) = sh("head -c 40000000 /dev/zero");
        assert!(matches!(run_program(&p, &a, None, Duration::from_secs(20)).await, Err(InventoryError::TooLarge(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_program_runs_in_the_folder_it_was_given_and_has_no_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let (p, a) = sh("pwd; cat");
        let out = run_program(&p, &a, Some(&dir.path().to_string_lossy()), Duration::from_secs(5)).await.unwrap();
        assert_eq!(std::fs::canonicalize(out.trim()).unwrap(), std::fs::canonicalize(dir.path()).unwrap(), "and cat read nothing instead of waiting");
    }

    #[test]
    fn ranges_are_private_and_small() {
        assert_eq!(parse_range("192.168.1.0/24"), Ok((u32::from(Ipv4Addr::new(192, 168, 1, 0)), 256)));
        assert_eq!(parse_range("192.168.1.77/24").unwrap().0, u32::from(Ipv4Addr::new(192, 168, 1, 0)), "the network, whatever address is given");
        assert_eq!(parse_range("10.1.2.3"), Ok((u32::from(Ipv4Addr::new(10, 1, 2, 3)), 1)));
        assert_eq!(parse_range("10.0.0.0/20").unwrap().1, 4096);
        assert!(parse_range("10.0.0.0/19").is_err(), "more than 4096");
        assert!(parse_range("8.8.8.0/24").is_err(), "not private");
        assert!(parse_range("0.0.0.0/0").is_err());
        assert!(parse_range("172.15.0.0/24").is_err() && parse_range("172.32.0.0/24").is_err() && parse_range("172.31.255.0/24").is_ok());
        assert!(parse_range("100.64.0.0/24").is_ok() && parse_range("100.128.0.0/24").is_err());
        assert!(parse_range("192.168.1.0/33").is_err() && parse_range("192.168.1.0/x").is_err() && parse_range("nope").is_err() && parse_range("").is_err());
    }

    #[tokio::test]
    async fn only_addresses_that_answer_with_an_ssh_banner_are_found() {
        let ssh = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = ssh.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = ssh.accept().await {
                let _ = tokio::io::AsyncWriteExt::write_all(&mut s, b"SSH-2.0-OpenSSH_9.6\r\n").await;
            }
        });
        let hits = scan_ssh("127.0.0.1", port, Duration::from_millis(500)).await.unwrap();
        assert_eq!(hits, vec![ScanHit { ip: "127.0.0.1".into(), banner: "SSH-2.0-OpenSSH_9.6".into() }]);

        // Something that answers but isn't SSH, and something that doesn't answer at all.
        let web = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let web_port = web.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = web.accept().await {
                let _ = tokio::io::AsyncWriteExt::write_all(&mut s, b"HTTP/1.1 400 Bad Request\r\n").await;
            }
        });
        assert!(scan_ssh("127.0.0.1", web_port, Duration::from_millis(300)).await.unwrap().is_empty());
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(scan_ssh("127.0.0.1", closed, Duration::from_millis(300)).await.unwrap().is_empty());
        assert!(scan_ssh("8.8.8.8", 22, Duration::from_millis(300)).await.is_err());
    }
}
