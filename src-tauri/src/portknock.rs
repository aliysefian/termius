//! Port knocking: a short sequence of connection attempts to closed ports that tells a stealth firewall (`knockd`,
//! `fwknop`, an iptables recent-match rule) to open the SSH port for this computer's address.
//!
//! A TCP knock is one SYN. The port is closed by design, so the attempt being refused or timing out is the expected
//! outcome and not an error. A UDP knock is one tiny datagram. Between the knocks, and again after the last one,
//! the sequence waits, so the firewall sees them in order and has applied its rule before the SSH handshake starts.
//!
//! Where it runs: right before a hop is dialled directly from this computer. Through a SOCKS5 or HTTP proxy the TCP
//! knocks go through the proxy (the firewall sees the proxy's address, which is the one that will connect); UDP
//! cannot go through those, and a ProxyCommand is not run for a knock, so those are skipped with a notice. A host
//! reached through a jump host is not knocked from here: the firewall would see this computer, not the jump host.

use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{lookup_host, TcpStream, UdpSocket};

use crate::models::{Host, KnockProtocol, KnockStep, PortKnocking, ProxySpec};

pub const INTER_KNOCK_MIN_MS: u32 = 10;
pub const INTER_KNOCK_MAX_MS: u32 = 5000;
pub const POST_KNOCK_MAX_MS: u32 = 10_000;
/// No real sequence is longer; a typo'd paste shouldn't send hundreds of packets.
pub const MAX_STEPS: usize = 16;
/// A TCP knock only has to put the SYN on the wire, so it doesn't wait for an answer.
const TCP_KNOCK_TIMEOUT: Duration = Duration::from_millis(400);
/// Through a proxy the knock first has to reach the proxy, so it gets longer.
const PROXIED_KNOCK_TIMEOUT: Duration = Duration::from_millis(2000);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KnockError {
    #[error("{0}")]
    Invalid(String),
    #[error("couldn't find {0} to knock on it")]
    Resolve(String),
    #[error("couldn't send the knock to port {port}: {reason}")]
    Send { port: u16, reason: String },
}

/// Where the sequence's progress goes, one short line at a time (the terminal, or an event for the window).
pub type KnockNotify = Arc<dyn Fn(&str) + Send + Sync>;

/// A sequence ready to send, with delays already inside their limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<KnockStep>,
    pub inter_knock_ms: u32,
    pub post_knock_ms: u32,
}

/// What a hop carries into `connect_hop`: the plan, and where to report progress.
#[derive(Clone)]
pub struct Knock {
    pub plan: Plan,
    pub notify: Option<KnockNotify>,
}

impl Knock {
    /// The plan for a saved host, or `None` when knocking is off or there is nothing to send.
    pub fn for_host(host: &Host) -> Option<Knock> {
        let cfg = host.port_knocking.as_ref()?;
        Some(Knock { plan: Plan::from_config(cfg)?, notify: None })
    }
}

impl Plan {
    /// `None` when knocking is off or the sequence is empty. A stored delay outside its range (an old or hand-edited
    /// record) is brought inside it, so a bad value never stops a connection.
    pub fn from_config(cfg: &PortKnocking) -> Option<Plan> {
        if !cfg.enabled || cfg.sequence.is_empty() {
            return None;
        }
        Some(Plan {
            steps: cfg.sequence.iter().take(MAX_STEPS).copied().collect(),
            inter_knock_ms: cfg.inter_knock_delay_ms.clamp(INTER_KNOCK_MIN_MS, INTER_KNOCK_MAX_MS),
            post_knock_ms: cfg.post_knock_delay_ms.min(POST_KNOCK_MAX_MS),
        })
    }
}

fn proto_name(p: KnockProtocol) -> &'static str {
    match p {
        KnockProtocol::Tcp => "tcp",
        KnockProtocol::Udp => "udp",
    }
}

