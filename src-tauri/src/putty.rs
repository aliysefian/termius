//! Import saved PuTTY sessions: from the registry on Windows, from
//! `~/.putty/sessions/` on other systems. Only connection details are read;
//! PuTTY doesn't store passwords, and key paths are referenced.

use std::collections::BTreeMap;
use std::path::Path;

use crate::sshconfig::ImportedHost;

/// PuTTY percent-encodes session names (`My%20Server`).
fn decode_name(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `22`, or the registry's `0x16`.
fn parse_num(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Turn one session's settings into a host. `None` when it isn't SSH or has
/// no address.
pub fn session_to_host(name: &str, kv: &BTreeMap<String, String>) -> Option<ImportedHost> {
    let get = |k: &str| kv.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    if get("Protocol").is_some_and(|p| !p.eq_ignore_ascii_case("ssh")) {
        return None;
    }
    let hostname = get("HostName")?;
    // "user@host" in the HostName field is accepted by PuTTY too.
    let (user_in_host, hostname) = match hostname.split_once('@') {
        Some((u, h)) => (Some(u.to_string()), h.to_string()),
        None => (None, hostname),
    };
    Some(ImportedHost {
        alias: decode_name(name),
        hostname,
        port: get("PortNumber").and_then(|p| parse_num(&p)).filter(|p| *p > 0).and_then(|p| u16::try_from(p).ok()).unwrap_or(22),
        user: get("UserName").or(user_in_host),
        identity_file: get("PublicKeyFile"),
        proxy_jump: None,
        forward_agent: get("AgentFwd").and_then(|v| parse_num(&v)) == Some(1),
        forward_x11: get("X11Forward").and_then(|v| parse_num(&v)) == Some(1),
        group: None,
        proxy_command: match get("ProxyMethod").as_deref() {
            Some("5") => get("ProxyTelnetCommand"),
            _ => None,
        },
        keepalive_secs: get("PingInterval").and_then(|p| parse_num(&p)).filter(|p| *p > 0).and_then(|p| u32::try_from(p).ok()),
        forwards: Vec::new(),
    })
}

/// Parse `reg query HKCU\Software\SimonTatham\PuTTY\Sessions /s` output.
pub fn parse_reg_query(text: &str) -> Vec<ImportedHost> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut kv = BTreeMap::new();
    let flush = |name: &mut Option<String>, kv: &mut BTreeMap<String, String>, out: &mut Vec<ImportedHost>| {
        if let Some(n) = name.take() {
            if let Some(h) = session_to_host(&n, kv) {
                out.push(h);
            }
        }
        kv.clear();
    };
    for line in text.lines() {
        let l = line.trim_end();
        if let Some(rest) = l.rsplit_once("\\Sessions\\") {
            flush(&mut name, &mut kv, &mut out);
            name = Some(rest.1.to_string());
        } else if let Some(n) = &name {
            let _ = n;
            let parts: Vec<&str> = l.trim().splitn(3, "    ").collect();
            if parts.len() == 3 {
                kv.insert(parts[0].trim().to_string(), parts[2].trim().to_string());
            }
        }
    }
    flush(&mut name, &mut kv, &mut out);
    out
}

/// Parse a `~/.putty/sessions/<name>` file (`Key=Value` lines).
pub fn parse_session_file(name: &str, text: &str) -> Option<ImportedHost> {
    let kv: BTreeMap<String, String> = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.to_string()))
        .collect();
    session_to_host(name, &kv)
}

/// Every saved session on this computer.
pub fn saved_sessions(home: &Path) -> Vec<ImportedHost> {
    #[cfg(windows)]
    {
        let _ = home;
        let out = std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\SimonTatham\PuTTY\Sessions", "/s"])
            .output();
        match out {
            Ok(o) => parse_reg_query(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        let dir = home.join(".putty").join("sessions");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<ImportedHost> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(e.path()).ok()?;
                parse_session_file(&name, &text)
            })
            .collect();
        out.sort_by(|a, b| a.alias.cmp(&b.alias));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_output_becomes_hosts() {
        let text = "\r\nHKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\Prod%20DB\r\n    HostName    REG_SZ    db.example.com\r\n    PortNumber    REG_DWORD    0x8ae\r\n    UserName    REG_SZ    ops\r\n    Protocol    REG_SZ    ssh\r\n    AgentFwd    REG_DWORD    0x1\r\n\r\nHKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\serial-thing\r\n    HostName    REG_SZ    COM3\r\n    Protocol    REG_SZ    serial\r\n";
        let hosts = parse_reg_query(text);
        assert_eq!(hosts.len(), 1, "{hosts:?}");
        assert_eq!(hosts[0].alias, "Prod DB");
        assert_eq!(hosts[0].hostname, "db.example.com");
        assert_eq!(hosts[0].user.as_deref(), Some("ops"));
        assert!(hosts[0].forward_agent);
    }

    #[test]
    fn dword_ports_and_session_files() {
        let h = parse_session_file("web%2D01", "HostName=ops@10.0.0.5\nPortNumber=2222\nProtocol=ssh\nPublicKeyFile=/home/me/.ssh/id.ppk\n").unwrap();
        assert_eq!((h.alias.as_str(), h.hostname.as_str(), h.port), ("web-01", "10.0.0.5", 2222));
        assert_eq!(h.user.as_deref(), Some("ops"));
        assert!(h.identity_file.as_deref().unwrap().ends_with(".ppk"));
    }
}
