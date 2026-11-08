//! Listening ports: what is listening on the host, from `ss` (or `netstat`, or the kernel's own tables when neither is
//! there). Read on request. A listener is shown with the address it is bound to, and whether that is the host's own
//! loopback; nothing here claims a listener is reachable from outside, which depends on the firewall and the network.

use std::collections::HashMap;

use uuid::Uuid;

use super::model::{ListeningPort, PortList, Proto};
use super::parse::{sections, u, ParseError, MAX_OUTPUT_BYTES};

/// More listeners than this is not a host this list is for.
const MAX_ITEMS: usize = 2000;

pub const SCRIPT: &str = r#"LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
os=$(uname -s 2>/dev/null)
echo "@@OS"; echo "$os"
if [ "$os" = Linux ]; then
  echo "@@PORTS"
  if have ss; then
    echo "tool ss"
    ss -H -lntupe 2>/dev/null || ss -lntupe 2>/dev/null
  elif have netstat; then
    echo "tool netstat"
    netstat -lntup 2>/dev/null || netstat -lntu 2>/dev/null
  else
    echo "tool proc"
    for f in tcp tcp6 udp udp6; do echo "file $f"; cat "/proc/net/$f" 2>/dev/null; done
  fi
  echo "@@PASSWD"; cat /etc/passwd 2>/dev/null
fi
echo "@@END"
"#;

fn is_local_only(addr: &str) -> bool {
    addr.starts_with("127.") || addr == "::1" || addr == "localhost"
}

/// `0.0.0.0:22`, `[::]:22`, `*:443`, `127.0.0.53%lo:53`, `[fe80::1%eth0]:53`, `:::22` into (address, port).
fn split_addr(s: &str) -> Option<(String, u16)> {
    let (a, p) = s.rsplit_once(':')?;
    let port = p.parse::<u16>().ok()?;
    let a = a.trim_start_matches('[').trim_end_matches(']');
    let a = a.split('%').next().unwrap_or(a);
    (!a.is_empty() || s.starts_with(':')).then(|| (if a.is_empty() { "::".to_string() } else { a.to_string() }, port))
}

fn item(proto: Proto, state: &str, local: &str, pid: Option<u32>, process: Option<String>, user: Option<String>) -> Option<ListeningPort> {
    let (address, port) = split_addr(local)?;
    Some(ListeningPort { proto, ipv6: address.contains(':'), local_only: is_local_only(&address), address, port, pid, process, user, state: state.to_string() })
}

/// `users:(("sshd",pid=812,fd=3),("sshd",pid=900,fd=3))`: the first process named.
fn users_field(line: &str) -> (Option<u32>, Option<String>) {
    let Some(at) = line.find("users:((\"") else { return (None, None) };
    let rest = &line[at + 9..];
    let Some(end) = rest.find("\",pid=") else { return (None, None) };
    let name: String = rest[..end].chars().take(40).collect();
    let pid = rest[end + 6..].split(|c: char| !c.is_ascii_digit()).next().and_then(|d| d.parse().ok());
    (pid, Some(name))
}

fn parse_ss(lines: &[&str], users: &HashMap<u32, String>) -> Vec<ListeningPort> {
    let mut out = Vec::new();
    for l in lines {
        let t: Vec<&str> = l.split_whitespace().collect();
        // Netid State Recv-Q Send-Q Local:Port Peer:Port [users:(...)] [uid:N ino:N ...]; the header is skipped by its name.
        if t.len() < 6 || t[0] == "Netid" || out.len() >= MAX_ITEMS {
            continue;
        }
        let proto = match t[0] {
            "tcp" => Proto::Tcp,
            "udp" => Proto::Udp,
            _ => continue,
        };
        let (pid, process) = users_field(l);
        let user = t.iter().find_map(|x| x.strip_prefix("uid:")).and_then(|v| v.parse::<u32>().ok()).map(|uid| users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()));
        out.extend(item(proto, t[1], t[4], pid, process, user));
    }
    out
}

fn parse_netstat(lines: &[&str]) -> Vec<ListeningPort> {
    let mut out = Vec::new();
    for l in lines {
        let t: Vec<&str> = l.split_whitespace().collect();
        // Proto Recv-Q Send-Q Local Foreign [State] [PID/Program]
        if t.len() < 5 || out.len() >= MAX_ITEMS {
            continue;
        }
        let proto = if t[0].starts_with("tcp") {
            Proto::Tcp
        } else if t[0].starts_with("udp") {
            Proto::Udp
        } else {
            continue;
        };
        let last = t[t.len() - 1];
        let (pid, process) = match last.split_once('/') {
            Some((p, n)) if p.chars().all(|c| c.is_ascii_digit()) => (p.parse().ok(), Some(n.chars().take(40).collect())),
            _ => (None, None),
        };
        let state = if proto == Proto::Tcp { t.get(5).copied().unwrap_or("LISTEN") } else { "UNCONN" };
        if proto == Proto::Tcp && state != "LISTEN" {
            continue;
        }
        out.extend(item(proto, state, t[3], pid, process, None));
    }
    out
}