/// `"7000, 8000:udp, 9000"` as steps. Ports default to TCP; `:` or `/` may separate the protocol; commas, spaces and
/// semicolons separate the steps.
pub fn parse_sequence(text: &str) -> Result<Vec<KnockStep>, KnockError> {
    let mut steps = Vec::new();
    for token in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|t| !t.is_empty()) {
        steps.push(parse_step(token)?);
    }
    if steps.is_empty() {
        return Err(KnockError::Invalid("enter at least one port, like 7000, 8000:udp, 9000".into()));
    }
    if steps.len() > MAX_STEPS {
        return Err(KnockError::Invalid(format!("a knock sequence has at most {MAX_STEPS} ports")));
    }
    Ok(steps)
}

/// One step: `7000`, `7000:tcp`, `8000/udp`.
pub fn parse_step(token: &str) -> Result<KnockStep, KnockError> {
    let (port_text, proto_text) = match token.split_once([':', '/']) {
        Some((p, r)) => (p, Some(r)),
        None => (token, None),
    };
    let port: u32 = port_text.trim().parse().map_err(|_| KnockError::Invalid(format!("\"{token}\" isn't a port number")))?;
    if !(1..=65535).contains(&port) {
        return Err(KnockError::Invalid(format!("port {port} is out of range (1 to 65535)")));
    }
    let protocol = match proto_text.map(|p| p.trim().to_ascii_lowercase()) {
        None => KnockProtocol::Tcp,
        Some(p) if p == "tcp" => KnockProtocol::Tcp,
        Some(p) if p == "udp" => KnockProtocol::Udp,
        Some(p) => return Err(KnockError::Invalid(format!("\"{p}\" isn't a protocol: use tcp or udp"))),
    };
    Ok(KnockStep { port: port as u16, protocol })
}

