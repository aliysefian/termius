//! Import and export for Ásbrú Connection Manager (`~/.config/asbru/asbru.yml`, or a file from its Export dialog).
//!
//! The file is YAML with an `environments` map. Each entry is a connection or, with `_is_group: 1`, a folder; the
//! tree is rebuilt from each entry's `parent` (the top level is `__PAC__ROOT__`):
//!
//! ```yaml
//! environments:
//!   3f2a…:
//!     _is_group: 1
//!     name: Production
//!     parent: __PAC__ROOT__
//!   9c1b…:
//!     _is_group: 0
//!     method: SSH
//!     name: web-01
//!     ip: 10.0.0.5
//!     port: 22
//!     user: ops
//!     auth type: publickey
//!     public key: /home/me/.ssh/id_ed25519
//!     parent: 3f2a…
//! ```
//!
//! Only SSH connections are imported. Saved passwords and passphrases are left in the file: they're Ásbrú's to
//! manage, and the import warns how many it ignored. There's no YAML crate in the build, so [`yaml`] reads the block
//! mappings, quoted scalars and flow-empty values Ásbrú writes, and skips what it doesn't need (lists, anchors).

use std::collections::{BTreeMap, HashMap};

use crate::interchange::{ExportHost, HostFormat, ImportContext, Parsed};
use crate::sshconfig::ImportedHost;

const ROOT: &str = "__PAC__ROOT__";

mod yaml {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Node {
        Null,
        Str(String),
        Map(Vec<(String, Node)>),
    }

    impl Node {
        pub fn get(&self, key: &str) -> Option<&Node> {
            match self {
                Node::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }
        /// A scalar's text; `None` for null, maps and missing keys.
        pub fn text(&self, key: &str) -> Option<&str> {
            match self.get(key)? {
                Node::Str(s) => Some(s.as_str()),
                _ => None,
            }
        }
    }

    struct Line<'a> {
        indent: usize,
        text: &'a str,
    }

    fn lines(text: &str) -> Vec<Line<'_>> {
        text.lines()
            .filter_map(|raw| {
                let t = raw.trim_end();
                let body = t.trim_start();
                if body.is_empty() || body.starts_with('#') || body == "---" || body == "..." {
                    return None;
                }
                Some(Line { indent: t.len() - body.len(), text: body })
            })
            .collect()
    }

    pub fn parse(text: &str) -> Node {
        let lines = lines(text);
        match lines.first() {
            Some(first) => parse_map(&lines, &mut 0, first.indent),
            None => Node::Null,
        }
    }

    fn skip_deeper(lines: &[Line], pos: &mut usize, indent: usize) {
        while *pos < lines.len() && lines[*pos].indent > indent {
            *pos += 1;
        }
    }

    fn is_item(text: &str) -> bool {
        text == "-" || text.starts_with("- ")
    }

    fn parse_map(lines: &[Line], pos: &mut usize, indent: usize) -> Node {
        let mut entries = Vec::new();
        while *pos < lines.len() {
            let line = &lines[*pos];
            if line.indent < indent {
                break;
            }
            *pos += 1;
            if line.indent > indent || is_item(line.text) {
                // Stray deeper line, or a list item: not something we read.
                skip_deeper(lines, pos, line.indent.min(indent));
                continue;
            }
            let Some((key, rest)) = split_key(line.text) else {
                skip_deeper(lines, pos, indent);
                continue;
            };
            let rest = rest.trim();
            let value = if rest.is_empty() {
                match lines.get(*pos) {
                    Some(next) if next.indent > indent && !is_item(next.text) => parse_map(lines, pos, next.indent),
                    _ => {
                        skip_deeper(lines, pos, indent);
                        Node::Null
                    }
                }
            } else if rest.starts_with('|') || rest.starts_with('>') {
                skip_deeper(lines, pos, indent);
                Node::Str(String::new())
            } else {
                // A quoted scalar may continue on deeper lines; they're not needed.
                skip_deeper(lines, pos, indent);
                scalar(rest)
            };
            entries.push((key, value));
        }
        Node::Map(entries)
    }

    /// `key: rest`, for plain and quoted keys.
    fn split_key(text: &str) -> Option<(String, &str)> {
        if text.starts_with('\'') || text.starts_with('"') {
            let (key, after) = read_quoted(text)?;
            let rest = after.trim_start().strip_prefix(':')?;
            return Some((key, rest));
        }
        let b = text.as_bytes();
        (0..b.len())
            .find(|&i| b[i] == b':' && (i + 1 == b.len() || b[i + 1] == b' '))
            .map(|i| (text[..i].trim().to_string(), &text[i + 1..]))
    }

    fn scalar(rest: &str) -> Node {
        if rest.starts_with('\'') || rest.starts_with('"') {
            return read_quoted(rest).map(|(s, _)| Node::Str(s)).unwrap_or(Node::Null);
        }
        let v = rest.split(" #").next().unwrap_or(rest).trim();
        match v {
            "" | "~" | "null" | "{}" | "[]" => Node::Null,
            _ => Node::Str(v.to_string()),
        }
    }

    /// A quoted scalar at the start of `text`, and what follows its closing quote.
    fn read_quoted(text: &str) -> Option<(String, &str)> {
        let q = text.chars().next()?;
        let body = &text[1..];
        let mut out = String::new();
        let mut it = body.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            if c == q {
                if q == '\'' && matches!(it.peek(), Some((_, '\''))) {
                    it.next();
                    out.push('\'');
                    continue;
                }
                return Some((out, &body[i + 1..]));
            }
            if q == '"' && c == '\\' {
                match it.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| it.next().map(|(_, c)| c)).collect();
                        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            out.push(ch);
                        }
                    }
                    other => out.push(other),
                }
                continue;
            }
            out.push(c);
        }
        None
    }
}

