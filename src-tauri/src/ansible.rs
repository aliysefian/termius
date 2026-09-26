//! Import hosts from an Ansible INI inventory.
//!
//! Supported: groups, `[group:vars]`, `[group:children]`, `[all:vars]`,
//! hosts before any section (`ungrouped`), numeric and alphabetic ranges
//! (`web[01:03]`, `db-[a:c]`), inline host variables, quoted values, and the
//! connection variables `ansible_host`, `ansible_port`, `ansible_user`,
//! `ansible_ssh_private_key_file` (plus their legacy `ansible_ssh_*` names).
//! A `ProxyJump` inside `ansible_ssh_common_args` becomes the jump host.
//!
//! Variable precedence follows Ansible: host vars, then the host's group,
//! then that group's parents, then `all`.
//!
//! Hosts land in folders mirroring the group hierarchy, e.g. `prod/web`.

use std::collections::HashMap;
use std::path::Path;

use crate::sshconfig::{ImportedHost, ParseResult};

type Vars = HashMap<String, String>;

#[derive(Default)]
struct Inventory {
    /// host → (first group it appeared in, its inline vars), in file order.
    hosts: Vec<(String, String, Vars)>,
    group_vars: HashMap<String, Vars>,
    /// child group → first parent group.
    parent: HashMap<String, String>,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.to_string();
        }
    }
    v.to_string()
}

/// Split `key=value key2="a b"` respecting quotes.
fn split_vars(s: &str) -> Vars {
    let mut out = Vars::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut tokens = Vec::new();
    for c in s.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    for t in tokens {
        if let Some((k, v)) = t.split_once('=') {
            out.insert(k.trim().to_string(), unquote(v));
        }
    }
    out
}

/// Expand `web[01:03].x` → web01.x, web02.x, web03.x. Multiple ranges nest.
pub fn expand_range(pattern: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (pattern.find('['), pattern.find(']')) else {
        return vec![pattern.to_string()];
    };
    if close < open {
        return vec![pattern.to_string()];
    }
    let (pre, spec, post) = (
        &pattern[..open],
        &pattern[open + 1..close],
        &pattern[close + 1..],
    );
    let Some((a, b)) = spec.split_once(':') else {
        return vec![pattern.to_string()];
    };
    let mut items = Vec::new();
    if let (Ok(x), Ok(y)) = (a.parse::<u64>(), b.parse::<u64>()) {
        let width = if a.len() > 1 && a.starts_with('0') {
            a.len()
        } else {
            0
        };
        if y >= x && y - x <= 10_000 {
            for n in x..=y {
                items.push(format!("{n:0width$}"));
            }
        }
    } else if a.len() == 1 && b.len() == 1 {
        let (x, y) = (a.as_bytes()[0], b.as_bytes()[0]);
        if y >= x {
            items.extend((x..=y).map(|c| (c as char).to_string()));
        }
    }
    if items.is_empty() {
        return vec![pattern.to_string()];
    }
    items
        .into_iter()
        .flat_map(|i| expand_range(&format!("{pre}{i}{post}")))
        .collect()
}

fn parse_inventory(text: &str, warnings: &mut Vec<String>) -> Inventory {
    enum Section {
        Hosts(String),
        Vars(String),
        Children(String),
    }
    let mut inv = Inventory::default();
    let mut section = Section::Hosts("ungrouped".into());
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = match name.split_once(':') {
                Some((g, "vars")) => Section::Vars(g.to_string()),
                Some((g, "children")) => Section::Children(g.to_string()),
                Some((_, other)) => {
                    warnings.push(format!(
                        "line {}: unknown section suffix \":{other}\"",
                        lineno + 1
                    ));
                    Section::Hosts(name.to_string())
                }
                None => Section::Hosts(name.to_string()),
            };
            continue;
        }
        match &section {
            Section::Hosts(group) => {
                let (pattern, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
                let vars = split_vars(rest);
                for host in expand_range(pattern) {
                    if !inv.hosts.iter().any(|(h, _, _)| *h == host) {
                        inv.hosts.push((host, group.clone(), vars.clone()));
                    }
                }
            }
            Section::Vars(group) => {
                let v = split_vars(line);
                inv.group_vars.entry(group.clone()).or_default().extend(v);
            }
            Section::Children(group) => {
                inv.parent
                    .entry(line.to_string())
                    .or_insert_with(|| group.clone());
            }
        }
    }
    inv
}

fn chain(inv: &Inventory, group: &str) -> Vec<String> {
    let mut out = vec![group.to_string()];
    let mut cur = group;
    while let Some(p) = inv.parent.get(cur) {
        if out.contains(p) || out.len() > 32 {
            break;
        }
        out.push(p.clone());
        cur = p;
    }
    out
}

fn proxy_jump_from_args(args: &str) -> Option<String> {
    let mut it = args.split_whitespace().peekable();
    while let Some(tok) = it.next() {
        let value = if tok == "-J" {
            it.next().map(str::to_string)
        } else if let Some(v) = tok.strip_prefix("-J") {
            Some(v.to_string())
        } else if tok == "-o" {
            it.next().and_then(|o| {
                o.split_once('=')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("proxyjump"))
                    .map(|(_, v)| v.to_string())
            })
        } else {
            tok.strip_prefix("-oProxyJump=").map(str::to_string)
        };
        if let Some(v) = value {
            let v = v.trim_matches(|c| c == '"' || c == '\'');
            let host = v.rsplit('@').next().unwrap_or(v);
            return Some(host.split(':').next().unwrap_or(host).to_string());
        }
    }
    None
}

fn expand_home(p: &str, home: &Path) -> String {
    match p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        Some(rest) => rest
            .split(['/', '\\'])
            .filter(|c| !c.is_empty())
            .fold(home.to_path_buf(), |acc, c| acc.join(c))
            .to_string_lossy()
            .into_owned(),
        None => p.to_string(),
    }
}