/// The steps as `"7000, 8000:udp"`: TCP is the default so it has no suffix.
pub fn format_sequence(steps: &[KnockStep]) -> String {
    steps
        .iter()
        .map(|s| match s.protocol {
            KnockProtocol::Tcp => s.port.to_string(),
            KnockProtocol::Udp => format!("{}:udp", s.port),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a configuration may be saved. It need not be enabled to be checked, but a sequence that is there must be
/// well formed, and an enabled one must not be empty.
pub fn validate(cfg: &PortKnocking) -> Result<(), KnockError> {
    if cfg.enabled && cfg.sequence.is_empty() {
        return Err(KnockError::Invalid("port knocking is on but the sequence is empty".into()));
    }
    if cfg.sequence.len() > MAX_STEPS {
        return Err(KnockError::Invalid(format!("a knock sequence has at most {MAX_STEPS} ports")));
    }
    if cfg.sequence.iter().any(|s| s.port == 0) {
        return Err(KnockError::Invalid("port 0 can't be knocked".into()));
    }
    if !(INTER_KNOCK_MIN_MS..=INTER_KNOCK_MAX_MS).contains(&cfg.inter_knock_delay_ms) {
        return Err(KnockError::Invalid(format!("the delay between knocks must be {INTER_KNOCK_MIN_MS} to {INTER_KNOCK_MAX_MS} ms")));
    }
    if cfg.post_knock_delay_ms > POST_KNOCK_MAX_MS {
        return Err(KnockError::Invalid(format!("the wait before connecting must be 0 to {POST_KNOCK_MAX_MS} ms")));
    }
    Ok(())
}

/// What a refused, unreachable or timed-out knock looks like: all of them mean the SYN was sent to a closed port.
fn expected_for_a_closed_port(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionRefused | ErrorKind::TimedOut | ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::WouldBlock
    )
}

async fn tcp_knock(addr: SocketAddr) -> Result<(), KnockError> {
    match tokio::time::timeout(TCP_KNOCK_TIMEOUT, TcpStream::connect(addr)).await {
        // Connected (the port was open after all) or timed out: the SYN went out either way. Dropping closes it.
        Ok(Ok(_)) | Err(_) => Ok(()),
        Ok(Err(e)) if expected_for_a_closed_port(e.kind()) => Ok(()),
        Ok(Err(e)) => Err(KnockError::Send { port: addr.port(), reason: e.to_string() }),
    }
}

async fn udp_knock(addr: SocketAddr) -> Result<(), KnockError> {
    let bind = if addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let send = |e: std::io::Error| KnockError::Send { port: addr.port(), reason: e.to_string() };
    let socket = UdpSocket::bind(bind).await.map_err(send)?;
    match socket.send_to(&[0u8], addr).await {
        Ok(_) => Ok(()),
        Err(e) if expected_for_a_closed_port(e.kind()) => Ok(()),
        Err(e) => Err(send(e)),
    }
}

/// A TCP knock through a proxy: the proxy opens the connection, so it is the proxy's address the firewall sees.
/// The answer (refused, or an open port) doesn't matter; the proxy having been asked is the knock.
async fn proxied_tcp_knock(host: &str, port: u16, proxy: &ProxySpec) {
    let _ = tokio::time::timeout(PROXIED_KNOCK_TIMEOUT, crate::dial::open(host, port, Some(proxy), "")).await;
}

fn say(notify: Option<&KnockNotify>, line: &str) {
    if let Some(n) = notify {
        n(line);
    }
}

/// Send the sequence to `host`. Returns once the last knock has been followed by the post-knock wait.
pub async fn run(host: &str, proxy: Option<&ProxySpec>, plan: &Plan, notify: Option<&KnockNotify>) -> Result<(), KnockError> {
    if matches!(proxy, Some(ProxySpec::Command { .. })) {
        say(notify, &format!("Skipping the port knock for {host}: it isn't sent through a ProxyCommand."));
        return Ok(());
    }
    // Resolve once, so every knock goes to the same address, and a mistyped name is said so plainly.
    let addr = if proxy.is_none() {
        let first = lookup_host((host, 0)).await.map_err(|_| KnockError::Resolve(host.to_string()))?.next();
        Some(first.ok_or_else(|| KnockError::Resolve(host.to_string()))?)
    } else {
        None
    };
    let total = plan.steps.len();
    for (i, step) in plan.steps.iter().enumerate() {
        say(notify, &format!("Sending port knock sequence to {host} ({}/{total}: {}/{})...", i + 1, step.port, proto_name(step.protocol)));
        match (proxy, addr) {
            (None, Some(mut a)) => {
                a.set_port(step.port);
                match step.protocol {
                    KnockProtocol::Tcp => tcp_knock(a).await?,
                    KnockProtocol::Udp => udp_knock(a).await?,
                }
            }
            (Some(p), _) => match step.protocol {
                KnockProtocol::Tcp => proxied_tcp_knock(host, step.port, p).await,
                KnockProtocol::Udp => say(notify, &format!("Skipping {}/udp: a UDP knock can't go through a proxy.", step.port)),
            },
            (None, None) => unreachable!("without a proxy the address was resolved above"),
        }
        if i + 1 < total {
            tokio::time::sleep(Duration::from_millis(u64::from(plan.inter_knock_ms))).await;
        }
    }
    say(notify, &format!("Port knock sequence completed. Waiting {}ms before SSH handshake...", plan.post_knock_ms));
    if plan.post_knock_ms > 0 {
        tokio::time::sleep(Duration::from_millis(u64::from(plan.post_knock_ms))).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;

    fn step(port: u16, protocol: KnockProtocol) -> KnockStep {
        KnockStep { port, protocol }
    }

    #[test]
    fn parses_ports_with_tcp_as_the_default() {
        assert_eq!(
            parse_sequence("7000, 8000:udp, 9000").unwrap(),
            vec![step(7000, KnockProtocol::Tcp), step(8000, KnockProtocol::Udp), step(9000, KnockProtocol::Tcp)]
        );
        assert_eq!(parse_sequence("7000:tcp 8000/UDP;9000").unwrap().len(), 3);
        assert_eq!(parse_sequence("  1  ").unwrap(), vec![step(1, KnockProtocol::Tcp)]);
    }

    #[test]
    fn says_what_is_wrong_with_a_sequence() {
        for (text, needle) in [("", "at least one"), ("  ,, ", "at least one"), ("0", "out of range"), ("65536", "out of range"), ("-5", "isn't a port"), ("abc", "isn't a port"), ("7000:icmp", "tcp or udp"), ("7000:", "tcp or udp")] {
            let e = parse_sequence(text).unwrap_err().to_string();
            assert!(e.contains(needle), "{text:?} gave {e:?}");
        }
        assert!(parse_sequence(&vec!["7000"; MAX_STEPS + 1].join(",")).is_err());
    }

    #[test]
    fn formats_back_to_the_compact_form() {
        let steps = parse_sequence("7000, 8000:udp,9000/tcp").unwrap();
        assert_eq!(format_sequence(&steps), "7000, 8000:udp, 9000");
        assert_eq!(parse_sequence(&format_sequence(&steps)).unwrap(), steps);
    }

    fn cfg(enabled: bool) -> PortKnocking {
        PortKnocking { enabled, sequence: vec![step(7000, KnockProtocol::Tcp)], inter_knock_delay_ms: 150, post_knock_delay_ms: 500 }
    }

    #[test]
    fn validates_delays_and_the_sequence() {
        assert!(validate(&cfg(true)).is_ok());
        assert!(validate(&PortKnocking { sequence: vec![], ..cfg(true) }).is_err());
        assert!(validate(&PortKnocking { sequence: vec![], ..cfg(false) }).is_ok(), "a switched-off, empty sequence is fine");
        assert!(validate(&PortKnocking { inter_knock_delay_ms: 9, ..cfg(true) }).is_err());
        assert!(validate(&PortKnocking { inter_knock_delay_ms: 5001, ..cfg(true) }).is_err());
        assert!(validate(&PortKnocking { post_knock_delay_ms: 10_001, ..cfg(true) }).is_err());
        assert!(validate(&PortKnocking { post_knock_delay_ms: 0, inter_knock_delay_ms: 10, ..cfg(true) }).is_ok());
        assert!(validate(&PortKnocking { sequence: vec![step(0, KnockProtocol::Tcp)], ..cfg(true) }).is_err());
    }

    #[test]
    fn a_plan_exists_only_for_an_enabled_sequence_and_keeps_delays_in_range() {
        assert_eq!(Plan::from_config(&cfg(false)), None);
        assert_eq!(Plan::from_config(&PortKnocking { sequence: vec![], ..cfg(true) }), None);
        let wild = Plan::from_config(&PortKnocking { inter_knock_delay_ms: 0, post_knock_delay_ms: 999_999, ..cfg(true) }).unwrap();
        assert_eq!((wild.inter_knock_ms, wild.post_knock_ms), (INTER_KNOCK_MIN_MS, POST_KNOCK_MAX_MS));
        let host = Host { label: "a".into(), hostname: "h".into(), port_knocking: Some(cfg(true)), ..Default::default() };
        assert!(Knock::for_host(&host).is_some());
        assert!(Knock::for_host(&Host::default()).is_none());
    }

    fn collect() -> (KnockNotify, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let l2 = Arc::clone(&lines);
        (Arc::new(move |s: &str| l2.lock().unwrap().push(s.to_string())), lines)
    }

    /// A TCP knock to a port nobody listens on is refused, and that is a successful knock.
    #[tokio::test]
    async fn a_refused_tcp_knock_succeeds_and_the_progress_is_reported() {
        // Bind then drop, so the port is known to be closed.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let plan = Plan { steps: vec![step(closed, KnockProtocol::Tcp), step(closed, KnockProtocol::Tcp)], inter_knock_ms: 10, post_knock_ms: 0 };
        let (n, lines) = collect();
        run("127.0.0.1", None, &plan, Some(&n)).await.unwrap();
        let lines = lines.lock().unwrap().clone();
        assert!(lines[0].starts_with("Sending port knock sequence to 127.0.0.1 (1/2:"), "{lines:?}");
        assert!(lines[1].contains("(2/2:"), "{lines:?}");
        assert!(lines.last().unwrap().contains("completed") && lines.last().unwrap().contains("0ms"), "{lines:?}");
    }

    /// Each TCP knock really is a connection attempt: a listener sees them, in order.
    #[tokio::test]
    async fn knocks_arrive_in_order_on_tcp_and_udp() {
        let l1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let l2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let u = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (p1, p2, pu) = (l1.local_addr().unwrap().port(), l2.local_addr().unwrap().port(), u.local_addr().unwrap().port());
        let plan = Plan { steps: vec![step(p1, KnockProtocol::Tcp), step(pu, KnockProtocol::Udp), step(p2, KnockProtocol::Tcp)], inter_knock_ms: 30, post_knock_ms: 0 };
        let seen = Arc::new(Mutex::new(Vec::<(Instant, &'static str)>::new()));
        let s = Arc::clone(&seen);
        let t1 = tokio::spawn(async move {
            l1.accept().await.unwrap();
            s.lock().unwrap().push((Instant::now(), "tcp1"));
        });
        let s = Arc::clone(&seen);
        let t2 = tokio::spawn(async move {
            l2.accept().await.unwrap();
            s.lock().unwrap().push((Instant::now(), "tcp2"));
        });
        let s = Arc::clone(&seen);
        let tu = tokio::spawn(async move {
            let mut buf = [0u8; 8];
            let (n, _) = u.recv_from(&mut buf).await.unwrap();
            assert_eq!(n, 1, "the UDP knock is a single byte");
            s.lock().unwrap().push((Instant::now(), "udp"));
        });
        run("127.0.0.1", None, &plan, None).await.unwrap();
        for t in [t1, t2, tu] {
            tokio::time::timeout(Duration::from_secs(2), t).await.unwrap().unwrap();
        }
        let mut order = seen.lock().unwrap().clone();
        order.sort_by_key(|(at, _)| *at);
        assert_eq!(order.iter().map(|(_, n)| *n).collect::<Vec<_>>(), ["tcp1", "udp", "tcp2"]);
    }

    /// UDP knocks, so the only time spent is the waits themselves: a TCP knock to a closed port takes as long as the
    /// OS takes to refuse it (the whole 400 ms timeout on Windows), which would drown the delays being measured.
    #[tokio::test]
    async fn waits_between_knocks_and_after_the_last() {
        let sink = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = sink.local_addr().unwrap().port();
        let plan = Plan { steps: vec![step(port, KnockProtocol::Udp); 3], inter_knock_ms: 300, post_knock_ms: 300 };
        let started = Instant::now();
        run("127.0.0.1", None, &plan, None).await.unwrap();
        // Two gaps and the final wait: at least 900 ms, and clearly short of the 1200 ms it would be with a gap after
        // the last knock too. The 250 ms of room above the minimum is for a slow machine.
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(900), "{took:?}");
        assert!(took < Duration::from_millis(1150), "{took:?}");
    }

    /// A TCP knock never holds the sequence up for more than its short timeout, whatever the OS does with a closed
    /// port (an instant refusal on Linux, a slow one on Windows).
    #[tokio::test]
    async fn a_tcp_knock_is_cut_off_at_its_timeout() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let plan = Plan { steps: vec![step(closed, KnockProtocol::Tcp); 3], inter_knock_ms: 10, post_knock_ms: 0 };
        let started = Instant::now();
        run("127.0.0.1", None, &plan, None).await.unwrap();
        // Three knocks of at most 400 ms each, plus two 10 ms gaps, plus slack.
        assert!(started.elapsed() < Duration::from_millis(1200 + 20 + 1500), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn an_unknown_host_is_reported_not_swallowed() {
        let plan = Plan { steps: vec![step(7000, KnockProtocol::Tcp)], inter_knock_ms: 10, post_knock_ms: 0 };
        let e = run("no-such-host.invalid", None, &plan, None).await.unwrap_err();
        assert!(matches!(e, KnockError::Resolve(_)), "{e:?}");
    }

    #[tokio::test]
    async fn a_proxy_command_is_skipped_with_a_notice() {
        let plan = Plan { steps: vec![step(7000, KnockProtocol::Tcp)], inter_knock_ms: 10, post_knock_ms: 0 };
        let (n, lines) = collect();
        let proxy = ProxySpec::Command { command: "ssh -W %h:%p x".into(), approved: false };
        run("h", Some(&proxy), &plan, Some(&n)).await.unwrap();
        assert!(lines.lock().unwrap()[0].contains("ProxyCommand"));
    }
}
