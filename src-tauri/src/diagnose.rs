//! Connection diagnostics: why can't I reach this host, or why is it slow? Resolves the name, times the TCP
//! connection and the SSH key exchange, traces the route, and (when there are credentials) tries the login, then
//! names the most likely place the connection fails. No shell is opened and nothing is run on the host.
//!
//! Tauri-agnostic: [`run`] reports each stage to a callback as it finishes, so a view can fill in as it goes.
//! Nothing here contacts a third party: the only traffic is to the host itself, to the hops on the way to it, and
//! to this computer's own resolver.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::client;
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::models::ProxySpec;
use crate::ssh::{SshError, Target};

const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const PTR_TIMEOUT: Duration = Duration::from_secs(4);
const TCP_TIMEOUT: Duration = Duration::from_secs(6);
const PROXY_TCP_TIMEOUT: Duration = Duration::from_secs(10);
const BANNER_TIMEOUT: Duration = Duration::from_secs(3);
const KEX_TIMEOUT: Duration = Duration::from_secs(10);
const ROUTE_TIMEOUT: Duration = Duration::from_secs(25);
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);
/// Addresses whose reverse name is looked up.
const MAX_PTR: usize = 4;
const MAX_HOPS: u32 = 20;

/// Where one hop of the way to the host is.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Place {
    pub host: String,
    pub port: u16,
    /// What the person calls it.
    pub label: String,
}

/// What to diagnose.
#[derive(Debug, Clone)]
pub struct Subject {
    /// The host the person wants to reach.
    pub target: Place,
    /// What this computer reaches directly: the first jump host, or the host itself when there are none.
    pub first_hop: Place,
    /// The first hop is a bastion in front of the host, not the host.
    pub via_bastion: bool,
    pub proxy: Option<ProxySpec>,
    pub legacy_algorithms: bool,
    /// Trace the route (a few seconds, and it runs another program). Off in tests that don't want it.
    pub trace_route: bool,
}

