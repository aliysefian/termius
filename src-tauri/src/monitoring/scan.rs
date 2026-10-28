//! The port scan: which of a host's ports accept a connection, the way `nc -z host port` would tell you, for many ports
//! at once.
//!
//! It runs only when asked, on a host that is already in the monitoring list (the command takes the host's id, never an
//! address, so it can't be pointed at some other machine), for at most 1024 ports, 64 at a time, and one scan at a
//! time. Nothing is sent on a connection that opens: it is closed again at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

pub const MAX_PORTS: usize = 1024;
const PARALLEL: usize = 64;

/// What is usually found on each port, for the ports people usually look at.
pub const COMMON: &[(u16, &str)] = &[
    (21, "ftp"),
    (22, "ssh"),
    (23, "telnet"),
    (25, "smtp"),
    (53, "dns"),
    (80, "http"),
    (110, "pop3"),
    (143, "imap"),
    (443, "https"),
    (445, "smb"),
    (465, "smtps"),
    (587, "submission"),
    (993, "imaps"),
    (995, "pop3s"),
    (1433, "mssql"),
    (1521, "oracle"),
    (2049, "nfs"),
    (3000, "dev server"),
    (3306, "mysql"),
    (3389, "rdp"),
    (5432, "postgresql"),
    (5900, "vnc"),
    (6379, "redis"),
    (8080, "http alt"),
    (8443, "https alt"),
    (9090, "prometheus"),
    (9200, "elasticsearch"),
    (11211, "memcached"),
    (27017, "mongodb"),
];

pub fn service_of(port: u16) -> Option<&'static str> {
    COMMON.iter().find(|(p, _)| *p == port).map(|(_, s)| *s)
}

