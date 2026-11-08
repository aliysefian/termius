//! Logins and SSH authentication events, read-only: who is logged in (`who`), recent logins (`last`), failed logins
//! (`lastb`), and the SSH daemon's own accept/fail lines from the journal or the auth log.
//!
//! Each source needs different rights. What could not be read is listed with the reason, never silently left out, and
//! nothing is called suspicious: an event is shown as what the log says it is.

use uuid::Uuid;

use super::model::{AuthEvent, AuthKind, Limit, LoginEntry, LoginReport};
use super::parse::{sections, ParseError, MAX_OUTPUT_BYTES};

pub const SCRIPT: &str = r#"LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
os=$(uname -s 2>/dev/null)
echo "@@OS"; echo "$os"
if [ "$os" = Linux ]; then
  echo "@@NOW"; date +%s 2>/dev/null
  echo "@@TZ"; date +%z 2>/dev/null
  echo "@@WHO"; if have who; then who 2>&1; else echo "missing who"; fi
  echo "@@LAST"; if have last; then last -n 30 -F -i -w 2>&1; else echo "missing last"; fi
  echo "@@LASTB"; if have lastb; then lastb -n 30 -F -i -w 2>&1; else echo "missing lastb"; fi
  echo "@@AUTH"
  PAT='sshd.*(Accepted|Failed|Invalid user)'
  src=
  if have journalctl; then
    o=$(journalctl -q --no-pager -n 600 -o short-iso -u ssh -u sshd --since "24 hours ago" 2>&1)
    case $o in
      *"insufficient permissions"*|*"not seeing messages"*|*"Permission denied"*) echo "denied journal" ;;
      "") ;;
      *) src=journal; echo "source journal"; printf '%s\n' "$o" | grep -E "$PAT" | tail -n 100 ;;
    esac
  fi
  if [ -z "$src" ]; then
    for f in /var/log/auth.log /var/log/secure; do
      if [ -r "$f" ]; then echo "source $f"; tail -n 2000 "$f" | grep -E "$PAT" | tail -n 100; src=$f; break; fi
    done
    [ -n "$src" ] || echo "none"
  fi
fi
echo "@@END"
"#;

const MAX_ENTRIES: usize = 60;
const DAYS: &[&str] = &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: &[&str] = &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm), so a timestamp needs no date library.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `+0330` to seconds east of UTC.
fn tz_secs(z: &str) -> i64 {
    let z = z.trim();
    let sign = if z.starts_with('-') { -1 } else { 1 };
    let d = z.trim_start_matches(['+', '-']);
    if d.len() != 4 || !d.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    sign * (d[..2].parse::<i64>().unwrap_or(0) * 3600 + d[2..].parse::<i64>().unwrap_or(0) * 60)
}

/// Milliseconds since 1970 for a local time on the host (`tz` seconds east of UTC).
fn local_ms(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64, tz: i64) -> Option<u64> {
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 || !(1970..=2200).contains(&y) {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s - tz;
    u64::try_from(secs).ok().map(|s| s * 1000)
}

fn hms(t: &str) -> Option<(i64, i64, i64)> {
    let mut it = t.split(':');
    let (h, m) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    let s = it.next().map_or(Some(0), |s| s.parse().ok())?;
    Some((h, m, s))
}

fn from_field(parts: &[&str]) -> Option<String> {
    let f = parts.join(" ");
    // No address recorded: `last -i` prints 0.0.0.0, and a console login has nothing.
    (!f.is_empty() && f != "0.0.0.0" && f != "::").then(|| f.chars().take(80).collect())
}

/// One line of `last`/`lastb -F -i -w`: `user tty from Dow Mon dd hh:mm:ss yyyy - Dow Mon dd hh:mm:ss yyyy (dur)`.
fn parse_last_line(l: &str, tz: i64) -> Option<LoginEntry> {
    let t: Vec<&str> = l.split_whitespace().collect();
    if t.len() < 7 || ["reboot", "shutdown", "runlevel", "wtmp", "btmp"].contains(&t[0]) {
        return None;
    }
    let day = t.iter().position(|x| DAYS.contains(x))?;
    if day < 2 || t.len() < day + 5 {
        return None;
    }
    let mo = MONTHS.iter().position(|m| *m == t[day + 1]).map(|i| i as i64 + 1)?;
    let (h, mi, s) = hms(t[day + 3])?;
    let started_ms = local_ms(t[day + 4].parse().ok()?, mo, t[day + 2].parse().ok()?, h, mi, s, tz);
    Some(LoginEntry {
        user: t[0].chars().take(40).collect(),
        tty: t[1].chars().take(20).collect(),
        from: from_field(&t[2..day]),
        started_ms,
        started: t[day..day + 5].join(" "),
        ended: t[day + 5..].join(" "),
    })
}

/// One line of `who`: `user tty 2026-11-08 07:01 (host)`, or the older `user tty Nov  8 07:01 (host)`.
fn parse_who_line(l: &str, tz: i64) -> Option<LoginEntry> {
    let t: Vec<&str> = l.split_whitespace().collect();
    if t.len() < 4 {
        return None;
    }
    let from = t.last().filter(|x| x.starts_with('(') && x.ends_with(')')).map(|x| x[1..x.len() - 1].to_string()).filter(|x| !x.is_empty());
    let (started_ms, started) = match t[2].split('-').collect::<Vec<_>>()[..] {
        [y, m, d] => {
            let when = hms(t[3]);
            (when.and_then(|(h, mi, s)| local_ms(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?, h, mi, s, tz)), format!("{} {}", t[2], t[3]))
        }
        _ => (None, t[2..t.len().min(5)].join(" ")),
    };
    Some(LoginEntry { user: t[0].chars().take(40).collect(), tty: t[1].chars().take(20).collect(), from: from.map(|f| f.chars().take(80).collect()), started_ms, started, ended: "still logged in".into() })
}

fn word_after<'a>(msg: &'a str, key: &str) -> Option<&'a str> {
    let at = msg.find(key)? + key.len();
    msg[at..].split_whitespace().next()
}