/// A hex address from `/proc/net/*`: IPv4 is one little-endian word, IPv6 is four of them.
fn hex_addr(h: &str) -> Option<String> {
    let bytes: Vec<u8> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect::<Option<_>>()?;
    match bytes.len() {
        4 => Some(format!("{}.{}.{}.{}", bytes[3], bytes[2], bytes[1], bytes[0])),
        16 => {
            let mut words = [0u16; 8];
            for w in 0..4 {
                let b = &bytes[w * 4..w * 4 + 4];
                // Each 32-bit word is little-endian: reverse its bytes to get the address's order.
                words[w * 2] = u16::from_be_bytes([b[3], b[2]]);
                words[w * 2 + 1] = u16::from_be_bytes([b[1], b[0]]);
            }
            Some(std::net::Ipv6Addr::from(words).to_string())
        }
        _ => None,
    }
}

fn parse_proc(lines: &[&str], users: &HashMap<u32, String>) -> Vec<ListeningPort> {
    let mut out = Vec::new();
    let mut proto = Proto::Tcp;
    for l in lines {
        if let Some(f) = l.strip_prefix("file ") {
            proto = if f.trim().starts_with("udp") { Proto::Udp } else { Proto::Tcp };
            continue;
        }
        let t: Vec<&str> = l.split_whitespace().collect();
        if t.len() < 8 || t[0] == "sl" || out.len() >= MAX_ITEMS {
            continue;
        }
        // A listening TCP socket is state 0A; a bound, unconnected UDP socket is 07.
        let want = if proto == Proto::Tcp { "0A" } else { "07" };
        if t[3] != want {
            continue;
        }
        let Some((h, p)) = t[1].split_once(':') else { continue };
        let (Some(addr), Ok(port)) = (hex_addr(h), u16::from_str_radix(p, 16)) else { continue };
        let user = t[7].parse::<u32>().ok().map(|uid| users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()));
        let local = if addr.contains(':') { format!("[{addr}]:{port}") } else { format!("{addr}:{port}") };
        out.extend(item(proto, if proto == Proto::Tcp { "LISTEN" } else { "UNCONN" }, &local, None, None, user));
    }
    out
}

pub fn parse(host_id: Uuid, taken_at_ms: u64, out: &str) -> Result<Option<PortList>, ParseError> {
    if out.len() > MAX_OUTPUT_BYTES {
        return Err(ParseError::TooLarge);
    }
    if out.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let s = sections(out);
    if !s.contains_key("END") {
        return Err(ParseError::Incomplete);
    }
    let get = |k: &str| s.get(k).map(Vec::as_slice).unwrap_or(&[]);
    if get("OS").iter().map(|l| l.trim()).find(|l| !l.is_empty()) != Some("Linux") {
        return Ok(None);
    }
    let users: HashMap<u32, String> = get("PASSWD")
        .iter()
        .filter_map(|l| {
            let p: Vec<&str> = l.split(':').collect();
            Some((u32::try_from(u(p.get(2)?)?).ok()?, p.first()?.to_string()))
        })
        .collect();
    let ports = get("PORTS");
    let tool = ports.first().and_then(|l| l.strip_prefix("tool ")).unwrap_or("").trim();
    let body = if ports.is_empty() { &[][..] } else { &ports[1..] };
    let (source, mut items) = match tool {
        "ss" => ("ss", parse_ss(body, &users)),
        "netstat" => ("netstat", parse_netstat(body)),
        _ => ("proc", parse_proc(body, &users)),
    };
    // The same socket is listed once per address family it is bound on, and a program that forked workers shares its
    // socket: one row per (protocol, address, port) is what a person means.
    items.sort_by(|a, b| (a.port, a.proto as u8, a.address.as_str()).cmp(&(b.port, b.proto as u8, b.address.as_str())));
    items.dedup_by(|a, b| a.proto == b.proto && a.address == b.address && a.port == b.port);
    let owners_hidden = source != "proc" && items.iter().any(|i| i.process.is_none());
    Ok(Some(PortList { host_id, taken_at_ms, source: source.to_string(), owners_hidden, items }))
}