// -- findings -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Address {
    pub ip: String,
    /// "A" for IPv4, "AAAA" for IPv6.
    pub record: &'static str,
    /// The name the address reverses to (PTR), if it has one.
    pub ptr: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DnsReport {
    pub name: String,
    /// The host was given as an address, so nothing was looked up.
    pub literal: bool,
    pub addresses: Vec<Address>,
    pub resolve_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TcpFailure {
    /// Something answered and said no: nothing listens on the port.
    Refused,
    /// No answer in time.
    Timeout,
    /// The network said it could not get there, or the dial failed another way.
    Unreachable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TcpReport {
    pub addr: String,
    /// Time for the TCP handshake.
    pub connect_ms: Option<u64>,
    pub failure: Option<TcpFailure>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SshReport {
    /// The server's identification line, e.g. `SSH-2.0-OpenSSH_9.6p1`.
    pub banner: Option<String>,
    /// Time from the TCP connection being up to the banner arriving.
    pub banner_ms: Option<u64>,
    /// Time for the key exchange, on a second connection (the banner's one is closed unused).
    pub kex_ms: Option<u64>,
    pub algorithm: Option<String>,
    pub fingerprint: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RouteHop {
    pub hop: u32,
    /// `None` where the hop did not answer.
    pub ip: Option<String>,
    /// Share of probes that got no answer, where the tool says (mtr, traceroute).
    pub loss_pct: Option<f32>,
    pub avg_ms: Option<f32>,
    pub sent: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RouteReport {
    /// The program that traced it, or `None` if none was available.
    pub tool: Option<String>,
    pub hops: Vec<RouteHop>,
    /// The last hop is the host itself.
    pub reached: bool,
    pub error: Option<String>,
}

/// What went wrong with the login, grouped by what the person can do about it.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthFailure {
    /// The login was refused: wrong key, password or user.
    Rejected,
    /// A bastion in front of the host could not be reached or refused the login.
    Bastion,
    /// The bastion is fine but could not reach the host behind it.
    BehindBastion,
    HostKeyUntrusted,
    HostKeyChanged,
    Timeout,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthReport {
    pub ok: bool,
    pub total_ms: Option<u64>,
    pub failure: Option<AuthFailure>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Healthy,
    NameNotResolved,
    LocalNetwork,
    IntermediateHop,
    BastionUnreachable,
    TargetPortClosed,
    AuthRejected,
    HostKeyProblem,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Verdict {
    pub kind: VerdictKind,
    pub title: String,
    pub detail: String,
}

/// One stage's result, as it arrives.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Event {
    Dns { report: DnsReport },
    Tcp { report: TcpReport },
    Ssh { report: SshReport },
    /// The route trace has started (it takes a few seconds).
    RouteStarted,
    Route { report: RouteReport },
    Auth { report: AuthReport },
    Verdict { verdict: Verdict },
}

// -- running it -------------------------------------------------------------------------------------------------

/// Run every stage, reporting each to `emit` when it finishes, and return the verdict. `login` is the target to try
/// a login on (with its stored credentials and a host-key policy that never asks); `None` skips that stage.
pub async fn run(subject: Subject, login: Option<Target>, emit: impl Fn(Event) + Send + Sync + 'static) -> Verdict {
    let emit = Arc::new(emit);
    let dns = dns_stage(&subject.first_hop.host, subject.first_hop.port).await;
    emit(Event::Dns { report: dns.clone() });
    let first_ip = dns.addresses.first().map(|a| a.ip.clone());

    let tcp_and_ssh = {
        let (subject, emit) = (subject.clone(), Arc::clone(&emit));
        async move {
            let (tcp, ssh) = tcp_and_ssh_stages(&subject, &emit).await;
            (tcp, ssh)
        }
    };
    let route = {
        let emit = Arc::clone(&emit);
        let wanted = subject.trace_route && subject.proxy.is_none();
        async move {
            if !wanted {
                return None;
            }
            let ip = first_ip?;
            emit(Event::RouteStarted);
            let report = route_stage(&ip).await;
            emit(Event::Route { report: report.clone() });
            Some(report)
        }
    };
    let ((tcp, ssh), route) = tokio::join!(tcp_and_ssh, route);

    let auth = match (&login, &tcp, &ssh) {
        (Some(target), tcp, Some(ssh)) if tcp.connect_ms.is_some() && ssh.kex_ms.is_some() => {
            let report = auth_stage(target).await;
            emit(Event::Auth { report: report.clone() });
            Some(report)
        }
        _ => None,
    };

    let verdict = classify(&Findings { subject: &subject, dns: &dns, tcp: Some(&tcp), ssh: ssh.as_ref(), route: route.as_ref(), auth: auth.as_ref() });
    emit(Event::Verdict { verdict: verdict.clone() });
    verdict
}

async fn tcp_and_ssh_stages(subject: &Subject, emit: &Arc<impl Fn(Event) + Send + Sync + 'static>) -> (TcpReport, Option<SshReport>) {
    let hop = &subject.first_hop;
    let addr = format!("{}:{}", hop.host, hop.port);
    let limit = if subject.proxy.is_some() { PROXY_TCP_TIMEOUT } else { TCP_TIMEOUT };
    let started = Instant::now();
    let dialled = tokio::time::timeout(limit, crate::dial::open(&hop.host, hop.port, subject.proxy.as_ref(), "")).await;
    let (mut stream, tcp) = match dialled {
        Ok(Ok(stream)) => {
            let ms = started.elapsed().as_millis() as u64;
            (stream, TcpReport { addr, connect_ms: Some(ms), failure: None, error: None })
        }
        Ok(Err(e)) => {
            let failure = match &e {
                SshError::Connect { source, .. } if source.kind() == std::io::ErrorKind::ConnectionRefused => TcpFailure::Refused,
                _ => TcpFailure::Unreachable,
            };
            let tcp = TcpReport { addr, connect_ms: None, failure: Some(failure), error: Some(e.to_string()) };
            emit(Event::Tcp { report: tcp.clone() });
            return (tcp, None);
        }
        Err(_) => {
            let tcp = TcpReport { addr, connect_ms: None, failure: Some(TcpFailure::Timeout), error: Some(format!("no answer within {} s", limit.as_secs())) };
            emit(Event::Tcp { report: tcp.clone() });
            return (tcp, None);
        }
    };
    emit(Event::Tcp { report: tcp.clone() });

    // The server speaks first in SSH: how long it takes to say who it is, and what it says.
    let banner_started = Instant::now();
    let mut other = None;
    let ssh_banner = tokio::time::timeout(BANNER_TIMEOUT, read_banner(&mut stream, &mut other)).await.ok().flatten();
    let banner_ms = ssh_banner.as_ref().map(|_| banner_started.elapsed().as_millis() as u64);
    // What the server said is worth showing even when it is not an SSH banner.
    let banner = ssh_banner.or(other);
    drop(stream);

    let mut ssh = SshReport { banner: banner.clone(), banner_ms, kex_ms: None, algorithm: None, fingerprint: None, error: None };
    if banner.as_deref().is_some_and(|b| b.starts_with("SSH-")) {
        match kex_probe(subject).await {
            Ok((ms, alg, fp)) => {
                ssh.kex_ms = Some(ms);
                ssh.algorithm = Some(alg);
                ssh.fingerprint = Some(fp);
            }
            Err(e) => ssh.error = Some(e),
        }
    } else {
        ssh.error = Some(match &banner {
            Some(b) => format!("the port answers, but not as SSH: {}", b.chars().take(60).collect::<String>()),
            None => "the port accepts connections but never says it is SSH".to_string(),
        });
    }
    emit(Event::Ssh { report: ssh.clone() });
    (tcp, Some(ssh))
}

/// The server's identification line, without its line break. The protocol allows other lines before it, so reading
/// goes on past them; the first such line is left in `other`, so that a service which is not SSH can be named even
/// when the wait for an SSH line runs out.
async fn read_banner<S: AsyncRead + Unpin>(stream: &mut S, other: &mut Option<String>) -> Option<String> {
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while seen.len() < 512 {
        if stream.read_exact(&mut byte).await.is_err() {
            break;
        }
        if byte[0] == b'\n' {
            let line = String::from_utf8_lossy(&seen).trim_end_matches('\r').to_string();
            if line.starts_with("SSH-") {
                return Some(line);
            }
            if other.is_none() && !line.trim().is_empty() {
                *other = Some(line);
            }
            seen.clear();
            continue;
        }
        seen.push(byte[0]);
    }
    let rest = String::from_utf8_lossy(&seen).trim().to_string();
    if other.is_none() && !rest.is_empty() {
        *other = Some(rest);
    }
    None
}

/// Takes no credentials: a client that connects, exchanges keys (the point at which the server's key is known) and
/// goes away without logging in.
struct KexProbe {
    seen: Arc<Mutex<Option<(String, String)>>>,
}

impl client::Handler for KexProbe {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key: PublicKey = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::new(cert.public_key().clone(), ""),
        };
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some((key.algorithm().to_string(), key.fingerprint(HashAlg::Sha256).to_string()));
        Ok(true)
    }
}

async fn kex_probe(subject: &Subject) -> Result<(u64, String, String), String> {
    let hop = &subject.first_hop;
    let stream = tokio::time::timeout(TCP_TIMEOUT, crate::dial::open(&hop.host, hop.port, subject.proxy.as_ref(), ""))
        .await
        .map_err(|_| "the second connection, for the key exchange, got no answer".to_string())?
        .map_err(|e| e.to_string())?;
    let seen = Arc::new(Mutex::new(None));
    let config = Arc::new(client::Config { preferred: crate::algorithms::preferred(subject.legacy_algorithms), ..Default::default() });
    let started = Instant::now();
    let handle = tokio::time::timeout(KEX_TIMEOUT, client::connect_stream(config, stream, KexProbe { seen: Arc::clone(&seen) }))
        .await
        .map_err(|_| format!("key exchange did not finish within {} s", KEX_TIMEOUT.as_secs()))?
        .map_err(|e| crate::algorithms::explain(&e))?;
    let ms = started.elapsed().as_millis() as u64;
    let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    let (alg, fp) = seen.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or_else(|| "the server never presented a host key".to_string())?;
    Ok((ms, alg, fp))
}

async fn auth_stage(target: &Target) -> AuthReport {
    let started = Instant::now();
    match tokio::time::timeout(AUTH_TIMEOUT, crate::ssh::open_client(target, None)).await {
        Ok(Ok((client, _))) => {
            let total_ms = started.elapsed().as_millis() as u64;
            client.close().await;
            AuthReport { ok: true, total_ms: Some(total_ms), failure: None, error: None }
        }
        Ok(Err(e)) => AuthReport { ok: false, total_ms: None, failure: Some(auth_failure(&e)), error: Some(e.to_string()) },
        Err(_) => AuthReport { ok: false, total_ms: None, failure: Some(AuthFailure::Timeout), error: Some(format!("the login did not finish within {} s", AUTH_TIMEOUT.as_secs())) },
    }
}

/// Which part of a failed connection this error names.
pub fn auth_failure(e: &SshError) -> AuthFailure {
    match e {
        SshError::AuthFailed { .. } | SshError::AgentNoKeys | SshError::NoAgent(_) | SshError::KeyFile { .. } | SshError::Key(_) => AuthFailure::Rejected,
        SshError::HostKeyUnknown { .. } => AuthFailure::HostKeyUntrusted,
        SshError::HostKeyChanged { .. } => AuthFailure::HostKeyChanged,
        SshError::Timeout(_) => AuthFailure::Timeout,
        // A failure on a bastion in front of the host.
        SshError::Jump { source, .. } => match auth_failure(source) {
            AuthFailure::HostKeyUntrusted => AuthFailure::HostKeyUntrusted,
            AuthFailure::HostKeyChanged => AuthFailure::HostKeyChanged,
            _ => AuthFailure::Bastion,
        },
        SshError::JumpUnreachable { .. } => AuthFailure::BehindBastion,
        _ => AuthFailure::Other,
    }
}

// -- DNS --------------------------------------------------------------------------------------------------------

async fn dns_stage(name: &str, port: u16) -> DnsReport {
    let started = Instant::now();
    let literal = name.parse::<IpAddr>().is_ok();
    let found: Result<Vec<IpAddr>, String> = if let Ok(ip) = name.parse::<IpAddr>() {
        Ok(vec![ip])
    } else {
        match tokio::time::timeout(DNS_TIMEOUT, tokio::net::lookup_host((name, port))).await {
            Ok(Ok(addrs)) => {
                let mut ips: Vec<IpAddr> = Vec::new();
                for a in addrs {
                    if !ips.contains(&a.ip()) {
                        ips.push(a.ip());
                    }
                }
                Ok(ips)
            }
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(format!("the resolver did not answer within {} s", DNS_TIMEOUT.as_secs())),
        }
    };
    let resolve_ms = started.elapsed().as_millis() as u64;
    match found {
        Err(error) => DnsReport { name: name.to_string(), literal, addresses: vec![], resolve_ms, error: Some(error) },
        Ok(ips) => {
            // IPv4 first: it is what most of the world's SSH servers answer on.
            let mut ips = ips;
            ips.sort_by_key(|ip| ip.is_ipv6());
            let lookups = ips.iter().take(MAX_PTR).map(|ip| ptr_lookup(*ip));
            let ptrs = futures_util::future::join_all(lookups).await;
            let addresses = ips
                .iter()
                .enumerate()
                .map(|(i, ip)| Address { ip: ip.to_string(), record: if ip.is_ipv4() { "A" } else { "AAAA" }, ptr: ptrs.get(i).cloned().flatten() })
                .collect();
            DnsReport { name: name.to_string(), literal, addresses, resolve_ms, error: None }
        }
    }
}

/// A program to try, its arguments, and how to read what it prints.
type Tool<T> = (&'static str, Vec<String>, fn(&str) -> T);

/// The name an address reverses to, asked of whichever lookup program this computer has. `None` if there is none or
/// no program could say.
async fn ptr_lookup(ip: IpAddr) -> Option<String> {
    let ip = ip.to_string();
    let attempts: [Tool<Option<String>>; 3] = [
        ("dig", vec!["+short".into(), "+time=2".into(), "+tries=1".into(), "-x".into(), ip.clone()], parse_dig_ptr),
        ("host", vec![ip.clone()], parse_host_ptr),
        ("nslookup", vec![ip.clone()], parse_nslookup_ptr),
    ];
    for (program, args, parse) in attempts {
        match run_tool(program, &args, PTR_TIMEOUT).await {
            // The program is not here: try the next.
            Err(ToolError::Missing) => continue,
            Err(_) => return None,
            Ok(out) => return parse(&out),
        }
    }
    None
}

pub fn parse_dig_ptr(out: &str) -> Option<String> {
    out.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with(';')).map(|l| l.trim_end_matches('.').to_string())
}

pub fn parse_host_ptr(out: &str) -> Option<String> {
    out.lines().find_map(|l| l.split("domain name pointer ").nth(1)).map(|n| n.trim().trim_end_matches('.').to_string())
}

pub fn parse_nslookup_ptr(out: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let l = l.trim();
        // Unix: "1.1.168.192.in-addr.arpa	name = router.lan."   Windows: "Name:    router.lan"
        if let Some((_, rest)) = l.split_once("name = ") {
            Some(rest.trim().trim_end_matches('.').to_string())
        } else {
            l.strip_prefix("Name:").map(|rest| rest.trim().to_string()).filter(|n| !n.is_empty())
        }
    })
}

// -- running programs ---------------------------------------------------------------------------------------------

#[derive(Debug)]
enum ToolError {
    /// There is no such program here.
    Missing,
    TimedOut,
    Failed(String),
}

/// Run a program to completion and return its output, or fail when it is missing, errors, or takes too long.
async fn run_tool(program: &str, args: &[String], limit: Duration) -> Result<String, ToolError> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
    // No console window flashing up on Windows.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let child = cmd.output();
    match tokio::time::timeout(limit, child).await {
        Err(_) => Err(ToolError::TimedOut),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Err(ToolError::Missing),
        Ok(Err(e)) => Err(ToolError::Failed(e.to_string())),
        Ok(Ok(out)) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(Ok(out)) => Err(ToolError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string())),
    }
}

