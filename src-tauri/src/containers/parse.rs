//! Reading the JSON that `docker`, `podman` and `nerdctl` print for `ps` and
//! `images`. Pure functions, tested against recorded output.
//!
//! Docker and nerdctl print one JSON object per line (`--format '{{json .}}'`),
//! Podman prints one array (`--format json`). Both are accepted whichever
//! runtime is asked, because versions differ. Fields differ in shape too (a
//! name is a string or a list, ports a string or a list of objects, labels a
//! string or a map), so every field is read tolerantly: a missing or odd
//! field leaves that field empty rather than failing the whole list.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Paused,
    Restarting,
    Exited,
    Created,
    Dead,
    Removing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortMapping {
    /// Empty when the port is exposed but not published.
    pub host_ip: String,
    /// A number, or a range like `8000-8010`; empty when not published.
    pub host_port: String,
    pub container_port: String,
    pub proto: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Container {
    /// The full ID.
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: State,
    /// The runtime's own words, e.g. "Up 4 seconds (Paused)".
    pub status: String,
    pub ports: Vec<PortMapping>,
    /// Seconds since the Unix epoch.
    pub created: Option<i64>,
    /// Only present when sizes were asked for, e.g. "49.2kB (virtual 52.7MB)".
    pub size: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub command: String,
    /// Podman pod name, if the container is in one.
    pub pod: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Image {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size_text: String,
    pub size_bytes: Option<u64>,
    pub created: Option<i64>,
    /// Containers using it, when the runtime says.
    pub containers: Option<u32>,
}

// -- entry points -----------------------------------------------------------

/// Every JSON object in `stdout`, whether it is one array or one per line.
fn objects(stdout: &str) -> Result<Vec<Value>, String> {
    let t = stdout.trim();
    if t.is_empty() {
        return Ok(Vec::new());
    }
    if t.starts_with('[') {
        let v: Value = serde_json::from_str(t).map_err(|e| format!("unreadable list from the runtime: {e}"))?;
        return Ok(v.as_array().cloned().unwrap_or_default());
    }
    let mut out = Vec::new();
    for (i, line) in t.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line).map_err(|e| format!("unreadable list from the runtime (line {}): {e}", i + 1))?;
        match v {
            Value::Object(_) => out.push(v),
            // Some versions wrap the whole list on a single line.
            Value::Array(items) => out.extend(items),
            _ => {}
        }
    }
    Ok(out)
}

pub fn parse_containers(stdout: &str) -> Result<Vec<Container>, String> {
    Ok(objects(stdout)?.iter().filter_map(container_from).collect())
}

pub fn parse_images(stdout: &str) -> Result<Vec<Image>, String> {
    Ok(objects(stdout)?.iter().flat_map(images_from).collect())
}

// -- field readers ----------------------------------------------------------

/// The first present key, as text. A list takes its first item.
fn text(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => return s.clone(),
            Some(Value::Array(a)) => {
                if let Some(Value::String(s)) = a.first() {
                    return s.clone();
                }
            }
            Some(Value::Number(n)) => return n.to_string(),
            _ => {}
        }
    }
    String::new()
}

fn state_from(s: &str) -> State {
    match s.trim().to_ascii_lowercase().as_str() {
        "running" | "stopping" | "up" => State::Running,
        "paused" => State::Paused,
        "restarting" => State::Restarting,
        // Podman says "stopped" for a container that exited.
        "exited" | "stopped" => State::Exited,
        "created" | "configured" | "initialized" => State::Created,
        "dead" => State::Dead,
        "removing" => State::Removing,
        _ => State::Unknown,
    }
}

fn container_from(v: &Value) -> Option<Container> {
    let id = text(v, &["ID", "Id", "id"]);
    if id.is_empty() {
        return None;
    }
    // Names may be "a,b" (docker) or ["a", "b"] (podman); show the first.
    let name = text(v, &["Names", "Name"]).split(',').next().unwrap_or("").trim_start_matches('/').to_string();
    let created = v
        .get("Created")
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .or_else(|| v.get("CreatedAt").and_then(Value::as_str).and_then(parse_time));
    let size = v.get("Size").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let pod = text(v, &["PodName"]);
    Some(Container {
        id,
        name,
        image: text(v, &["Image"]),
        state: state_from(&text(v, &["State"])),
        status: text(v, &["Status"]),
        ports: ports_from(v.get("Ports")),
        created,
        size,
        labels: labels_from(v.get("Labels")),
        command: command_from(v.get("Command")),
        pod: (!pod.is_empty()).then_some(pod),
    })
}

