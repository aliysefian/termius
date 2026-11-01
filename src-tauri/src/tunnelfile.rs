//! Tunnel definitions as a file, to move between computers or keep as a backup. The file names the host of each
//! tunnel (by its label) and holds the forwarding, and nothing else: no credentials, and nothing about whether a
//! tunnel was running. Importing matches hosts by label, skips what it already has, and says what it skipped.

use serde::{Deserialize, Serialize};

use crate::models::{ForwardKind, ForwardRule};

pub const FORMAT: &str = "sshvault-tunnels";
pub const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    tunnels: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The label of the host the tunnel goes through.
    pub host: String,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(flatten)]
    pub kind: ForwardKind,
}

/// The text of a tunnel file for these rules; `host_label` names a rule's host (`None` for one whose host is gone,
/// which is left out).
pub fn export(rules: &[ForwardRule], host_label: impl Fn(&ForwardRule) -> Option<String>) -> String {
    let tunnels = rules
        .iter()
        .filter_map(|r| {
            Some(Entry { name: r.label.clone(), description: r.description.clone(), host: host_label(r)?, auto_start: r.auto_start, kind: r.kind.clone() })
        })
        .collect();
    serde_json::to_string_pretty(&File { format: FORMAT.into(), version: VERSION, tunnels }).unwrap_or_default() + "\n"
}

/// The entries of a tunnel file, or why it isn't one.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let file: File = serde_json::from_str(text).map_err(|e| format!("this isn't a tunnel file: {e}"))?;
    if file.format != FORMAT {
        return Err("this isn't a tunnel file".into());
    }
    if file.version > VERSION {
        return Err(format!("this tunnel file is from a newer SSHVault (version {}); update to read it", file.version));
    }
    Ok(file.tunnels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn rule(label: &str, kind: ForwardKind) -> ForwardRule {
        ForwardRule { label: label.into(), host_id: Uuid::nil(), auto_start: true, description: "d".into(), kind }
    }

    #[test]
    fn round_trips_every_kind_without_ids_or_state() {
        let rules = vec![
            rule("web", ForwardKind::Local { bind_addr: "127.0.0.1".into(), bind_port: 8000, dest_host: "127.0.0.1".into(), dest_port: 80 }),
            rule("expose", ForwardKind::Remote { bind_addr: "0.0.0.0".into(), bind_port: 1420, dest_host: "127.0.0.1".into(), dest_port: 1420 }),
            rule("socks", ForwardKind::Dynamic { bind_addr: "127.0.0.1".into(), bind_port: 1080 }),
            rule("orphan", ForwardKind::Dynamic { bind_addr: "127.0.0.1".into(), bind_port: 1081 }),
        ];
        let text = export(&rules, |r| (r.label != "orphan").then(|| "claude".to_string()));
        assert!(!text.contains("host_id") && !text.contains("orphan"), "{text}");
        let back = parse(&text).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!((back[0].name.as_str(), back[0].host.as_str(), back[0].auto_start), ("web", "claude", true));
        assert_eq!(back[1].kind, rules[1].kind);
        assert_eq!(back[2].kind, rules[2].kind);
    }

    #[test]
    fn refuses_what_is_not_a_tunnel_file() {
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"format":"something-else","version":1,"tunnels":[]}"#).unwrap_err().contains("isn't a tunnel file"));
        assert!(parse(r#"{"format":"sshvault-tunnels","version":99,"tunnels":[]}"#).unwrap_err().contains("newer"));
        assert!(parse(r#"{"format":"sshvault-tunnels","version":1,"tunnels":[]}"#).unwrap().is_empty());
    }
}
