//! ICMP ping through the system's own `ping` program.
//!
//! Raw ICMP sockets need elevated rights on most systems, and the system `ping` already has them (set-uid, or a
//! capability), so this runs it and reads the answer. The parser does not depend on the language of the output: a
//! reply is a line with `ttl=` (or `hlim=`) in it, and a round trip is the number before the last `ms`. Windows
//! prints those the same way in every language.

use std::process::Stdio;
use std::time::Duration;

/// A host name or address that is safe to hand to `ping`: no leading `-` (which would be read as an option) and
/// only the characters names and IP addresses use.
pub fn valid_host(h: &str) -> bool {
    !h.is_empty() && h.len() <= 253 && !h.starts_with('-') && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'))
}

#[derive(Debug, Clone, PartialEq)]
pub struct PingResult {
    pub sent: u8,
    pub received: u8,
    /// 0 to 100.
    pub loss_pct: f64,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Why no reply came, when the program itself could not reach the host (for example the name did not resolve).
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingError {
    InvalidHost,
    /// There is no `ping` program on this computer.
    Missing,
    Failed(String),
}

impl std::fmt::Display for PingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PingError::InvalidHost => write!(f, "that is not a valid host name or address"),
            PingError::Missing => write!(f, "the ping program is not installed on this computer"),
            PingError::Failed(m) => write!(f, "{m}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Linux,
    /// macOS and the BSDs: `-W` is in milliseconds.
    Bsd,
    Windows,
}

pub fn flavor() -> Flavor {
    if cfg!(windows) {
        Flavor::Windows
    } else if cfg!(target_os = "linux") {
        Flavor::Linux
    } else {
        Flavor::Bsd
    }
}

/// The program to run. BSD `ping` handles IPv4 only; IPv6 has its own program.
pub fn program(f: Flavor, host: &str) -> &'static str {
    if f == Flavor::Bsd && host.contains(':') {
        "ping6"
    } else {
        "ping"
    }
}

pub fn args(f: Flavor, host: &str, count: u8, timeout_ms: u32) -> Vec<String> {
    match f {
        Flavor::Linux => vec!["-c".into(), count.to_string(), "-W".into(), timeout_ms.div_ceil(1000).max(1).to_string(), host.to_string()],
        Flavor::Bsd => vec!["-c".into(), count.to_string(), "-W".into(), timeout_ms.to_string(), host.to_string()],
        Flavor::Windows => vec!["-n".into(), count.to_string(), "-w".into(), timeout_ms.to_string(), host.to_string()],
    }
}

/// The round trip on one reply line (already lower case), in milliseconds. `time<1ms` counts as half a millisecond.
fn line_ms(l: &str) -> Option<f64> {
    // Windows in Russian writes the unit in Cyrillic.
    let end = l.rfind("ms").or_else(|| l.rfind("\u{43c}\u{441}"))?;
    let head = l[..end].trim_end();
    let start = head.char_indices().rev().find(|(_, c)| !(c.is_ascii_digit() || *c == '.')).map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    let num = &head[start..];
    if num.is_empty() {
        return None;
    }
    let v: f64 = num.parse().ok()?;
    Some(if head[..start].ends_with('<') { v / 2.0 } else { v })
}

/// Read what `ping` printed after `sent` packets.
pub fn parse_output(text: &str, sent: u8) -> PingResult {
    let mut times: Vec<f64> = Vec::new();
    let mut received: u32 = 0;
    for line in text.lines() {
        let l = line.to_ascii_lowercase();
        if !(l.contains("ttl=") || l.contains("hlim=")) || l.contains("(dup!)") {
            continue;
        }
        received += 1;
        if let Some(ms) = line_ms(&l) {
            times.push(ms);
        }
    }
    let received = received.min(u32::from(sent)) as u8;
    let loss_pct = if sent == 0 { 100.0 } else { f64::from(sent - received) * 100.0 / f64::from(sent) };
    let (min_ms, max_ms, avg_ms) = if times.is_empty() {
        (None, None, None)
    } else {
        let min = times.iter().copied().fold(f64::INFINITY, f64::min);
        let max = times.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (Some(min), Some(max), Some(times.iter().sum::<f64>() / times.len() as f64))
    };
    PingResult { sent, received, loss_pct, avg_ms, min_ms, max_ms, error: None }
}

/// What `ping` prints when it is not permitted to send (no raw-socket right, a sandbox) or was given bad options.
fn cannot_run(said: &str) -> bool {
    let l = said.to_lowercase();
    ["operation not permitted", "permission denied", "cap_net_raw", "usage:", "invalid option", "unknown option", "socket:", "raw socket", "lacking privilege"].iter().any(|m| l.contains(m))
}