/// `22, 80, 8000-8100` or `common`, as a sorted list without repeats.
pub fn parse_ports(spec: &str) -> Result<Vec<u16>, String> {
    if spec.len() > 4096 {
        return Err("That list of ports is too long.".into());
    }
    let mut out: Vec<u16> = Vec::new();
    for token in spec.split([',', ';', ' ', '\n', '\t']).map(str::trim).filter(|t| !t.is_empty()) {
        if token.eq_ignore_ascii_case("common") {
            out.extend(COMMON.iter().map(|(p, _)| *p));
            continue;
        }
        let bad = || format!("\"{token}\" is not a port or a range like 8000-8100");
        let (from, to) = match token.split_once('-') {
            Some((a, b)) => (a.trim().parse::<u32>().map_err(|_| bad())?, b.trim().parse::<u32>().map_err(|_| bad())?),
            None => {
                let p = token.parse::<u32>().map_err(|_| bad())?;
                (p, p)
            }
        };
        if from == 0 || to > 65_535 || from > to {
            return Err(bad());
        }
        if (to - from) as usize >= MAX_PORTS {
            return Err(format!("Scan at most {MAX_PORTS} ports at a time."));
        }
        out.extend((from..=to).map(|p| p as u16));
        // Repeats are removed at the end, so stop one that only repeats from growing without limit.
        if out.len() > 8 * MAX_PORTS {
            return Err(format!("Scan at most {MAX_PORTS} ports at a time."));
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err("Name the ports to scan, such as 22, 80, 8000-8100 or common.".into());
    }
    if out.len() > MAX_PORTS {
        return Err(format!("Scan at most {MAX_PORTS} ports at a time."));
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenPort {
    pub port: u16,
    pub service: Option<&'static str>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    pub host: String,
    pub scanned: usize,
    pub open: Vec<OpenPort>,
    pub took_ms: u64,
}

static BUSY: AtomicBool = AtomicBool::new(false);

/// Holds the "a scan is running" flag and clears it when the scan ends, however it ends.
struct Busy;

impl Busy {
    fn take() -> Option<Busy> {
        (!BUSY.swap(true, Ordering::SeqCst)).then_some(Busy)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Try each port, 64 at a time, giving each `per_port` to answer.
pub async fn scan(host: &str, ports: &[u16], per_port: Duration) -> Result<ScanReport, String> {
    if !super::ping::valid_host(host) {
        return Err("that is not a valid host name or address".into());
    }
    if ports.is_empty() || ports.len() > MAX_PORTS {
        return Err(format!("Scan between 1 and {MAX_PORTS} ports."));
    }
    let Some(_busy) = Busy::take() else {
        return Err("A scan is already running. Wait for it to finish.".into());
    };
    let start = Instant::now();
    // Look the name up once, so a name that doesn't exist is an error and not "no port is open".
    let ip = match tokio::time::timeout(Duration::from_secs(10), tokio::net::lookup_host((host, 0))).await {
        Ok(Ok(mut found)) => found.next().map(|a| a.ip()).ok_or_else(|| format!("could not find {host}"))?,
        Ok(Err(e)) => return Err(format!("could not find {host}: {e}")),
        Err(_) => return Err(format!("looking up {host} took too long")),
    };
    let permits = Arc::new(Semaphore::new(PARALLEL));
    let mut set = tokio::task::JoinSet::new();
    for &port in ports {
        let permits = Arc::clone(&permits);
        set.spawn(async move {
            let _permit = permits.acquire_owned().await.ok()?;
            let began = Instant::now();
            match tokio::time::timeout(per_port, TcpStream::connect(std::net::SocketAddr::new(ip, port))).await {
                Ok(Ok(_stream)) => Some(OpenPort { port, service: service_of(port), latency_ms: began.elapsed().as_millis() as u64 }),
                _ => None,
            }
        });
    }
    let mut open: Vec<OpenPort> = Vec::new();
    while let Some(done) = set.join_next().await {
        if let Ok(Some(p)) = done {
            open.push(p);
        }
    }
    open.sort_by_key(|p| p.port);
    Ok(ScanReport { host: host.to_string(), scanned: ports.len(), open, took_ms: start.elapsed().as_millis() as u64 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only one scan runs at a time, so the tests that scan take turns.
    static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn port_lists_ranges_and_the_common_set() {
        assert_eq!(parse_ports("22, 80,443").unwrap(), [22, 80, 443]);
        assert_eq!(parse_ports("8000-8003 22 22").unwrap(), [22, 8000, 8001, 8002, 8003]);
        assert_eq!(parse_ports("65535").unwrap(), [65_535]);
        let common = parse_ports("common").unwrap();
        assert!(common.contains(&22) && common.contains(&443) && common.contains(&5432));
        assert!(common.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
        assert_eq!(parse_ports("22,common").unwrap().iter().filter(|p| **p == 22).count(), 1);
    }

    #[test]
    fn what_is_not_a_port_is_refused_with_the_word_that_was_wrong() {
        for bad in ["", "   ", "0", "65536", "http", "10-5", "1-", "-5", "22,x"] {
            assert!(parse_ports(bad).is_err(), "{bad:?}");
        }
        assert!(parse_ports("22,x").unwrap_err().contains("\"x\""));
        assert!(parse_ports("1-2000").unwrap_err().contains("1024"));
        assert!(parse_ports("1-1024").is_ok());
        assert!(parse_ports("1-1000,2000-2100").unwrap_err().contains("1024"), "the total counts, not each range");
    }

    #[test]
    fn services_are_named_for_the_usual_ports() {
        assert_eq!(service_of(22), Some("ssh"));
        assert_eq!(service_of(5432), Some("postgresql"));
        assert_eq!(service_of(54_321), None);
    }

    #[tokio::test]
    async fn finds_the_open_ports_and_only_those() {
        let _turn = TURN.lock().await;
        let a = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let b = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (pa, pb, pc) = (a.local_addr().unwrap().port(), b.local_addr().unwrap().port(), closed.local_addr().unwrap().port());
        drop(closed);
        let mut ports = vec![pa, pb, pc];
        ports.sort_unstable();
        let report = scan("127.0.0.1", &ports, Duration::from_secs(2)).await.unwrap();
        let found: Vec<u16> = report.open.iter().map(|p| p.port).collect();
        let mut want = vec![pa, pb];
        want.sort_unstable();
        assert_eq!(found, want, "{report:?}");
        assert_eq!(report.scanned, 3);
        assert_eq!(report.host, "127.0.0.1");
    }

    #[test]
    fn a_huge_list_is_refused_without_building_it() {
        assert!(parse_ports(&"1-1024,".repeat(1000)).is_err());
        assert!(parse_ports(&"1-1024,".repeat(10)).is_err(), "repeats still count while they are collected");
        assert!(parse_ports(&"22,".repeat(2000)).is_err(), "and a long list is refused outright");
    }

    #[tokio::test]
    async fn a_name_that_does_not_exist_is_an_error_not_an_empty_result() {
        let _turn = TURN.lock().await;
        let e = scan("no-such-host.invalid", &[22], Duration::from_millis(500)).await.unwrap_err();
        assert!(e.contains("could not find"), "{e}");
    }

    #[tokio::test]
    async fn bad_requests_are_refused_and_the_busy_flag_is_released() {
        let _turn = TURN.lock().await;
        assert!(scan("-oops", &[22], Duration::from_secs(1)).await.is_err());
        assert!(scan("127.0.0.1", &[], Duration::from_secs(1)).await.is_err());
        let too_many: Vec<u16> = (1..=1025).collect();
        assert!(scan("127.0.0.1", &too_many, Duration::from_secs(1)).await.is_err());
        // Refusals above must not leave the flag set, or no scan could ever run again.
        assert!(scan("127.0.0.1", &[1], Duration::from_millis(300)).await.is_ok());
    }
}