// -- the route ----------------------------------------------------------------------------------------------------

async fn route_stage(ip: &str) -> RouteReport {
    let hops = MAX_HOPS.to_string();
    // mtr is first: it reports loss per hop. It needs raw sockets, which some systems give it and some don't; any
    // failure falls through to the next tool.
    let tools: Vec<Tool<Vec<RouteHop>>> = if cfg!(windows) {
        vec![("tracert", vec!["-d".into(), "-h".into(), hops.clone(), "-w".into(), "1000".into(), ip.into()], parse_tracert)]
    } else {
        vec![
            ("mtr", vec!["-r".into(), "-n".into(), "-w".into(), "-c".into(), "3".into(), "-i".into(), "0.4".into(), "-m".into(), hops.clone(), ip.into()], parse_mtr),
            ("traceroute", vec!["-n".into(), "-q".into(), "3".into(), "-w".into(), "1".into(), "-m".into(), hops.clone(), ip.into()], parse_traceroute),
            ("tracepath", vec!["-n".into(), "-m".into(), hops.clone(), ip.into()], parse_tracepath),
        ]
    };
    let mut last_error: Option<String> = None;
    for (program, args, parse) in tools {
        match run_tool(program, &args, ROUTE_TIMEOUT).await {
            Err(ToolError::Missing) => continue,
            Err(ToolError::TimedOut) => {
                last_error = Some(format!("{program} did not finish within {} s", ROUTE_TIMEOUT.as_secs()));
            }
            Err(ToolError::Failed(why)) => {
                last_error = Some(format!("{program}: {}", why.lines().next().unwrap_or("failed")));
            }
            Ok(out) => {
                let hops = parse(&out);
                if hops.is_empty() {
                    last_error = Some(format!("{program} printed nothing this app could read"));
                    continue;
                }
                let reached = hops.last().and_then(|h| h.ip.as_deref()) == Some(ip);
                return RouteReport { tool: Some(program.to_string()), hops, reached, error: None };
            }
        }
    }
    RouteReport { tool: None, hops: vec![], reached: false, error: Some(last_error.unwrap_or_else(|| "no route-tracing program (mtr, traceroute, tracepath, tracert) is installed".into())) }
}