/// One sshd line from the journal (`2026-11-08T07:01:12+0000 host sshd[12]: msg`) or an auth log (`Nov  8 07:01:12 host
/// sshd[12]: msg`).
fn parse_auth_line(l: &str) -> Option<AuthEvent> {
    let at = l.find("sshd[").or_else(|| l.find("sshd:"))?;
    let after = &l[at..];
    let msg = after.split_once("]: ").or_else(|| after.split_once(": ")).map(|x| x.1)?.trim();
    let head: Vec<&str> = l[..at].split_whitespace().collect();
    let time = if head.first().is_some_and(|t| t.len() > 10 && t.as_bytes()[4] == b'-') { head[0].to_string() } else { head.iter().take(3).copied().collect::<Vec<_>>().join(" ") };
    let time: String = time.chars().take(40).collect();
    let (kind, user, from, method) = if let Some(rest) = msg.strip_prefix("Accepted ") {
        (AuthKind::Accepted, word_after(msg, " for "), word_after(msg, " from "), rest.split_whitespace().next())
    } else if msg.starts_with("Failed ") {
        let inv = msg.contains(" for invalid user ");
        let user = if inv { word_after(msg, " for invalid user ") } else { word_after(msg, " for ") };
        (if inv { AuthKind::InvalidUser } else { AuthKind::Failed }, user, word_after(msg, " from "), msg.split_whitespace().nth(1))
    } else if msg.starts_with("Invalid user ") {
        (AuthKind::InvalidUser, word_after(msg, "Invalid user "), word_after(msg, " from "), None)
    } else {
        return None;
    };
    let clip = |s: Option<&str>| s.map(|s| s.chars().take(60).collect::<String>());
    Some(AuthEvent { kind, user: clip(user), from: clip(from), method: clip(method), time })
}

pub fn parse(host_id: Uuid, taken_at_ms: u64, out: &str) -> Result<Option<LoginReport>, ParseError> {
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
    let tz = get("TZ").first().map_or(0, |z| tz_secs(z));
    let mut limits = Vec::new();

    let who = get("WHO");
    let sessions: Vec<LoginEntry> = who.iter().filter_map(|l| parse_who_line(l, tz)).take(MAX_ENTRIES).collect();
    if who.first().is_some_and(|l| l.starts_with("missing")) {
        limits.push(Limit { what: "Current sessions".into(), reason: "`who` is not installed on this host.".into() });
    }

    let parse_log = |name: &str, what: &str, hint: &str, limits: &mut Vec<Limit>| -> Vec<LoginEntry> {
        let lines = get(name);
        if let Some(first) = lines.iter().find(|l| !l.trim().is_empty()) {
            if first.starts_with("missing") {
                limits.push(Limit { what: what.into(), reason: format!("`{}` is not installed on this host.", name.to_lowercase()) });
                return Vec::new();
            }
            // `last` and `lastb` explain themselves on stderr, which the script keeps for exactly this.
            if first.contains("Permission denied") || first.starts_with("last:") || first.starts_with("lastb:") {
                let why = if first.contains("Permission denied") { hint.to_string() } else { first.trim().chars().take(160).collect() };
                limits.push(Limit { what: what.into(), reason: why });
                return Vec::new();
            }
        }
        lines.iter().filter_map(|l| parse_last_line(l, tz)).take(MAX_ENTRIES).collect()
    };
    let recent = parse_log("LAST", "Recent logins", "Reading the login history needs access to /var/log/wtmp.", &mut limits);
    let failed_logins = parse_log("LASTB", "Failed login attempts", "Failed logins are recorded in /var/log/btmp, which only root (or a group such as adm) can read. Ask an administrator, or use the SSH authentication events below where they are readable.", &mut limits);

    let auth_lines = get("AUTH");
    let mut auth_source = None;
    let mut auth = Vec::new();
    for l in auth_lines {
        if let Some(src) = l.strip_prefix("source ") {
            auth_source = Some(src.trim().to_string());
        } else if let Some(src) = l.strip_prefix("denied ") {
            limits.push(Limit { what: "SSH authentication events".into(), reason: format!("The {} cannot be read by this user. Joining the systemd-journal (or adm) group, or using a readable /var/log/auth.log, would allow it.", src.trim()) });
        } else if l.trim() == "none" {
            limits.push(Limit { what: "SSH authentication events".into(), reason: "No readable journal or auth log was found for the SSH daemon.".into() });
        } else if let Some(e) = parse_auth_line(l) {
            auth.push(e);
        }
    }
    // Newest last in the logs; shown newest first.
    auth.reverse();
    auth.truncate(100);
    // If the journal was denied but a file was readable, the file's events are what is shown, and the limit is dropped.
    if auth_source.is_some() {
        limits.retain(|l| l.what != "SSH authentication events");
    }
    Ok(Some(LoginReport { host_id, taken_at_ms, sessions, recent, failed_logins, auth, auth_source, limits }))
}