use yaml::Node;

/// A folder's name and its parent's id.
type Groups = HashMap<String, (String, String)>;

fn group_path(id: &str, groups: &Groups) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut cur = id;
    // Bounded, so a parent loop in a damaged file can't hang the import.
    for _ in 0..32 {
        let Some((name, parent)) = groups.get(cur) else { break };
        parts.push(name.as_str());
        cur = parent.as_str();
    }
    parts.reverse();
    parts.join("/")
}

pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let root = yaml::parse(text);
    // A full config has `environments`; some exports are the map itself.
    let envs = root.get("environments").unwrap_or(&root);
    let Node::Map(entries) = envs else {
        out.warnings.push("No Ásbrú connections were found in that file.".into());
        return out;
    };
    let is_entry = |n: &Node| matches!(n, Node::Map(_)) && n.get("_is_group").is_some();
    let is_group = |n: &Node| n.text("_is_group").is_some_and(|v| v.trim() == "1");

    let groups: Groups = entries
        .iter()
        .filter(|(id, n)| id != ROOT && is_entry(n) && is_group(n))
        .map(|(id, n)| {
            let name = n.text("name").map(|s| s.trim().replace('/', "-")).filter(|s| !s.is_empty()).unwrap_or_else(|| "Group".into());
            (id.clone(), (name, n.text("parent").unwrap_or(ROOT).to_string()))
        })
        .collect();

    let (mut skipped_other, mut passwords) = (0usize, 0usize);
    for (id, n) in entries.iter().filter(|(_, n)| is_entry(n) && !is_group(n)) {
        let name = n.text("name").map(str::trim).filter(|s| !s.is_empty()).or_else(|| n.text("title").map(str::trim)).unwrap_or(id);
        if !n.text("method").unwrap_or("SSH").trim().eq_ignore_ascii_case("ssh") {
            skipped_other += 1;
            continue;
        }
        let host = n.text("ip").map(str::trim).unwrap_or_default();
        if host.is_empty() {
            out.warnings.push(format!("{name}: no host name; skipped"));
            continue;
        }
        if n.text("pass").is_some_and(|p| !p.is_empty()) || n.text("passphrase").is_some_and(|p| !p.is_empty()) {
            passwords += 1;
        }
        let auth = n.text("auth type").or_else(|| n.text("auth_type")).unwrap_or_default();
        let key = n.text("public key").map(str::trim).filter(|k| !k.is_empty() && auth.trim() == "publickey");
        let path = group_path(n.text("parent").unwrap_or(ROOT), &groups);
        out.hosts.push(ImportedHost {
            alias: name.to_string(),
            hostname: host.to_string(),
            port: n.text("port").and_then(|p| p.trim().parse::<u16>().ok()).filter(|p| *p > 0).unwrap_or(22),
            user: n.text("user").map(str::trim).filter(|u| !u.is_empty()).map(str::to_string),
            identity_file: key.map(str::to_string),
            group: (!path.is_empty()).then_some(path),
            ..Default::default()
        });
    }
    if skipped_other > 0 {
        out.warnings.push(format!("{skipped_other} non-SSH connection(s) (RDP, VNC, commands, …) were skipped"));
    }
    if passwords > 0 {
        out.warnings.push(format!("{passwords} saved password(s) or passphrase(s) were not imported; enter them in SSHVault"));
    }
    out
}