fn num(s: &str) -> Option<f32> {
    s.trim_end_matches('%').parse().ok()
}

/// `mtr -r -n -w`: one line per hop, `  3.|-- 10.0.0.1   0.0%   3   5.1   5.0 ...`.
pub fn parse_mtr(out: &str) -> Vec<RouteHop> {
    let mut hops = Vec::new();
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let Some(first) = it.next() else { continue };
        let Some(hop) = first.strip_suffix(".|--").and_then(|n| n.parse::<u32>().ok()) else { continue };
        let host = it.next();
        // Loss%, Snt, Last, Avg, Best, Wrst, StDev
        let nums: Vec<f32> = it.filter_map(num).collect();
        let ip = host.filter(|h| *h != "???").map(str::to_string);
        let (loss, sent, avg) = (nums.first().copied(), nums.get(1).map(|n| *n as u32), nums.get(3).copied());
        hops.push(RouteHop { hop, avg_ms: if ip.is_some() { avg } else { None }, ip, loss_pct: loss, sent });
    }
    hops
}

/// `traceroute -n -q 3`: ` 3  10.0.0.1  5.1 ms  *  5.3 ms`.
pub fn parse_traceroute(out: &str) -> Vec<RouteHop> {
    let mut hops = Vec::new();
    for line in out.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(hop) = tokens.first().and_then(|t| t.parse::<u32>().ok()) else { continue };
        let mut ip: Option<String> = None;
        let (mut times, mut stars) = (Vec::new(), 0u32);
        let mut i = 1;
        while i < tokens.len() {
            let t = tokens[i];
            if t == "*" {
                stars += 1;
            } else if t == "ms" {
                // handled with the number before it
            } else if let Some(ms) = t.parse::<f32>().ok().filter(|_| tokens.get(i + 1) == Some(&"ms")) {
                times.push(ms);
            } else if t.parse::<IpAddr>().is_ok() && ip.is_none() {
                ip = Some(t.to_string());
            }
            i += 1;
        }
        let sent = times.len() as u32 + stars;
        if sent == 0 {
            continue;
        }
        let avg = (!times.is_empty()).then(|| times.iter().sum::<f32>() / times.len() as f32);
        hops.push(RouteHop { hop, ip, loss_pct: Some(stars as f32 * 100.0 / sent as f32), avg_ms: avg, sent: Some(sent) });
    }
    hops
}

/// Windows `tracert -d`: `  3    5 ms    <1 ms     5 ms  10.0.0.1` and `  4     *        *        *     Request timed out.`
pub fn parse_tracert(out: &str) -> Vec<RouteHop> {
    let mut hops = Vec::new();
    for line in out.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(hop) = tokens.first().and_then(|t| t.parse::<u32>().ok()) else { continue };
        let (mut times, mut stars) = (Vec::new(), 0u32);
        let mut i = 1;
        while i < tokens.len() {
            let t = tokens[i];
            if t == "*" {
                stars += 1;
            } else if t == "ms" {
            } else if let Some(ms) = t.trim_start_matches('<').parse::<f32>().ok().filter(|_| tokens.get(i + 1) == Some(&"ms")) {
                times.push(if t.starts_with('<') { ms / 2.0 } else { ms });
            }
            i += 1;
        }
        let sent = times.len() as u32 + stars;
        if sent == 0 {
            continue;
        }
        let ip = tokens.iter().rev().find(|t| t.parse::<IpAddr>().is_ok()).map(|t| t.to_string());
        let avg = (!times.is_empty()).then(|| times.iter().sum::<f32>() / times.len() as f32);
        hops.push(RouteHop { hop, ip, loss_pct: Some(stars as f32 * 100.0 / sent as f32), avg_ms: avg, sent: Some(sent) });
    }
    hops
}

/// `tracepath -n`: ` 3:  10.0.0.1   5.123ms` (and ` 4:  no reply`). It reports no loss.
pub fn parse_tracepath(out: &str) -> Vec<RouteHop> {
    let mut hops: Vec<RouteHop> = Vec::new();
    for line in out.lines() {
        let line = line.trim_start();
        let Some((head, rest)) = line.split_once(':') else { continue };
        // "1?:" is the sending side, not a hop.
        let Ok(hop) = head.parse::<u32>() else { continue };
        if hops.iter().any(|h| h.hop == hop) {
            continue;
        }
        let rest = rest.trim();
        let ip = rest.split_whitespace().next().filter(|t| t.parse::<IpAddr>().is_ok()).map(str::to_string);
        let avg = rest.split_whitespace().find_map(|t| t.strip_suffix("ms").and_then(|n| n.parse::<f32>().ok()));
        hops.push(RouteHop { hop, ip, loss_pct: None, avg_ms: avg, sent: None });
    }
    hops
}

// -- the verdict -----------------------------------------------------------------------------------------------

pub struct Findings<'a> {
    pub subject: &'a Subject,
    pub dns: &'a DnsReport,
    pub tcp: Option<&'a TcpReport>,
    pub ssh: Option<&'a SshReport>,
    pub route: Option<&'a RouteReport>,
    pub auth: Option<&'a AuthReport>,
}

fn verdict(kind: VerdictKind, title: &str, detail: impl Into<String>) -> Verdict {
    Verdict { kind, title: title.to_string(), detail: detail.into() }
}

