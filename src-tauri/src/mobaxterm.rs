//! Import SSH bookmarks from MobaXterm (`MobaXterm.ini`, or an exported
//! `.mxtsessions` file). Each `[Bookmarks…]` section is a folder
//! (`SubRep=Parent\Child`); each SSH session is a line such as
//!
//! ```text
//! web-01=#109#0%10.0.0.5%22%ops%%-1%-1%%%%%0%0%0%_ProfileDir_\.ssh\id.ppk%…
//! ```
//!
//! where `109` means SSH and the `%` fields start with host, port and user.
//! Passwords aren't stored there (MobaXterm keeps them elsewhere, encrypted),
//! so none are imported.

use crate::sshconfig::ImportedHost;

const SSH: &str = "109";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub hosts: Vec<ImportedHost>,
    pub warnings: Vec<String>,
}

/// MobaXterm writes some characters in names as `__PIPE__`, `__DIEZE__`…
fn unescape(s: &str) -> String {
    s.replace("__PIPE__", "|")
        .replace("__DIEZE__", "#")
        .replace("__PTVIRG__", ";")
        .replace("__DBLQUO__", "\"")
        .replace("__APOS__", "'")
        .replace("__PERCENT__", "%")
}

/// A private-key path, if one of the fields looks like one we can use.
/// MobaXterm's `_ProfileDir_` / `_CurrentDrive_` placeholders are expanded
/// when `profile_dir` is known.
fn key_path(fields: &[&str], profile_dir: Option<&str>) -> Option<String> {
    fields.iter().find_map(|f| {
        let f = f.trim();
        let looks = f.ends_with(".ppk") || f.ends_with(".pem") || f.ends_with(".key") || f.contains("id_");
        if !looks || f.len() < 4 {
            return None;
        }
        let expanded = match (f.strip_prefix("_ProfileDir_"), profile_dir) {
            (Some(rest), Some(dir)) => format!("{dir}{rest}"),
            (Some(_), None) => return None,
            (None, _) => f.to_string(),
        };
        (!expanded.contains("_CurrentDrive_")).then_some(expanded)
    })
}

pub fn parse(text: &str, profile_dir: Option<&str>) -> Parsed {
    let mut out = Parsed::default();
    let mut in_bookmarks = false;
    let mut folder = String::new();
    let mut skipped_other = 0usize;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_bookmarks = line[1..line.len() - 1].starts_with("Bookmarks");
            folder.clear();
            continue;
        }
        if !in_bookmarks || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "SubRep" => {
                folder = value.replace('\\', "/").trim_matches('/').to_string();
                continue;
            }
            "ImgNum" => continue,
            _ => {}
        }
        // "#109#0%host%port%user%…"
        let Some(rest) = value.strip_prefix('#') else { continue };
        let Some((kind, params)) = rest.split_once('#') else { continue };
        if kind != SSH {
            skipped_other += 1;
            continue;
        }
        let fields: Vec<&str> = params.split('%').collect();
        let host = fields.get(1).map(|s| s.trim()).unwrap_or_default();
        if host.is_empty() {
            out.warnings.push(format!("{}: no host name; skipped", unescape(key)));
            continue;
        }
        let port = fields.get(2).and_then(|p| p.trim().parse::<u16>().ok()).filter(|p| *p > 0).unwrap_or(22);
        let user = fields.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()).map(str::to_string);
        out.hosts.push(ImportedHost {
            alias: unescape(key.trim()),
            hostname: host.to_string(),
            port,
            user,
            identity_file: key_path(&fields[4.min(fields.len())..], profile_dir),
            group: (!folder.is_empty()).then(|| folder.clone()),
            ..Default::default()
        });
    }
    if skipped_other > 0 {
        out.warnings.push(format!(
            "{skipped_other} non-SSH bookmark(s) (RDP, VNC, serial, …) were skipped"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = r#"
[Misc]
Foo=bar

[Bookmarks]
SubRep=
ImgNum=42
web-01=#109#0%10.0.0.5%22%ops%%-1%-1%%%%%0%0%0%_ProfileDir_\.ssh\id_ed25519%%-1%0%0%0%%1080%%0%0%1
desk=#91#4%192.168.1.9%3389%me%0%-1%-1

[Bookmarks_1]
SubRep=Production\Databases
ImgNum=41
db__PIPE__primary=#109#0%db.example.com%2222%%%-1%-1%%%%%0%0%0%%%-1
broken=#109#0%%22%x
"#;

    #[test]
    fn ssh_bookmarks_with_folders_ports_users_and_keys() {
        let p = parse(INI, Some("C:\\Users\\me"));
        assert_eq!(p.hosts.len(), 2, "{p:?}");
        let web = &p.hosts[0];
        assert_eq!((web.alias.as_str(), web.hostname.as_str(), web.port), ("web-01", "10.0.0.5", 22));
        assert_eq!(web.user.as_deref(), Some("ops"));
        assert_eq!(web.identity_file.as_deref(), Some("C:\\Users\\me\\.ssh\\id_ed25519"));
        assert_eq!(web.group, None);
        let db = &p.hosts[1];
        assert_eq!((db.alias.as_str(), db.port), ("db|primary", 2222));
        assert_eq!(db.user, None);
        assert_eq!(db.group.as_deref(), Some("Production/Databases"));
        assert!(p.warnings.iter().any(|w| w.contains("non-SSH")), "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("broken")));
    }

    #[test]
    fn placeholders_without_a_profile_dir_are_dropped() {
        let p = parse(INI, None);
        assert_eq!(p.hosts[0].identity_file, None);
    }
}