/// Send `count` echo requests to `host` and report loss and latency. Never errors for a host that does not answer:
/// that is a result (100% loss). It errors only when ping itself cannot run.
pub async fn ping(host: &str, count: u8, timeout_ms: u32) -> Result<PingResult, PingError> {
    if !valid_host(host) {
        return Err(PingError::InvalidHost);
    }
    let count = count.clamp(1, 10);
    let timeout_ms = timeout_ms.clamp(200, 10_000);
    let f = flavor();
    let mut cmd = tokio::process::Command::new(program(f, host));
    cmd.args(args(f, host, count, timeout_ms)).stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    // Each packet can take a second to send, or the whole wait if it is lost (Windows waits that long for every one).
    let limit = Duration::from_millis(u64::from(count) * u64::from(timeout_ms).max(1000) + 3000);
    let out = match tokio::time::timeout(limit, cmd.output()).await {
        Err(_) => return Ok(PingResult { sent: count, received: 0, loss_pct: 100.0, avg_ms: None, min_ms: None, max_ms: None, error: Some("ping did not finish in time".into()) }),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(PingError::Missing),
        Ok(Err(e)) => return Err(PingError::Failed(e.to_string())),
        Ok(Ok(out)) => out,
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut result = parse_output(&stdout, count);
    if result.received == 0 {
        // Windows reports an unknown host on stdout and the others on stderr.
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let said = if said.is_empty() { stdout.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string() } else { said };
        let said: String = said.chars().take(200).collect();
        // A ping that was not allowed to run says nothing about the host. Calling that 100% loss would page someone.
        if cannot_run(&said) {
            return Err(PingError::Failed(said));
        }
        if !said.is_empty() {
            result.error = Some(said);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = "PING 10.0.0.5 (10.0.0.5) 56(84) bytes of data.\n\
64 bytes from 10.0.0.5: icmp_seq=1 ttl=64 time=0.412 ms\n\
64 bytes from 10.0.0.5: icmp_seq=2 ttl=64 time=0.380 ms\n\
64 bytes from 10.0.0.5: icmp_seq=4 ttl=64 time=1.208 ms\n\
\n\
--- 10.0.0.5 ping statistics ---\n\
4 packets transmitted, 3 received, 25% packet loss, time 3052ms\n\
rtt min/avg/max/mdev = 0.380/0.667/1.208/0.382 ms\n";

    const WINDOWS: &str = "\nPinging 10.0.0.5 with 32 bytes of data:\n\
Reply from 10.0.0.5: bytes=32 time=12ms TTL=57\n\
Reply from 10.0.0.5: bytes=32 time<1ms TTL=57\n\
Request timed out.\n\
Reply from 10.0.0.5: bytes=32 time=14ms TTL=57\n\
\n\
Ping statistics for 10.0.0.5:\n\
    Packets: Sent = 4, Received = 3, Lost = 1 (25% loss),\n\
Approximate round trip times in milli-seconds:\n\
    Minimum = 0ms, Maximum = 14ms, Average = 8ms\n";

    #[test]
    fn linux_output_gives_loss_and_latency() {
        let r = parse_output(LINUX, 4);
        assert_eq!((r.sent, r.received), (4, 3));
        assert!((r.loss_pct - 25.0).abs() < 1e-9);
        assert!((r.min_ms.unwrap() - 0.380).abs() < 1e-9);
        assert!((r.max_ms.unwrap() - 1.208).abs() < 1e-9);
        assert!((r.avg_ms.unwrap() - 0.6666666).abs() < 1e-3);
    }

    #[test]
    fn windows_output_ignores_the_summary_lines_and_reads_sub_millisecond_replies() {
        let r = parse_output(WINDOWS, 4);
        assert_eq!(r.received, 3);
        assert!((r.loss_pct - 25.0).abs() < 1e-9);
        assert_eq!(r.max_ms, Some(14.0));
        assert_eq!(r.min_ms, Some(0.5));
    }

    #[test]
    fn a_translated_windows_line_still_counts() {
        // Only "TTL=" and "ms" are relied on, not the words around them.
        let text = "Antwort von 10.0.0.5: Bytes=32 Zeit=3ms TTL=64\nZeitüberschreitung der Anforderung.\n";
        let r = parse_output(text, 2);
        assert_eq!(r.received, 1);
        assert_eq!(r.avg_ms, Some(3.0));
        // A multi-byte character right before the number must not break the scan.
        assert_eq!(parse_output("Antwort: Zeit：7ms TTL=64\n", 1).avg_ms, Some(7.0));
    }

    #[test]
    fn macos_and_ipv6_replies() {
        let mac = "64 bytes from 10.0.0.5: icmp_seq=0 ttl=64 time=2.345 ms\n";
        assert_eq!(parse_output(mac, 1).avg_ms, Some(2.345));
        let v6 = "16 bytes from 2001:db8::1, icmp_seq=0 hlim=64 time=1.5 ms\n";
        assert_eq!(parse_output(v6, 1).received, 1);
    }

    #[test]
    fn nothing_answering_is_total_loss_and_duplicates_do_not_exceed_what_was_sent() {
        let r = parse_output("PING x\n--- x ping statistics ---\n4 packets transmitted, 0 received, 100% packet loss\n", 4);
        assert_eq!((r.received, r.loss_pct, r.avg_ms), (0, 100.0, None));
        let dup = "64 bytes from a: icmp_seq=1 ttl=64 time=1 ms\n64 bytes from a: icmp_seq=1 ttl=64 time=1 ms (DUP!)\n";
        assert_eq!(parse_output(dup, 1).received, 1);
        assert_eq!(parse_output("ttl=1 time=1 ms\nttl=1 time=1 ms\nttl=1 time=1 ms\n", 2).received, 2);
    }

    #[test]
    fn a_host_name_with_dup_in_it_is_not_a_duplicate_reply() {
        let line = "64 bytes from backup-dup.example.com (10.0.0.5): icmp_seq=1 ttl=64 time=0.5 ms\n";
        assert_eq!(parse_output(line, 1).received, 1);
    }

    #[test]
    fn russian_windows_writes_the_unit_in_cyrillic() {
        let line = "\u{41e}\u{442}\u{432}\u{435}\u{442} \u{43e}\u{442} 10.0.0.5: \u{447}\u{438}\u{441}\u{43b}\u{43e} \u{431}\u{430}\u{439}\u{442}=32 \u{432}\u{440}\u{435}\u{43c}\u{44f}=7\u{43c}\u{441} TTL=64\n";
        let r = parse_output(line, 1);
        assert_eq!((r.received, r.avg_ms), (1, Some(7.0)));
    }

    #[test]
    fn a_ping_that_may_not_run_is_an_error_not_total_loss() {
        assert!(cannot_run("ping: socket: Operation not permitted"));
        assert!(cannot_run("ping: Lacking privilege for raw socket."));
        assert!(!cannot_run("ping: web.example.com: Name or service not known"));
        assert!(!cannot_run("Ping request could not find host x. Please check the name and try again."));
    }

    #[test]
    fn hosts_that_could_be_read_as_options_are_refused() {
        for bad in ["", "-c", "-oProxyCommand=x", "a b", "a;b", "a$(x)", "a\nb", "ho`st`"] {
            assert!(!valid_host(bad), "{bad:?}");
        }
        for good in ["10.0.0.5", "web-01.example.com", "fe80::1%eth0", "my_host"] {
            assert!(valid_host(good), "{good:?}");
        }
        assert!(!valid_host(&"a".repeat(254)));
    }

    #[test]
    fn arguments_per_platform() {
        assert_eq!(args(Flavor::Linux, "h", 4, 1500), ["-c", "4", "-W", "2", "h"]);
        assert_eq!(args(Flavor::Linux, "h", 1, 200), ["-c", "1", "-W", "1", "h"]);
        assert_eq!(args(Flavor::Bsd, "h", 3, 800), ["-c", "3", "-W", "800", "h"]);
        assert_eq!(args(Flavor::Windows, "h", 2, 1000), ["-n", "2", "-w", "1000", "h"]);
        assert_eq!(program(Flavor::Bsd, "::1"), "ping6");
        assert_eq!(program(Flavor::Linux, "::1"), "ping");
    }

    #[tokio::test]
    async fn pinging_a_bad_name_does_not_run_anything() {
        assert_eq!(ping("-c", 1, 500).await, Err(PingError::InvalidHost));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn loopback_answers_when_ping_is_installed() {
        match ping("127.0.0.1", 1, 1000).await {
            Err(PingError::Missing) => eprintln!("skipping: no ping program"),
            Err(e) => panic!("{e}"),
            Ok(r) => {
                // Some sandboxes forbid raw sockets; then there is an error text and no reply.
                if r.received == 0 {
                    eprintln!("skipping: ping could not send ({:?})", r.error);
                } else {
                    assert_eq!(r.loss_pct, 0.0);
                    assert!(r.avg_ms.is_some());
                }
            }
        }
    }
}