/// A single-quoted YAML scalar. Line breaks become spaces so one value stays on one line.
fn q(s: &str) -> String {
    format!("'{}'", s.replace(['\r', '\n'], " ").replace('\'', "''"))
}

/// A file for Ásbrú's Import: the folders as groups under the top level, then each host as an SSH connection.
/// No passwords are written. The `children` lists Ásbrú keeps on each folder are filled in as well.
pub fn export(hosts: &[ExportHost]) -> String {
    use std::fmt::Write as _;
    // Folder path -> id, with every ancestor present.
    let mut folders: BTreeMap<String, String> = BTreeMap::new();
    let segments = |group: &str| -> Vec<String> { group.split('/').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect() };
    for h in hosts {
        let mut path = String::new();
        for seg in segments(&h.group) {
            path = if path.is_empty() { seg } else { format!("{path}/{seg}") };
            folders.entry(path.clone()).or_insert_with(|| uuid::Uuid::new_v4().to_string());
        }
    }
    let parent_of = |path: &str| -> String { path.rsplit_once('/').and_then(|(p, _)| folders.get(p).cloned()).unwrap_or_else(|| ROOT.to_string()) };

    // Hosts get ids up front so folders can list them.
    let host_ids: Vec<String> = hosts.iter().map(|_| uuid::Uuid::new_v4().to_string()).collect();
    let mut children: HashMap<String, Vec<&str>> = HashMap::new();
    for (path, id) in &folders {
        children.entry(parent_of(path)).or_default().push(id);
    }
    let group_id_of = |h: &ExportHost| -> String { segments(&h.group).join("/") };
    let host_parent: Vec<String> = hosts.iter().map(|h| folders.get(&group_id_of(h)).cloned().unwrap_or_else(|| ROOT.to_string())).collect();
    for (id, parent) in host_ids.iter().zip(&host_parent) {
        children.entry(parent.clone()).or_default().push(id);
    }

    let mut out = String::from("---\n__PAC__EXPORTED__PARTIAL__: 1\nenvironments:\n");
    let kids = |out: &mut String, id: &str| match children.get(id) {
        Some(c) if !c.is_empty() => {
            out.push_str("    children:\n");
            for k in c {
                let _ = writeln!(out, "      {k}: 1");
            }
        }
        _ => out.push_str("    children: {}\n"),
    };
    for (path, id) in &folders {
        let name = path.rsplit('/').next().unwrap_or(path);
        let _ = write!(out, "  {id}:\n    _is_group: 1\n    _protected: 0\n");
        kids(&mut out, id);
        let _ = write!(out, "    description: ''\n    name: {}\n    parent: {}\n", q(name), parent_of(path));
    }
    for ((h, id), parent) in hosts.iter().zip(&host_ids).zip(&host_parent) {
        let name = if h.name.trim().is_empty() { h.hostname.trim() } else { h.name.trim() };
        let key = h.key_file.as_deref().filter(|k| !k.is_empty());
        let _ = write!(out, "  {id}:\n    _is_group: 0\n    _protected: 0\n");
        kids(&mut out, id);
        let _ = write!(
            out,
            "    auth type: {}\n    description: ''\n    ip: {}\n    method: SSH\n    name: {}\n    options: ''\n    parent: {parent}\n    pass: ''\n    passphrase: ''\n    passphrase user: ''\n    port: {}\n    public key: {}\n    title: {}\n    user: {}\n",
            if key.is_some() { "publickey" } else { "userpass" },
            q(h.hostname.trim()),
            q(name),
            if h.port == 0 { 22 } else { h.port },
            q(key.unwrap_or_default()),
            q(name),
            q(h.user.as_deref().unwrap_or_default()),
        );
    }
    out
}

pub struct Asbru;