fn command_from(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim_matches('"').to_string(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

fn labels_from(v: Option<&Value>) -> BTreeMap<String, String> {
    match v {
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect(),
        Some(Value::String(s)) => labels_from_text(s),
        _ => BTreeMap::new(),
    }
}

/// `a=b,c=d`. A value may itself contain commas; a piece with no `=` belongs
/// to the previous value.
pub fn labels_from_text(s: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    for piece in s.split(',') {
        match piece.split_once('=') {
            Some((k, v)) if !k.is_empty() && !k.contains(' ') => {
                out.insert(k.to_string(), v.to_string());
                last = Some(k.to_string());
            }
            _ => {
                if let Some(k) = &last {
                    if let Some(v) = out.get_mut(k) {
                        v.push(',');
                        v.push_str(piece);
                    }
                }
            }
        }
    }
    out
}

fn ports_from(v: Option<&Value>) -> Vec<PortMapping> {
    match v {
        Some(Value::String(s)) => ports_from_text(s),
        Some(Value::Array(a)) => a.iter().filter_map(port_from_object).collect(),
        _ => Vec::new(),
    }
}

fn port_from_object(p: &Value) -> Option<PortMapping> {
    let num = |k: &str| p.get(k).and_then(Value::as_u64).filter(|n| *n > 0).map(|n| n.to_string());
    let container_port = num("container_port")?;
    // `range` is how many consecutive ports the mapping covers.
    let span = p.get("range").and_then(Value::as_u64).filter(|n| *n > 1);
    let widen = |first: String| match (span, first.parse::<u64>()) {
        (Some(n), Ok(f)) => format!("{f}-{}", f + n - 1),
        _ => first,
    };
    Some(PortMapping {
        host_ip: p.get("host_ip").and_then(Value::as_str).unwrap_or("").to_string(),
        host_port: num("host_port").map(&widen).unwrap_or_default(),
        container_port: widen(container_port),
        proto: p.get("protocol").and_then(Value::as_str).unwrap_or("tcp").to_string(),
    })
}

/// `0.0.0.0:58432->5432/tcp, [::]:58432->5432/tcp, 8080/tcp`.
pub fn ports_from_text(s: &str) -> Vec<PortMapping> {
    s.split(", ")
        .filter_map(|piece| {
            let piece = piece.trim();
            if piece.is_empty() {
                return None;
            }
            let (left, right) = match piece.split_once("->") {
                Some((l, r)) => (Some(l), r),
                None => (None, piece),
            };
            let (container_port, proto) = right.split_once('/').unwrap_or((right, "tcp"));
            let (host_ip, host_port) = match left {
                None => (String::new(), String::new()),
                Some(l) => match l.rsplit_once(':') {
                    Some((ip, port)) => (ip.trim_matches(|c| c == '[' || c == ']').to_string(), port.to_string()),
                    None => (String::new(), l.to_string()),
                },
            };
            Some(PortMapping { host_ip, host_port, container_port: container_port.to_string(), proto: proto.to_string() })
        })
        .collect()
}

fn images_from(v: &Value) -> Vec<Image> {
    let id = text(v, &["ID", "Id"]);
    if id.is_empty() {
        return Vec::new();
    }
    let (size_text, size_bytes) = match v.get("Size") {
        Some(Value::String(s)) => (s.clone(), parse_size(s)),
        Some(Value::Number(n)) => (human_size(n.as_u64().unwrap_or(0)), n.as_u64()),
        _ => (String::new(), None),
    };
    let created = v
        .get("Created")
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .or_else(|| v.get("CreatedAt").and_then(Value::as_str).and_then(parse_time));
    let containers = v.get("Containers").and_then(|c| c.as_u64().or_else(|| c.as_str().and_then(|s| s.parse().ok()))).map(|n| n as u32);
    let make = |repository: String, tag: String| Image {
        id: id.trim_start_matches("sha256:").to_string(),
        repository,
        tag,
        size_text: size_text.clone(),
        size_bytes,
        created,
        containers,
    };
    // Docker: one row per repository:tag. Podman: a list of "repo:tag" strings.
    // (An untagged Podman image has `"RepoTags": null`, so the key's presence decides.)
    if let Some(tags_value) = v.get("RepoTags") {
        let tags: &[Value] = tags_value.as_array().map(Vec::as_slice).unwrap_or(&[]);
        let rows: Vec<Image> = tags
            .iter()
            .filter_map(Value::as_str)
            .filter(|t| *t != "<none>:<none>")
            .map(|t| match t.rsplit_once(':') {
                // A colon inside a registry host:port is not a tag separator.
                Some((r, tag)) if !tag.contains('/') => make(r.to_string(), tag.to_string()),
                _ => make(t.to_string(), String::new()),
            })
            .collect();
        return if rows.is_empty() { vec![make("<none>".into(), "<none>".into())] } else { rows };
    }
    vec![make(text(v, &["Repository"]), text(v, &["Tag"]))]
}

// -- sizes and times ----------------------------------------------------------

/// `642MB`, `49.2kB`, `1.5GB`: decimal units, as the Docker CLI prints them.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let factor = match unit.trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * factor).round() as u64)
}

pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..=999 => format!("{bytes}B"),
        1_000..=999_999 => format!("{:.1}kB", b / 1e3),
        1_000_000..=999_999_999 => format!("{:.0}MB", b / 1e6),
        _ => format!("{:.2}GB", b / 1e9),
    }
}