pub fn parse(text: &str, home: &Path) -> ParseResult {
    let mut result = ParseResult::default();
    let inv = parse_inventory(text, &mut result.warnings);
    let empty = Vars::new();
    let all = inv.group_vars.get("all").unwrap_or(&empty);

    for (host, group, host_vars) in &inv.hosts {
        let groups = chain(&inv, group);
        let get = |keys: &[&str]| -> Option<String> {
            for k in keys {
                if let Some(v) = host_vars.get(*k) {
                    return Some(v.clone());
                }
            }
            for g in &groups {
                if let Some(gv) = inv.group_vars.get(g) {
                    for k in keys {
                        if let Some(v) = gv.get(*k) {
                            return Some(v.clone());
                        }
                    }
                }
            }
            keys.iter().find_map(|k| all.get(*k).cloned())
        };

        let port = match get(&["ansible_port", "ansible_ssh_port"]) {
            None => 22,
            Some(p) => p.parse().unwrap_or_else(|_| {
                result
                    .warnings
                    .push(format!("{host}: invalid port \"{p}\", using 22"));
                22
            }),
        };
        // Folder path from the outermost parent down, skipping "ungrouped".
        let path: Vec<&str> = groups
            .iter()
            .rev()
            .map(String::as_str)
            .filter(|g| *g != "ungrouped" && *g != "all")
            .collect();
        result.hosts.push(ImportedHost {
            alias: host.clone(),
            hostname: get(&["ansible_host", "ansible_ssh_host"]).unwrap_or_else(|| host.clone()),
            port,
            user: get(&["ansible_user", "ansible_ssh_user"]),
            identity_file: get(&["ansible_ssh_private_key_file", "ansible_private_key_file"])
                .map(|p| expand_home(&p, home)),
            proxy_jump: get(&["ansible_ssh_common_args", "ansible_ssh_extra_args"])
                .and_then(|a| proxy_jump_from_args(&a)),
            forward_agent: false,
            forward_x11: false,
            group: (!path.is_empty()).then(|| path.join("/")),
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const INV: &str = r#"
bastion ansible_host=203.0.113.10 ansible_user=ops

[web]
web[01:03].example.com
web-special ansible_host=10.0.0.9 ansible_port=2222 ansible_user="deploy user"

[db]
db-[a:b] ansible_ssh_host=10.0.1.1

[web:vars]
ansible_user=deploy
ansible_ssh_private_key_file=~/.ssh/deploy
ansible_ssh_common_args='-o ProxyJump=ops@bastion:22'

[prod:children]
web
db

[all:vars]
ansible_user=admin
"#;

    fn get<'a>(r: &'a ParseResult, a: &str) -> &'a ImportedHost {
        r.hosts
            .iter()
            .find(|h| h.alias == a)
            .unwrap_or_else(|| panic!("no {a}"))
    }

    #[test]
    fn ranges_expand_with_padding_and_letters() {
        assert_eq!(
            expand_range("web[01:03].x"),
            ["web01.x", "web02.x", "web03.x"]
        );
        assert_eq!(expand_range("db-[a:c]"), ["db-a", "db-b", "db-c"]);
        assert_eq!(
            expand_range("n[1:2]-[a:b]"),
            ["n1-a", "n1-b", "n2-a", "n2-b"]
        );
        assert_eq!(expand_range("plain"), ["plain"]);
        assert_eq!(expand_range("bad[3:1]"), ["bad[3:1]"]);
    }

    #[test]
    fn resolves_vars_groups_and_jumps() {
        let r = parse(INV, Path::new("/home/me"));
        assert_eq!(r.hosts.len(), 7);

        let b = get(&r, "bastion");
        assert_eq!(b.hostname, "203.0.113.10");
        assert_eq!(b.user.as_deref(), Some("ops"));
        assert_eq!(b.group, None);

        let w = get(&r, "web02.example.com");
        assert_eq!(w.hostname, "web02.example.com");
        assert_eq!(w.user.as_deref(), Some("deploy")); // group var beats all:vars
        assert_eq!(w.group.as_deref(), Some("prod/web"));
        assert_eq!(w.proxy_jump.as_deref(), Some("bastion"));
        let expected_key = Path::new("/home/me").join(".ssh").join("deploy");
        assert_eq!(
            w.identity_file.as_deref(),
            Some(expected_key.to_string_lossy().as_ref())
        );

        let s = get(&r, "web-special");
        assert_eq!((s.hostname.as_str(), s.port), ("10.0.0.9", 2222));
        assert_eq!(s.user.as_deref(), Some("deploy user")); // host var, quoted

        let d = get(&r, "db-a");
        assert_eq!(d.hostname, "10.0.1.1"); // legacy ansible_ssh_host
        assert_eq!(d.user.as_deref(), Some("admin")); // falls through to all:vars
        assert_eq!(d.group.as_deref(), Some("prod/db"));
        assert_eq!(d.proxy_jump, None);
    }

    #[test]
    fn proxy_jump_forms() {
        assert_eq!(
            proxy_jump_from_args("-J bastion").as_deref(),
            Some("bastion")
        );
        assert_eq!(proxy_jump_from_args("-Jme@b:2200").as_deref(), Some("b"));
        assert_eq!(
            proxy_jump_from_args("-o StrictHostKeyChecking=no -o ProxyJump=j").as_deref(),
            Some("j")
        );
        assert_eq!(
            proxy_jump_from_args("-oProxyJump=\"x\"").as_deref(),
            Some("x")
        );
        assert_eq!(proxy_jump_from_args("-o ProxyCommand=nc"), None);
    }
}