impl HostFormat for Asbru {
    fn id(&self) -> &'static str {
        "asbru"
    }
    fn name(&self) -> &'static str {
        "Ásbrú Connection Manager"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["yml", "yaml"]
    }
    fn parse(&self, text: &str, _ctx: &ImportContext) -> Parsed {
        parse(text)
    }
    fn export(&self, hosts: &[ExportHost]) -> String {
        export(hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YML: &str = r#"---
__PAC__EXPORTED__FULL__: 1
config:
  tabs position: top
defaults:
  ssh: {}
environments:
  __PAC__ROOT__:
    _is_group: 1
    children:
      grp1: 1
      c1: 1
    name: My Connections
  grp1:
    _is_group: 1
    children:
      grp2: 1
    name: Production
    parent: __PAC__ROOT__
  grp2:
    _is_group: 1
    name: Databases
    parent: grp1
  c1:
    _is_group: 0
    auth type: userpass
    ip: 10.0.0.5
    method: SSH
    name: web-01
    parent: __PAC__ROOT__
    pass: hunter2
    port: 22
    user: ops
  c2:
    _is_group: 0
    auth type: publickey
    ip: db.example.com
    method: SSH
    name: "db \"primary\""
    options: -o "Foo=bar"
    parent: grp2
    pass: ''
    port: '2222'
    public key: /home/me/.ssh/id_ed25519
    user: ''
  c3:
    _is_group: 0
    ip: 192.168.1.9
    method: RDP (xfreerdp)
    name: desk
    parent: __PAC__ROOT__
  c4:
    _is_group: 0
    method: SSH
    name: no-host
    parent: __PAC__ROOT__
  c5:
    _is_group: 0
    ip: bare.example.com
    name: bare
    parent: grp1
    some list:
    - a
    - b
"#;

    #[test]
    fn reads_folders_ssh_parameters_and_skips_the_rest() {
        let p = parse(YML);
        assert_eq!(p.hosts.len(), 3, "{p:?}");
        let web = &p.hosts[0];
        assert_eq!((web.alias.as_str(), web.hostname.as_str(), web.port, web.group.clone()), ("web-01", "10.0.0.5", 22, None));
        assert_eq!(web.user.as_deref(), Some("ops"));
        assert_eq!(web.identity_file, None, "userpass auth uses no key");
        let db = &p.hosts[1];
        assert_eq!(db.alias, "db \"primary\"");
        assert_eq!(db.port, 2222);
        assert_eq!(db.user, None);
        assert_eq!(db.group.as_deref(), Some("Production/Databases"));
        assert_eq!(db.identity_file.as_deref(), Some("/home/me/.ssh/id_ed25519"));
        let bare = &p.hosts[2];
        assert_eq!((bare.port, bare.group.as_deref()), (22, Some("Production")));
        assert!(p.warnings.iter().any(|w| w.contains("non-SSH")), "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("no-host")), "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("1 saved password")), "{:?}", p.warnings);
        // The password itself is nowhere in what was parsed.
        assert!(!format!("{:?}", p.hosts).contains("hunter2"));
    }

    #[test]
    fn export_round_trips_through_the_parser() {
        let hosts = vec![
            ExportHost { name: "web-01".into(), hostname: "10.0.0.5".into(), port: 22, user: Some("ops".into()), ..Default::default() },
            ExportHost { name: "it's: \"odd\"".into(), hostname: "db.example.com".into(), port: 2222, group: "Production/Databases".into(), key_file: Some("/home/me/.ssh/id_ed25519".into()), ..Default::default() },
            ExportHost { name: "cache".into(), hostname: "h".into(), port: 0, group: "Production".into(), ..Default::default() },
        ];
        let text = export(&hosts);
        let p = parse(&text);
        assert_eq!(p.warnings, Vec::<String>::new(), "{text}");
        assert_eq!(p.hosts.len(), 3, "{text}");
        let by = |n: &str| p.hosts.iter().find(|h| h.alias == n).unwrap_or_else(|| panic!("{n} in {text}"));
        assert_eq!((by("web-01").user.as_deref(), by("web-01").group.clone()), (Some("ops"), None));
        let odd = by("it's: \"odd\"");
        assert_eq!((odd.port, odd.group.as_deref(), odd.identity_file.as_deref()), (2222, Some("Production/Databases"), Some("/home/me/.ssh/id_ed25519")));
        assert_eq!((by("cache").port, by("cache").group.as_deref()), (22, Some("Production")));
    }

    #[test]
    fn export_writes_no_secrets_and_links_children() {
        let text = export(&[ExportHost { name: "a".into(), hostname: "h".into(), port: 22, group: "G".into(), ..Default::default() }]);
        assert!(text.contains("pass: ''"));
        assert!(text.contains("__PAC__ROOT__"));
        // The folder lists its host as a child.
        let host_id = text.lines().filter(|l| l.starts_with("  ") && !l.starts_with("   ") && l.ends_with(':')).nth(1).unwrap().trim().trim_end_matches(':').to_string();
        assert!(text.contains(&format!("      {host_id}: 1")), "{text}");
    }

    #[test]
    fn text_that_is_not_an_asbru_file_gives_a_warning_not_a_panic() {
        for t in ["", "just text", "- a\n- b\n", "environments: 3\n", "a: [unterminated\n"] {
            let p = parse(t);
            assert!(p.hosts.is_empty(), "{t:?}");
        }
        assert!(!parse("").warnings.is_empty());
    }
}