/// `2026-10-05 15:07:51 +0000 UTC` (Docker) or `2025-09-27T10:00:00Z` (ISO).
/// Seconds since the Unix epoch.
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once([' ', 'T'])?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let mut parts = rest.split([' ', 'Z', '+']).next()?.split(':');
    let (hh, mm): (i64, i64) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    // Seconds may carry a fraction.
    let ss: i64 = parts.next().unwrap_or("0").split('.').next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Offset: "+0100", "+01:00", or "-0500", after the time.
    let tail = &rest[rest.find(':')? + 1..];
    let off_secs = tail
        .find(['+', '-'])
        .and_then(|i| {
            let sign = if tail.as_bytes()[i] == b'-' { -1 } else { 1 };
            let digits: String = tail[i + 1..].chars().filter(char::is_ascii_digit).take(4).collect();
            if digits.len() != 4 {
                return None;
            }
            let (h, m): (i64, i64) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
            Some(sign * (h * 3600 + m * 60))
        })
        .unwrap_or(0);
    Some(days_from_civil(y, m, day) * 86_400 + hh * 3600 + mm * 60 + ss - off_secs)
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Which parser flavour a runtime's listing commands use, for the command line.
pub fn json_format_args(rt: Runtime) -> [&'static str; 2] {
    match rt {
        Runtime::Podman => ["--format", "json"],
        Runtime::Docker | Runtime::Nerdctl => ["--format", "{{json .}}"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCKER_PS: &str = include_str!("fixtures/docker_ps.ndjson");
    const DOCKER_PS_SIZES: &str = include_str!("fixtures/docker_ps_sizes.ndjson");
    const DOCKER_IMAGES: &str = include_str!("fixtures/docker_images.ndjson");
    // Hand-written from the Podman 4/5 documentation, not recorded: see the note in the file.
    const PODMAN_PS: &str = include_str!("fixtures/podman_ps.json");
    const PODMAN_IMAGES: &str = include_str!("fixtures/podman_images.json");
    const NERDCTL_PS: &str = include_str!("fixtures/nerdctl_ps.ndjson");

    fn by_name<'a>(list: &'a [Container], n: &str) -> &'a Container {
        list.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("{n} not parsed"))
    }

    #[test]
    fn docker_containers_from_recorded_output() {
        let list = parse_containers(DOCKER_PS).unwrap();
        assert_eq!(list.len(), 5);
        let web = by_name(&list, "c0-web");
        assert_eq!(web.id.len(), 64, "the full ID, not the short one");
        assert_eq!(web.state, State::Running);
        assert!(web.status.starts_with("Up "));
        assert_eq!(web.image, "nginxinc/nginx-unprivileged:1.27-alpine");
        assert_eq!(web.ports, vec![PortMapping { host_ip: "127.0.0.1".into(), host_port: "58080".into(), container_port: "8080".into(), proto: "tcp".into() }]);
        assert_eq!(web.labels.get("app").map(String::as_str), Some("demo"));
        assert_eq!(web.labels.get("tier").map(String::as_str), Some("web"));
        assert!(web.labels.get("maintainer").unwrap().contains("<docker-maint@nginx.com>"));
        assert!(web.command.starts_with("/docker-entrypoint.sh nginx"));
        assert_eq!(web.size, Some("49.2kB (virtual 52.7MB)".to_string()), "Size arrives with the listing here");
        let created = web.created.expect("CreatedAt parses");
        assert!(created > 1_700_000_000);

        assert_eq!(by_name(&list, "c0-paused").state, State::Paused);
        assert_eq!(by_name(&list, "c0-created").state, State::Created);
        let done = by_name(&list, "c0-done");
        assert_eq!(done.state, State::Exited);
        assert!(done.status.starts_with("Exited (0)"));
        assert!(done.ports.is_empty());
        // Exposed but not published.
        let paused = by_name(&list, "c0-paused");
        assert_eq!(paused.ports, vec![PortMapping { host_ip: String::new(), host_port: String::new(), container_port: "8080".into(), proto: "tcp".into() }]);
    }

    #[test]
    fn docker_ipv4_and_ipv6_publishes_both_parse() {
        let list = parse_containers(DOCKER_PS).unwrap();
        let pg = by_name(&list, "c0-pg");
        assert_eq!(pg.ports.len(), 2);
        assert_eq!((pg.ports[0].host_ip.as_str(), pg.ports[0].host_port.as_str()), ("0.0.0.0", "58432"));
        assert_eq!((pg.ports[1].host_ip.as_str(), pg.ports[1].host_port.as_str()), ("::", "58432"), "brackets are stripped");
        assert!(pg.labels.is_empty());
    }

    #[test]
    fn docker_sizes_listing() {
        let list = parse_containers(DOCKER_PS_SIZES).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].size.as_deref(), Some("49.2kB (virtual 52.7MB)"));
    }

    #[test]
    fn docker_images_from_recorded_output() {
        let list = parse_images(DOCKER_IMAGES).unwrap();
        assert_eq!(list.len(), 4);
        let pg = list.iter().find(|i| i.repository == "postgres" && i.tag == "16").unwrap();
        assert_eq!(pg.id, "a3b7f434b2dc");
        assert_eq!(pg.size_text, "642MB");
        assert_eq!(pg.size_bytes, Some(642_000_000));
        assert!(pg.created.is_some());
        assert_eq!(pg.containers, Some(1));
        assert!(list.iter().any(|i| i.repository == "nginxinc/nginx-unprivileged" && i.tag == "1.27-alpine"));
    }

    #[test]
    fn podman_array_shape() {
        let list = parse_containers(PODMAN_PS).unwrap();
        assert_eq!(list.len(), 3);
        let web = by_name(&list, "web");
        assert_eq!(web.id, "5d2f0c1a9b7e4c3d8a6f1e2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d");
        assert_eq!(web.state, State::Running);
        assert_eq!(web.created, Some(1_759_672_800));
        assert_eq!(web.ports, vec![PortMapping { host_ip: String::new(), host_port: "8080".into(), container_port: "80".into(), proto: "tcp".into() }]);
        assert_eq!(web.labels.get("app").map(String::as_str), Some("demo"));
        assert_eq!(web.command, "nginx -g daemon off;");
        assert_eq!(web.pod.as_deref(), Some("mypod"));
        let range = by_name(&list, "ranged");
        assert_eq!(range.ports[0].host_port, "9000-9002");
        assert_eq!(range.ports[0].container_port, "9000-9002");
        assert_eq!(range.state, State::Exited, "podman's \"stopped\" means exited");
        assert_eq!(by_name(&list, "fresh").state, State::Created, "\"configured\" means created");
        assert!(range.pod.is_none());
    }

    #[test]
    fn podman_images_expand_one_row_per_tag() {
        let list = parse_images(PODMAN_IMAGES).unwrap();
        assert_eq!(list.len(), 4);
        let tags: Vec<_> = list.iter().map(|i| format!("{}:{}", i.repository, i.tag)).collect();
        assert!(tags.contains(&"docker.io/library/nginx:latest".to_string()));
        assert!(tags.contains(&"docker.io/library/nginx:1.27".to_string()));
        assert!(tags.contains(&"localhost:5000/team/app:v2".to_string()), "a registry port is not a tag: {tags:?}");
        assert!(tags.contains(&"<none>:<none>".to_string()));
        let nginx = list.iter().find(|i| i.tag == "latest").unwrap();
        assert_eq!(nginx.size_bytes, Some(196_000_000));
        assert_eq!(nginx.size_text, "196MB");
        assert_eq!(nginx.id.len(), 64);
        assert_eq!(nginx.created, Some(1_759_000_000));
    }

    #[test]
    fn nerdctl_lines_read_like_docker() {
        let list = parse_containers(NERDCTL_PS).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(by_name(&list, "web").state, State::Running);
        assert_eq!(by_name(&list, "web").ports[0].container_port, "80");
        assert_eq!(by_name(&list, "job").state, State::Exited);
    }

    #[test]
    fn empty_and_odd_input() {
        assert!(parse_containers("").unwrap().is_empty());
        assert!(parse_containers("  \n\n").unwrap().is_empty());
        assert!(parse_containers("[]").unwrap().is_empty());
        assert!(parse_images("").unwrap().is_empty());
        // An object without an ID is skipped, not fatal.
        assert!(parse_containers("{\"Names\":\"x\"}").unwrap().is_empty());
        // Garbage is an error that says where, not a panic.
        let err = parse_containers("{\"ID\":\"a\"}\nnot json").unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(parse_containers("[1, 2").is_err());
        // A list on a single line, and fields of the wrong type.
        let l = parse_containers("[{\"ID\":\"abc\",\"Names\":5,\"Ports\":7,\"Labels\":[1]}]").unwrap();
        assert_eq!(l[0].id, "abc");
        assert!(l[0].ports.is_empty() && l[0].labels.is_empty());
    }

    #[test]
    fn ports_text_forms() {
        let p = ports_from_text("0.0.0.0:80->80/tcp, :::80->80/tcp, 53/udp, 8000-8002->8000-8002/tcp");
        assert_eq!(p.len(), 4);
        assert_eq!(p[1].host_ip, "::", "an unbracketed :::80 splits at the last colon");
        assert_eq!((p[2].host_port.as_str(), p[2].container_port.as_str(), p[2].proto.as_str()), ("", "53", "udp"));
        assert_eq!((p[3].host_port.as_str(), p[3].container_port.as_str()), ("8000-8002", "8000-8002"));
        assert!(ports_from_text("").is_empty());
    }

    #[test]
    fn label_text_with_commas_in_values() {
        let l = labels_from_text("a=1,desc=one, two, three,b=2");
        assert_eq!(l.get("a").map(String::as_str), Some("1"));
        assert_eq!(l.get("desc").map(String::as_str), Some("one, two, three"));
        assert_eq!(l.get("b").map(String::as_str), Some("2"));
        assert!(labels_from_text("").is_empty());
        assert_eq!(labels_from_text("flag=").get("flag").map(String::as_str), Some(""));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("642MB"), Some(642_000_000));
        assert_eq!(parse_size("49.2kB"), Some(49_200));
        assert_eq!(parse_size("1.5GB"), Some(1_500_000_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("N/A"), None);
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("12"), None);
        assert_eq!(human_size(196_000_000), "196MB");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(2_500_000_000), "2.50GB");
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("1970-01-01 00:00:00 +0000 UTC"), Some(0));
        assert_eq!(parse_time("2026-10-05 15:07:51 +0000 UTC"), Some(1_791_212_871));
        // A non-UTC offset moves the instant.
        assert_eq!(parse_time("2026-10-05 15:07:51 +0200 CEST"), Some(1_791_212_871 - 7200));
        assert_eq!(parse_time("2026-10-05 15:07:51 -0500 EST"), Some(1_791_212_871 + 18_000));
        assert_eq!(parse_time("2025-09-27T10:00:00Z"), Some(1_758_967_200));
        assert_eq!(parse_time("2025-09-27T10:00:00.123456+01:00"), Some(1_758_967_200 - 3600));
        assert_eq!(parse_time("2024-02-29 00:00:00 +0000 UTC"), Some(1_709_164_800), "leap day");
        assert_eq!(parse_time("2 hours ago"), None);
        assert_eq!(parse_time("2026-13-05 15:07:51 +0000 UTC"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn states() {
        for (s, want) in [("running", State::Running), ("Exited", State::Exited), ("stopped", State::Exited), ("paused", State::Paused), ("restarting", State::Restarting), ("created", State::Created), ("dead", State::Dead), ("???", State::Unknown), ("", State::Unknown)] {
            assert_eq!(state_from(s), want, "{s}");
        }
    }
}