/// Name the most likely place the connection fails. A heuristic, and it says so in its wording: every verdict names
/// the evidence it rests on, so a person can overrule it.
pub fn classify(f: &Findings) -> Verdict {
    let hop = &f.subject.first_hop;
    let bastion = f.subject.via_bastion;
    let at = |what: &str| if bastion { format!("bastion {} ({what})", hop.label) } else { what.to_string() };

    if f.dns.error.is_some() && f.dns.addresses.is_empty() {
        return verdict(VerdictKind::NameNotResolved, "Name does not resolve", format!("{} could not be resolved: {}.", f.dns.name, f.dns.error.as_deref().unwrap_or("")));
    }

    let Some(tcp) = f.tcp else {
        return verdict(VerdictKind::Unknown, "Not enough to say", "The connection test did not run.");
    };
    if tcp.connect_ms.is_none() {
        let how = tcp.error.clone().unwrap_or_default();
        if tcp.failure == Some(TcpFailure::Refused) {
            return if bastion {
                verdict(VerdictKind::BastionUnreachable, "Bastion unreachable", format!("Bastion {} refused the connection on port {}: its SSH service is not listening. {how}", hop.label, hop.port))
            } else {
                verdict(VerdictKind::TargetPortClosed, "Target port closed", format!("{}:{} refused the connection: the host is up but nothing listens on that port (SSH is down, or the port differs). {how}", hop.host, hop.port))
            };
        }
        // No answer, or the network said it could not get there: where does the trail go cold?
        let responding: Vec<&RouteHop> = f.route.map(|r| r.hops.iter().filter(|h| h.ip.is_some()).collect()).unwrap_or_default();
        let last = responding.iter().map(|h| h.hop).max().unwrap_or(0);
        let Some(route) = f.route.filter(|r| r.tool.is_some()) else {
            let title = if bastion { "Bastion unreachable" } else { "No answer from the host" };
            let kind = if bastion { VerdictKind::BastionUnreachable } else { VerdictKind::IntermediateHop };
            return verdict(kind, title, format!("{} did not answer ({how}). The route could not be traced, so where it stops is unknown.", at(&hop.host)));
        };
        if last == 0 {
            return verdict(VerdictKind::LocalNetwork, "Local network or ISP drop", "No router on the way answered, not even the first one: this computer's network connection is down or filtered.");
        }
        if route.reached {
            let (kind, title) = if bastion { (VerdictKind::BastionUnreachable, "Bastion unreachable") } else { (VerdictKind::IntermediateHop, "Firewall timeout") };
            return verdict(kind, title, format!("The route reaches {} but port {} gets no answer ({how}): a firewall is most likely dropping SSH.", hop.host, hop.port));
        }
        if last <= 2 {
            return verdict(VerdictKind::LocalNetwork, "Local network or ISP drop", format!("The route answers up to hop {last} (your gateway or your provider's first router) and then goes silent."));
        }
        let place = responding.iter().find(|h| h.hop == last).and_then(|h| h.ip.clone()).unwrap_or_default();
        let (kind, title) = if bastion { (VerdictKind::BastionUnreachable, "Bastion unreachable") } else { (VerdictKind::IntermediateHop, "Intermediate hop or firewall timeout") };
        return verdict(kind, title, format!("The route answers up to hop {last} ({place}) and nothing after it: the link or a firewall after that hop is dropping traffic to {}.", hop.host));
    }

    let Some(ssh) = f.ssh else {
        return verdict(VerdictKind::Unknown, "Not enough to say", "The SSH test did not run.");
    };
    if ssh.kex_ms.is_none() {
        let why = ssh.error.clone().unwrap_or_default();
        let kind = if bastion { VerdictKind::BastionUnreachable } else { VerdictKind::TargetPortClosed };
        let title = if bastion { "Bastion unreachable" } else { "SSH service not answering" };
        return verdict(kind, title, format!("{} accepts connections on port {}, but the SSH handshake did not complete: {why}", at(&hop.host), hop.port));
    }

    match f.auth {
        None => verdict(VerdictKind::Healthy, "Reachable", format!("{} answers and speaks SSH. No login was tried (no stored credentials to use).", at(&hop.host))),
        Some(a) if a.ok => verdict(VerdictKind::Healthy, "Healthy", format!("The login works{}.", a.total_ms.map(|ms| format!(" ({ms} ms from start to finish)")).unwrap_or_default())),
        Some(a) => {
            let why = a.error.clone().unwrap_or_default();
            match a.failure.unwrap_or(AuthFailure::Other) {
                AuthFailure::Rejected => verdict(VerdictKind::AuthRejected, "Authentication or key rejected", format!("The host is reachable but did not accept the login: {why}")),
                AuthFailure::Bastion => verdict(VerdictKind::BastionUnreachable, "Bastion rejected the login", format!("The bastion is reachable but the login through it failed: {why}")),
                AuthFailure::BehindBastion => verdict(VerdictKind::TargetPortClosed, "Target not reachable from the bastion", format!("The bastion works, but it could not reach the host behind it: {why}")),
                AuthFailure::HostKeyUntrusted => verdict(VerdictKind::HostKeyProblem, "Host key not trusted yet", format!("The connection works, but this host's key has not been accepted on this computer yet; connect once to review it. {why}")),
                AuthFailure::HostKeyChanged => verdict(VerdictKind::HostKeyProblem, "Host key changed", format!("The host presents a different key than before: it may have been reinstalled, or something is intercepting the connection. {why}")),
                AuthFailure::Timeout => verdict(VerdictKind::Unknown, "Login timed out", format!("The host is reachable and speaks SSH, but the login did not finish in time: {why}")),
                AuthFailure::Other => verdict(VerdictKind::Unknown, "Login failed", why),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(host: &str, port: u16, label: &str) -> Place {
        Place { host: host.into(), port, label: label.into() }
    }

    fn subject(bastion: bool) -> Subject {
        let target = place("10.0.0.9", 22, "web");
        Subject {
            first_hop: if bastion { place("bastion.example", 22, "bastion") } else { target.clone() },
            target,
            via_bastion: bastion,
            proxy: None,
            legacy_algorithms: false,
            trace_route: false,
        }
    }

    fn dns_ok() -> DnsReport {
        DnsReport { name: "x".into(), literal: false, addresses: vec![Address { ip: "203.0.113.5".into(), record: "A", ptr: None }], resolve_ms: 4, error: None }
    }

    fn tcp_fail(f: TcpFailure) -> TcpReport {
        TcpReport { addr: "x:22".into(), connect_ms: None, failure: Some(f), error: Some("boom".into()) }
    }

    fn tcp_ok() -> TcpReport {
        TcpReport { addr: "x:22".into(), connect_ms: Some(12), failure: None, error: None }
    }

    fn ssh_ok() -> SshReport {
        SshReport { banner: Some("SSH-2.0-OpenSSH_9.6".into()), banner_ms: Some(10), kex_ms: Some(40), algorithm: Some("ssh-ed25519".into()), fingerprint: Some("SHA256:x".into()), error: None }
    }

    fn hop(n: u32, ip: Option<&str>) -> RouteHop {
        RouteHop { hop: n, ip: ip.map(str::to_string), loss_pct: Some(if ip.is_some() { 0.0 } else { 100.0 }), avg_ms: ip.map(|_| 5.0), sent: Some(3) }
    }

    fn route(hops: Vec<RouteHop>, reached: bool) -> RouteReport {
        RouteReport { tool: Some("mtr".into()), hops, reached, error: None }
    }

    fn kind(s: &Subject, dns: &DnsReport, tcp: Option<&TcpReport>, ssh: Option<&SshReport>, route: Option<&RouteReport>, auth: Option<&AuthReport>) -> VerdictKind {
        classify(&Findings { subject: s, dns, tcp, ssh, route, auth }).kind
    }

    #[test]
    fn a_name_that_does_not_resolve_is_named_first() {
        let dns = DnsReport { name: "nope.invalid".into(), literal: false, addresses: vec![], resolve_ms: 3, error: Some("no such host".into()) };
        assert_eq!(kind(&subject(false), &dns, None, None, None, None), VerdictKind::NameNotResolved);
    }

    #[test]
    fn a_refusal_means_the_port_is_closed_or_the_bastion_has_no_ssh() {
        let t = tcp_fail(TcpFailure::Refused);
        assert_eq!(kind(&subject(false), &dns_ok(), Some(&t), None, None, None), VerdictKind::TargetPortClosed);
        assert_eq!(kind(&subject(true), &dns_ok(), Some(&t), None, None, None), VerdictKind::BastionUnreachable);
    }

    #[test]
    fn silence_is_placed_by_where_the_route_goes_quiet() {
        let t = tcp_fail(TcpFailure::Timeout);
        // Nothing answers, not even the gateway: this computer's own network.
        let none = route(vec![hop(1, None), hop(2, None)], false);
        assert_eq!(kind(&subject(false), &dns_ok(), Some(&t), None, Some(&none), None), VerdictKind::LocalNetwork);
        // The gateway and the provider's first router answer, then silence.
        let early = route(vec![hop(1, Some("192.168.1.1")), hop(2, Some("10.1.1.1")), hop(3, None)], false);
        assert_eq!(kind(&subject(false), &dns_ok(), Some(&t), None, Some(&early), None), VerdictKind::LocalNetwork);
        // Silence well down the road: a hop or a firewall on the way.
        let mid = route(vec![hop(1, Some("a")), hop(2, Some("b")), hop(3, Some("c")), hop(4, Some("198.51.100.7")), hop(5, None)], false);
        let v = classify(&Findings { subject: &subject(false), dns: &dns_ok(), tcp: Some(&t), ssh: None, route: Some(&mid), auth: None });
        assert_eq!(v.kind, VerdictKind::IntermediateHop);
        assert!(v.detail.contains("hop 4") && v.detail.contains("198.51.100.7"), "{}", v.detail);
        // The host itself answers the trace but not the port: a firewall.
        let up = route(vec![hop(1, Some("a")), hop(2, Some("b")), hop(3, Some("203.0.113.5"))], true);
        assert_eq!(kind(&subject(false), &dns_ok(), Some(&t), None, Some(&up), None), VerdictKind::IntermediateHop);
        // The same silence in front of a bastion is the bastion's.
        assert_eq!(kind(&subject(true), &dns_ok(), Some(&t), None, Some(&mid), None), VerdictKind::BastionUnreachable);
        // A broken local network stays local even with a bastion in the way.
        assert_eq!(kind(&subject(true), &dns_ok(), Some(&t), None, Some(&none), None), VerdictKind::LocalNetwork);
    }

    #[test]
    fn without_a_route_it_says_so_instead_of_guessing_where() {
        let t = tcp_fail(TcpFailure::Timeout);
        let missing = RouteReport { tool: None, hops: vec![], reached: false, error: Some("no tool".into()) };
        let v = classify(&Findings { subject: &subject(false), dns: &dns_ok(), tcp: Some(&t), ssh: None, route: Some(&missing), auth: None });
        assert!(v.detail.contains("could not be traced"), "{}", v.detail);
        let v = classify(&Findings { subject: &subject(false), dns: &dns_ok(), tcp: Some(&t), ssh: None, route: None, auth: None });
        assert!(v.detail.contains("could not be traced"));
    }

    #[test]
    fn an_open_port_that_is_not_ssh_is_a_dead_daemon() {
        let ssh = SshReport { banner: None, banner_ms: None, kex_ms: None, algorithm: None, fingerprint: None, error: Some("never says it is SSH".into()) };
        assert_eq!(kind(&subject(false), &dns_ok(), Some(&tcp_ok()), Some(&ssh), None, None), VerdictKind::TargetPortClosed);
        assert_eq!(kind(&subject(true), &dns_ok(), Some(&tcp_ok()), Some(&ssh), None, None), VerdictKind::BastionUnreachable);
    }

    #[test]
    fn the_login_decides_the_rest() {
        let s = subject(false);
        let (d, t, h) = (dns_ok(), tcp_ok(), ssh_ok());
        let auth = |ok: bool, failure: Option<AuthFailure>| AuthReport { ok, total_ms: ok.then_some(90), failure, error: Some("why".into()) };
        assert_eq!(kind(&s, &d, Some(&t), Some(&h), None, None), VerdictKind::Healthy);
        assert_eq!(kind(&s, &d, Some(&t), Some(&h), None, Some(&auth(true, None))), VerdictKind::Healthy);
        for (failure, want) in [
            (AuthFailure::Rejected, VerdictKind::AuthRejected),
            (AuthFailure::Bastion, VerdictKind::BastionUnreachable),
            (AuthFailure::BehindBastion, VerdictKind::TargetPortClosed),
            (AuthFailure::HostKeyUntrusted, VerdictKind::HostKeyProblem),
            (AuthFailure::HostKeyChanged, VerdictKind::HostKeyProblem),
            (AuthFailure::Timeout, VerdictKind::Unknown),
        ] {
            assert_eq!(kind(&s, &d, Some(&t), Some(&h), None, Some(&auth(false, Some(failure)))), want, "{failure:?}");
        }
    }

    #[test]
    fn ssh_errors_are_sorted_by_what_the_person_can_do() {
        let ssh = |e: SshError| auth_failure(&e);
        assert_eq!(ssh(SshError::AuthFailed { user: "u".into(), host: "h".into(), method: "key" }), AuthFailure::Rejected);
        assert_eq!(ssh(SshError::HostKeyUnknown { host: "h".into(), fingerprint: "f".into() }), AuthFailure::HostKeyUntrusted);
        assert_eq!(ssh(SshError::HostKeyChanged { host: "h".into(), port: 22, fingerprint: "f".into() }), AuthFailure::HostKeyChanged);
        assert_eq!(ssh(SshError::Timeout("h".into())), AuthFailure::Timeout);
        assert_eq!(ssh(SshError::JumpUnreachable { jump: "b".into(), addr: "h".into(), reason: "r".into() }), AuthFailure::BehindBastion);
        let wrap = |inner| SshError::Jump { host: "b".into(), source: Box::new(inner) };
        assert_eq!(ssh(wrap(SshError::AuthFailed { user: "u".into(), host: "b".into(), method: "key" })), AuthFailure::Bastion);
        assert_eq!(ssh(wrap(SshError::Timeout("b".into()))), AuthFailure::Bastion);
        assert_eq!(ssh(wrap(SshError::HostKeyChanged { host: "b".into(), port: 22, fingerprint: "f".into() })), AuthFailure::HostKeyChanged);
    }

    #[test]
    fn mtr_reports_are_read() {
        let out = "Start: 2026-10-18T10:00:00+0000\nHOST: laptop                     Loss%   Snt   Last   Avg  Best  Wrst StDev\n  1.|-- 192.168.1.1                0.0%     3    0.5   0.6   0.5   0.7   0.1\n  2.|-- ???                      100.0     3    0.0   0.0   0.0   0.0   0.0\n  3.|-- 203.0.113.5               33.3%     3   12.1  12.5  11.9  13.4   0.8\n";
        let hops = parse_mtr(out);
        assert_eq!(hops.len(), 3);
        assert_eq!(hops[0], RouteHop { hop: 1, ip: Some("192.168.1.1".into()), loss_pct: Some(0.0), avg_ms: Some(0.6), sent: Some(3) });
        assert_eq!(hops[1].ip, None);
        assert_eq!(hops[1].loss_pct, Some(100.0));
        assert_eq!(hops[1].avg_ms, None);
        assert_eq!(hops[2].loss_pct, Some(33.3));
        assert_eq!(hops[2].avg_ms, Some(12.5));
    }

    #[test]
    fn traceroute_lines_are_read_with_loss_per_hop() {
        let out = "traceroute to 203.0.113.5 (203.0.113.5), 20 hops max, 60 byte packets\n 1  192.168.1.1  0.521 ms  0.498 ms  0.489 ms\n 2  * * *\n 3  10.0.0.1  5.1 ms * 5.3 ms\n 4  203.0.113.5  12.0 ms  12.4 ms  12.2 ms\n";
        let hops = parse_traceroute(out);
        assert_eq!(hops.len(), 4);
        assert_eq!(hops[0].loss_pct, Some(0.0));
        assert!((hops[0].avg_ms.unwrap() - 0.502).abs() < 0.01);
        assert_eq!(hops[1].ip, None);
        assert_eq!(hops[1].loss_pct, Some(100.0));
        assert_eq!(hops[2].ip.as_deref(), Some("10.0.0.1"));
        assert!((hops[2].loss_pct.unwrap() - 33.33).abs() < 0.1);
        assert_eq!(hops[3].ip.as_deref(), Some("203.0.113.5"));
    }

    #[test]
    fn windows_tracert_lines_are_read() {
        let out = "\nTracing route to 203.0.113.5 over a maximum of 20 hops\n\n  1    <1 ms    <1 ms    <1 ms  192.168.1.1\n  2     *        *        *     Request timed out.\n  3     9 ms    10 ms     *     10.0.0.1\n  4    12 ms    12 ms    13 ms  203.0.113.5\n\nTrace complete.\n";
        let hops = parse_tracert(out);
        assert_eq!(hops.iter().map(|h| h.hop).collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!(hops[0].ip.as_deref(), Some("192.168.1.1"));
        assert_eq!(hops[1].ip, None);
        assert_eq!(hops[1].loss_pct, Some(100.0));
        assert!((hops[2].loss_pct.unwrap() - 33.33).abs() < 0.1);
        assert_eq!(hops[2].avg_ms, Some(9.5));
        assert_eq!(hops[3].ip.as_deref(), Some("203.0.113.5"));
    }

    #[test]
    fn tracepath_lines_are_read_once_per_hop() {
        let out = " 1?: [LOCALHOST]                      pmtu 1500\n 1:  192.168.1.1                                           0.521ms\n 1:  192.168.1.1                                           0.498ms\n 2:  no reply\n 3:  10.0.0.1                                              5.1ms asymm  4\n     Resume: pmtu 1500\n";
        let hops = parse_tracepath(out);
        assert_eq!(hops.iter().map(|h| h.hop).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(hops[0].avg_ms, Some(0.521));
        assert_eq!(hops[1].ip, None);
        assert_eq!(hops[2].ip.as_deref(), Some("10.0.0.1"));
        assert!(hops.iter().all(|h| h.loss_pct.is_none()), "tracepath says nothing about loss");
    }

    #[test]
    fn reverse_lookups_are_read_from_each_tool() {
        assert_eq!(parse_dig_ptr("static.5.113.0.203.example.net.\n").as_deref(), Some("static.5.113.0.203.example.net"));
        assert_eq!(parse_dig_ptr(""), None);
        assert_eq!(parse_host_ptr("5.113.0.203.in-addr.arpa domain name pointer web.example.net.\n").as_deref(), Some("web.example.net"));
        assert_eq!(parse_host_ptr("Host 5.113.0.203.in-addr.arpa. not found: 3(NXDOMAIN)\n"), None);
        assert_eq!(parse_nslookup_ptr("5.113.0.203.in-addr.arpa\tname = web.example.net.\n\nAuthoritative answers can be found from:\n").as_deref(), Some("web.example.net"));
        assert_eq!(parse_nslookup_ptr("Server:  dns.lan\nAddress:  192.168.1.1\n\nName:    web.example.net\nAddress:  203.0.113.5\n").as_deref(), Some("web.example.net"));
    }

    #[tokio::test]
    async fn the_banner_is_read_past_lines_that_come_before_it() {
        let mut other = None;
        let mut s: &[u8] = b"Welcome to the machine\r\nSSH-2.0-OpenSSH_9.6p1 Ubuntu\r\n";
        assert_eq!(read_banner(&mut s, &mut other).await.as_deref(), Some("SSH-2.0-OpenSSH_9.6p1 Ubuntu"));
        assert_eq!(other.as_deref(), Some("Welcome to the machine"));
        let mut other = None;
        let mut s: &[u8] = b"220 FTP server ready\r\n";
        assert_eq!(read_banner(&mut s, &mut other).await, None);
        assert_eq!(other.as_deref(), Some("220 FTP server ready"));
    }

    #[tokio::test]
    async fn an_address_is_not_looked_up_and_a_name_that_does_not_exist_says_so() {
        let r = dns_stage("127.0.0.1", 22).await;
        assert!(r.literal && r.error.is_none());
        assert_eq!(r.addresses.len(), 1);
        assert_eq!(r.addresses[0].record, "A");
        let r = dns_stage("::1", 22).await;
        assert_eq!(r.addresses[0].record, "AAAA");
        let r = dns_stage("this-host-does-not-exist.invalid", 22).await;
        assert!(r.addresses.is_empty() && r.error.is_some(), "{r:?}");
    }

    use crate::ssh::testutil::{spawn_sshd, target};

    fn here(port: u16) -> Subject {
        let p = place("127.0.0.1", port, "local");
        Subject { target: p.clone(), first_hop: p, via_bastion: false, proxy: None, legacy_algorithms: false, trace_route: false }
    }

    fn collect() -> (Arc<Mutex<Vec<Event>>>, impl Fn(Event) + Send + Sync + 'static) {
        let seen: Arc<Mutex<Vec<Event>>> = Arc::default();
        let sink = Arc::clone(&seen);
        (seen, move |e| sink.lock().unwrap().push(e))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_healthy_host_is_timed_stage_by_stage_and_the_login_is_tried() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (seen, emit) = collect();
        let login = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let v = run(here(sshd.port), Some(login), emit).await;
        assert_eq!(v.kind, VerdictKind::Healthy, "{v:?}");

        let events = seen.lock().unwrap();
        let stages: Vec<&str> = events
            .iter()
            .map(|e| match e {
                Event::Dns { .. } => "dns",
                Event::Tcp { .. } => "tcp",
                Event::Ssh { .. } => "ssh",
                Event::RouteStarted | Event::Route { .. } => "route",
                Event::Auth { .. } => "auth",
                Event::Verdict { .. } => "verdict",
            })
            .collect();
        assert_eq!(stages, ["dns", "tcp", "ssh", "auth", "verdict"], "each stage reported once, in order, with no route when it is off");
        for e in events.iter() {
            match e {
                Event::Tcp { report } => assert!(report.connect_ms.is_some() && report.failure.is_none()),
                Event::Ssh { report } => {
                    assert!(report.banner.as_deref().is_some_and(|b| b.starts_with("SSH-2.0-")), "{report:?}");
                    assert!(report.kex_ms.is_some() && report.banner_ms.is_some());
                    assert!(report.fingerprint.as_deref().is_some_and(|f| f.starts_with("SHA256:")));
                    assert!(report.algorithm.is_some());
                }
                Event::Auth { report } => assert!(report.ok && report.total_ms.is_some()),
                _ => {}
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_rejected_key_is_an_authentication_problem_not_a_network_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (_, emit) = collect();
        let login = target(&sshd, &sshd.other_key, dir.path().join("kh"));
        let v = run(here(sshd.port), Some(login), emit).await;
        assert_eq!(v.kind, VerdictKind::AuthRejected, "{v:?}");
        // With no credentials to try, the host is only known to be reachable.
        let (_, emit) = collect();
        assert_eq!(run(here(sshd.port), None, emit).await.kind, VerdictKind::Healthy);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_closed_port_is_named_and_a_service_that_is_not_ssh_is_too() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let (seen, emit) = collect();
        let v = run(here(closed), None, emit).await;
        assert_eq!(v.kind, VerdictKind::TargetPortClosed, "{v:?}");
        assert!(seen.lock().unwrap().iter().any(|e| matches!(e, Event::Tcp { report } if report.failure == Some(TcpFailure::Refused))));

        // Something answers on the port, but it is not SSH.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let _ = tokio::io::AsyncWriteExt::write_all(&mut s, b"220 mail.example ESMTP\r\n").await;
            }
        });
        let (seen, emit) = collect();
        let v = run(here(port), None, emit).await;
        assert_eq!(v.kind, VerdictKind::TargetPortClosed, "{v:?}");
        assert!(v.detail.contains("did not complete") || v.detail.contains("handshake"), "{}", v.detail);
        let ssh = seen.lock().unwrap().iter().find_map(|e| if let Event::Ssh { report } = e { Some(report.clone()) } else { None }).unwrap();
        assert!(ssh.error.as_deref().is_some_and(|e| e.contains("not as SSH")), "{ssh:?}");
    }

    #[tokio::test]
    async fn a_port_that_accepts_and_says_nothing_is_reported_after_the_banner_wait() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut keep = Vec::new();
            while let Ok((s, _)) = listener.accept().await {
                keep.push(s);
            }
        });
        let (_, emit) = collect();
        let started = Instant::now();
        let v = run(here(port), None, emit).await;
        assert_eq!(v.kind, VerdictKind::TargetPortClosed, "{v:?}");
        assert!(started.elapsed() < Duration::from_secs(8), "gives up after the banner wait, not forever");
    }

    #[tokio::test]
    async fn the_route_to_this_computer_is_traced_with_whatever_tool_is_installed() {
        let r = route_stage("127.0.0.1").await;
        eprintln!("route tool here: {:?}, hops: {:?}, error: {:?}", r.tool, r.hops, r.error);
        match r.tool {
            Some(_) => {
                assert!(!r.hops.is_empty());
                assert!(r.hops.iter().all(|h| h.hop >= 1));
                assert!(r.reached, "{r:?}");
            }
            None => assert!(r.error.is_some(), "no tool means it says why"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_working_bastion_with_a_dead_target_behind_it_is_told_apart_from_a_dead_bastion() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let kh = dir.path().join("kh");
        let bastion = target(&sshd, &sshd.client_key, kh.clone());
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut behind = bastion.clone();
        behind.port = dead;
        behind.jump = Some(Box::new(bastion.clone()));
        let subject = Subject {
            target: place("127.0.0.1", dead, "app"),
            first_hop: place("127.0.0.1", sshd.port, "bastion"),
            via_bastion: true,
            proxy: None,
            legacy_algorithms: false,
            trace_route: false,
        };
        let (_, emit) = collect();
        let v = run(subject, Some(behind), emit).await;
        assert_eq!(v.kind, VerdictKind::TargetPortClosed, "{v:?}");
        assert!(v.title.contains("from the bastion"), "{v:?}");

        // A bastion that is not there at all.
        let subject = Subject {
            target: place("127.0.0.1", dead, "app"),
            first_hop: place("127.0.0.1", dead, "bastion"),
            via_bastion: true,
            proxy: None,
            legacy_algorithms: false,
            trace_route: false,
        };
        let (_, emit) = collect();
        assert_eq!(run(subject, None, emit).await.kind, VerdictKind::BastionUnreachable);
    }
}
