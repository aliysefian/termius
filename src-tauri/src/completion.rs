//! Smart completion's questions to the host: what is in this folder, and the
//! names a few read-only commands list (git branches, containers, units).
//!
//! The question goes over **an extra exec channel of the pane's own
//! connection**, never the shell channel, so nothing is typed into the person's
//! terminal and the shell's output is never mixed up with the answer. If the
//! host refuses another channel (`MaxSessions 1`, a forced command, exec turned
//! off) that is remembered for the session and not tried again; it never
//! affects the shell.
//!
//! Nothing the person typed is ever part of a command: the commands here are
//! fixed text, and the folder and prefix reach them as positional arguments
//! (`"$1"`, `"$2"`), single-quoted for the login shell. A file called `$(rm -rf ~)`
//! is a name to list, not something to run.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use russh::ChannelMsg;
use serde::{Deserialize, Serialize};

use crate::containers::transport::shell_quote;
use crate::ssh::Client;

/// The longest one question may take.
pub const TIMEOUT: Duration = Duration::from_secs(3);
/// Entries returned at most.
pub const MAX_ENTRIES: usize = 500;
/// Bytes read from the host at most; the rest is dropped and the answer says it was cut.
pub const MAX_BYTES: usize = 256 * 1024;
/// A name longer than this is not offered.
const MAX_NAME: usize = 1024;
/// A folder or prefix longer than this is refused.
const MAX_ARG: usize = 4096;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// The entries of `dir` (absolute, or starting with `~`) whose names start with `prefix`.
    Dir { dir: String, prefix: String, limit: Option<usize> },
    /// The names a fixed, read-only command lists. `dir` is where to run it, for those that depend on it.
    Generator { id: String, dir: Option<String> },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Reply {
    pub entries: Vec<Entry>,
    /// There were more than `entries` holds, or the output was cut at its size limit.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    /// The host won't open a channel for this (or the connection is gone). Not tried again this session.
    #[error("the host does not allow lookups on this connection")]
    Refused,
    #[error("the lookup took longer than {} s", TIMEOUT.as_secs())]
    Timeout,
    #[error("{0}")]
    Invalid(String),
}

/// What is remembered per session.
#[derive(Default)]
pub struct Lookups {
    refused: AtomicBool,
    /// Channels this session has tried to open; for tests, and to show that "refused" stops the attempts.
    attempts: AtomicUsize,
    /// One question at a time: typing fast must not stack channels up.
    gate: tokio::sync::Mutex<()>,
}

/// POSIX `sh`, one line (a login shell like csh can't carry a newline inside quotes). `$1` is the folder,
/// `$2` the prefix, `$3` how many entries to stop at. Names come out NUL-separated, folders with a
/// trailing `/`; the prefix is compared as text, never as a pattern.
const LIST_DIR: &str = concat!(
    r#"case "$1" in "~") d=$HOME;; "~/"*) d=$HOME/${1#"~/"};; *) d=$1;; esac; "#,
    r#"cd -- "$d" 2>/dev/null || exit 3; n=0; "#,
    r#"for f in .[!.]* ..?* *; do "#,
    r#"case "$f" in "$2"*) ;; *) continue;; esac; "#,
    r#"[ -e "$f" ] || [ -L "$f" ] || continue; "#,
    r#"if [ -d "$f" ]; then printf '%s/\000' "$f"; else printf '%s\000' "$f"; fi; "#,
    r#"n=$((n+1)); [ "$n" -ge "$3" ] && break; "#,
    r#"done"#,
);

/// The read-only commands that may be run, by id. Each ends in `head -n "$2"`, so it cannot flood the channel.
fn generator(id: &str) -> Option<&'static str> {
    Some(match id {
        "git-branches" => r#"cd -- "$1" 2>/dev/null && git for-each-ref --format='%(refname:short)' refs/heads refs/remotes 2>/dev/null | head -n "$2""#,
        "git-tags" => r#"cd -- "$1" 2>/dev/null && git for-each-ref --format='%(refname:short)' refs/tags 2>/dev/null | head -n "$2""#,
        "docker-containers" => r#"docker ps -a --format '{{.Names}}' 2>/dev/null | head -n "$2""#,
        "docker-images" => r#"docker images --format '{{.Repository}}:{{.Tag}}' 2>/dev/null | grep -v '<none>' | head -n "$2""#,
        "systemd-units" => r#"systemctl list-units --all --plain --no-legend --no-pager 2>/dev/null | cut -d' ' -f1 | head -n "$2""#,
        "kubectl-contexts" => r#"kubectl --request-timeout=2s config get-contexts -o name 2>/dev/null | head -n "$2""#,
        "kubectl-namespaces" => r#"kubectl --request-timeout=2s get namespaces -o name 2>/dev/null | sed 's|^namespace/||' | head -n "$2""#,
        "kubectl-pods" => r#"kubectl --request-timeout=2s get pods -o name 2>/dev/null | sed 's|^pod/||' | head -n "$2""#,
        // The commands typed on this host before, newest last. Only ever asked for with the person's consent
        // ("Learn from the host's own history"), and every line goes through the same filter as typed commands.
        "shell-history" => concat!(
            r#"{ for f in "$HOME/.bash_history" "$HOME/.zsh_history"; do [ -r "$f" ] && tail -n "$2" "$f"; done 2>/dev/null | sed -e 's/^: [0-9]*:[0-9]*;//'; "#,
            r#"[ -r "$HOME/.local/share/fish/fish_history" ] && grep '^- cmd: ' "$HOME/.local/share/fish/fish_history" 2>/dev/null | tail -n "$2" | sed 's/^- cmd: //'; } | head -n "$2""#,
        ),
        _ => return None,
    })
}

/// The ids `generator` knows, for the frontend's tests and the docs.
pub const GENERATORS: &[&str] = &[
    "git-branches",
    "git-tags",
    "docker-containers",
    "docker-images",
    "systemd-units",
    "kubectl-contexts",
    "kubectl-namespaces",
    "kubectl-pods",
    "shell-history",
];

fn command(script: &str, args: &[&str]) -> String {
    // Install folders a non-interactive SSH command lacks (Homebrew, Snap), as for the Containers view.
    let full = format!("PATH=\"$PATH:/usr/local/bin:/opt/homebrew/bin:/snap/bin\"; export PATH; {script}");
    let words: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
    format!("sh -c {} sh {}", shell_quote(&full), words.join(" "))
}

fn check_arg(what: &str, s: &str) -> Result<(), LookupError> {
    if s.len() > MAX_ARG || s.contains('\0') {
        return Err(LookupError::Invalid(format!("{what} is not usable")));
    }
    Ok(())
}

/// The command to run and how many entries to keep, or why the request is refused.
fn build(req: &Request) -> Result<(String, usize, bool), LookupError> {
    match req {
        Request::Dir { dir, prefix, limit } => {
            check_arg("the folder", dir)?;
            check_arg("the prefix", prefix)?;
            if !(dir == "~" || dir.starts_with("~/") || dir.starts_with('/')) {
                return Err(LookupError::Invalid("the folder must be absolute or start with ~".into()));
            }
            let keep = limit.unwrap_or(MAX_ENTRIES).clamp(1, MAX_ENTRIES);
            // One more than is kept, to learn whether there were more.
            let stop = (keep + 1).to_string();
            Ok((command(LIST_DIR, &[dir, prefix, &stop]), keep, true))
        }
        Request::Generator { id, dir } => {
            let script = generator(id).ok_or_else(|| LookupError::Invalid(format!("unknown lookup: {id}")))?;
            let dir = dir.as_deref().unwrap_or("");
            check_arg("the folder", dir)?;
            let stop = (MAX_ENTRIES + 1).to_string();
            Ok((command(script, &[dir, &stop]), MAX_ENTRIES, false))
        }
    }
}

/// The same answer as a lookup over SSH, for a tab running a shell on this computer: the entries of `dir`
/// (absolute, or starting with `~`) whose names start with `prefix`. A folder that can't be read is an empty
/// answer, as over SSH. Only listings, never a command.
pub fn list_local(dir: &str, prefix: &str, limit: Option<usize>) -> Result<Reply, LookupError> {
    check_arg("the folder", dir)?;
    check_arg("the prefix", prefix)?;
    let path = if dir == "~" || dir.starts_with("~/") {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or_else(|| LookupError::Invalid("no home folder".into()))?;
        std::path::PathBuf::from(home).join(dir.trim_start_matches('~').trim_start_matches('/'))
    } else if dir.starts_with('/') {
        std::path::PathBuf::from(dir)
    } else {
        return Err(LookupError::Invalid("the folder must be absolute or start with ~".into()));
    };
    let keep = limit.unwrap_or(MAX_ENTRIES).clamp(1, MAX_ENTRIES);
    let Ok(read) = std::fs::read_dir(&path) else {
        return Ok(Reply { entries: Vec::new(), truncated: false });
    };
    let mut entries = Vec::new();
    let mut truncated = false;
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with(prefix) || name.is_empty() || name.len() > MAX_NAME || name.contains('\n') {
            continue;
        }
        if entries.len() >= keep {
            truncated = true;
            break;
        }
        // A link to a folder counts as a folder, as `[ -d ]` does over SSH.
        entries.push(Entry { name, dir: e.path().is_dir() });
    }
    Ok(Reply { entries, truncated })
}

fn parse(out: &[u8], capped: bool, keep: usize, nul_separated: bool) -> Reply {
    let sep = if nul_separated { 0u8 } else { b'\n' };
    let mut entries = Vec::new();
    let mut more = false;
    for raw in out.split(|b| *b == sep) {
        if raw.is_empty() {
            continue;
        }
        let name = String::from_utf8_lossy(raw);
        let (name, dir) = match nul_separated.then(|| name.strip_suffix('/')).flatten() {
            Some(n) => (n.to_string(), true),
            None => (name.trim_end_matches('\r').to_string(), false),
        };
        // A name with a newline or a slash in it could only mislead; a huge one is no use to anyone.
        if name.is_empty() || name.len() > MAX_NAME || name.contains('\n') || (nul_separated && name.contains('/')) {
            continue;
        }
        if entries.len() == keep {
            more = true;
            break;
        }
        entries.push(Entry { name, dir });
    }
    Reply { entries, truncated: more || capped }
}

impl Lookups {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the host refused, so nothing more will be tried.
    pub fn refused(&self) -> bool {
        self.refused.load(Ordering::Relaxed)
    }

    pub fn attempts(&self) -> usize {
        self.attempts.load(Ordering::Relaxed)
    }

    /// Ask the host, over a new channel of `client`.
    pub async fn lookup(&self, client: &Client, req: &Request) -> Result<Reply, LookupError> {
        let (cmd, keep, nul) = build(req)?;
        if self.refused() {
            return Err(LookupError::Refused);
        }
        let _one_at_a_time = self.gate.lock().await;
        if self.refused() {
            return Err(LookupError::Refused);
        }
        self.attempts.fetch_add(1, Ordering::Relaxed);
        match tokio::time::timeout(TIMEOUT, run(client, &cmd)).await {
            Err(_) => Err(LookupError::Timeout),
            Ok(Err(Refusal)) => {
                self.refused.store(true, Ordering::Relaxed);
                Err(LookupError::Refused)
            }
            Ok(Ok((out, capped))) => Ok(parse(&out, capped, keep, nul)),
        }
    }
}

struct Refusal;

/// Run `command` on a new channel and collect what it writes, up to `MAX_BYTES`.
async fn run(client: &Client, command: &str) -> Result<(Vec<u8>, bool), Refusal> {
    let mut channel = client.channel_open_session().await.map_err(|_| Refusal)?;
    channel.exec(true, command.as_bytes()).await.map_err(|_| Refusal)?;
    let mut out = Vec::new();
    let mut capped = false;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => {
                let room = MAX_BYTES.saturating_sub(out.len());
                out.extend_from_slice(&data[..data.len().min(room)]);
                if data.len() > room {
                    capped = true;
                    let _ = channel.close().await;
                    break;
                }
            }
            // The host said no to running a command at all.
            ChannelMsg::Failure => return Err(Refusal),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok((out, capped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(d: &str, p: &str) -> Request {
        Request::Dir { dir: d.into(), prefix: p.into(), limit: None }
    }

    #[test]
    fn the_folder_and_prefix_are_arguments_never_part_of_the_command() {
        let (cmd, keep, nul) = build(&dir("/tmp/$(touch pwned)", "`id`'; rm -rf ~; '")).unwrap();
        assert!(nul);
        assert_eq!(keep, MAX_ENTRIES);
        // The script is one quoted word; each argument is another, and a quote in one cannot end its quoting.
        assert!(cmd.starts_with("sh -c '"), "{cmd}");
        assert!(cmd.contains(" sh '/tmp/$(touch pwned)' '`id`'\\''; rm -rf ~; '\\''' 501"), "{cmd}");
        assert!(!cmd.contains('\n'), "a csh login shell can't carry a newline inside quotes");
    }

    #[test]
    fn only_absolute_folders_and_tilde_are_taken() {
        assert!(build(&dir("/", "")).is_ok());
        assert!(build(&dir("~", "")).is_ok());
        assert!(build(&dir("~/src", "")).is_ok());
        for bad in ["", "src", "./x", "../x", "~root/x", "-rf"] {
            assert!(matches!(build(&dir(bad, "")), Err(LookupError::Invalid(_))), "{bad}");
        }
        assert!(matches!(build(&dir("/a\0b", "")), Err(LookupError::Invalid(_))));
        assert!(matches!(build(&dir("/a", &"x".repeat(5000))), Err(LookupError::Invalid(_))));
    }

    #[test]
    fn only_the_listed_generators_run_and_each_is_capped() {
        for id in GENERATORS {
            let s = generator(id).unwrap_or_else(|| panic!("{id}"));
            assert!(s.contains(r#"head -n "$2""#), "{id} is not capped");
            assert!(!s.contains('\n'), "{id}");
        }
        for bad in ["", "rm", "git-branches; id", "../git-branches", "GIT-BRANCHES"] {
            let r = Request::Generator { id: bad.into(), dir: None };
            assert!(matches!(build(&r), Err(LookupError::Invalid(_))), "{bad}");
        }
        let r = Request::Generator { id: "git-branches".into(), dir: Some("/srv/$(x)".into()) };
        let (cmd, _, nul) = build(&r).unwrap();
        assert!(!nul);
        assert!(cmd.contains(" sh '/srv/$(x)' 501"), "{cmd}");
    }

    #[test]
    fn the_limit_is_clamped() {
        let at = |limit| build(&Request::Dir { dir: "/".into(), prefix: "".into(), limit }).unwrap();
        assert_eq!(at(Some(0)).1, 1);
        assert_eq!(at(Some(10)).1, 10);
        assert_eq!(at(Some(1_000_000)).1, MAX_ENTRIES);
        assert!(at(Some(10)).0.ends_with(" 11"));
    }

    #[test]
    fn listings_parse_with_folders_marked_and_odd_names_kept_or_dropped_safely() {
        let out = b"a b\0sub/\0it's\0$(x)\0two\nlines\0bad/name\0\0.hidden\0";
        let r = parse(out, false, 100, true);
        let names: Vec<(&str, bool)> = r.entries.iter().map(|e| (e.name.as_str(), e.dir)).collect();
        // "bad/name" would have a slash inside a name (not a folder marker), "two\nlines" a newline: both dropped.
        assert_eq!(names, [("a b", false), ("sub", true), ("it's", false), ("$(x)", false), (".hidden", false)]);
        assert!(!r.truncated);
    }

    #[test]
    fn more_than_asked_for_or_a_cut_off_read_says_truncated() {
        let out = b"a\0b\0c\0d\0";
        let r = parse(out, false, 3, true);
        assert_eq!(r.entries.len(), 3);
        assert!(r.truncated);
        assert!(!parse(out, false, 4, true).truncated);
        assert!(parse(out, true, 10, true).truncated);
    }

    #[test]
    fn generator_output_is_lines() {
        let r = parse(b"main\r\nfeature/x\n\norigin/main\n", false, 10, false);
        assert_eq!(r.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["main", "feature/x", "origin/main"]);
        assert!(r.entries.iter().all(|e| !e.dir));
    }

    // -- over a real sshd ---------------------------------------------------------------

    use crate::ssh::testutil::{spawn_sshd, spawn_sshd_config, target};
    use crate::ssh::open_client;
    use std::path::Path;

    fn names(r: &Reply) -> Vec<String> {
        r.entries.iter().map(|e| format!("{}{}", e.name, if e.dir { "/" } else { "" })).collect()
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lists_a_folder_and_completes_tilde_and_prefixes() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (client, _) = open_client(&target(&sshd, &sshd.client_key, dir.path().join("kh")), None).await.unwrap();
        let lookups = Lookups::new();

        let work = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(work.path().join("src")).unwrap();
        std::fs::create_dir(work.path().join("my docs")).unwrap();
        for f in ["a.txt", "b b.txt", "it's.txt", "say \"hi\".txt", ".hidden", "src/main.rs"] {
            touch(work.path(), f);
        }
        let w = work.path().to_string_lossy().to_string();

        let r = lookups.lookup(&client, &dir_req(&w, "")).await.unwrap();
        let mut got = names(&r);
        got.sort();
        assert_eq!(got, [".hidden", "a.txt", "b b.txt", "it's.txt", "my docs/", "say \"hi\".txt", "src/"]);
        assert!(!r.truncated);

        // Prefixes, including one with a space and one with a quote.
        assert_eq!(names(&lookups.lookup(&client, &dir_req(&w, "b ")).await.unwrap()), ["b b.txt"]);
        assert_eq!(names(&lookups.lookup(&client, &dir_req(&w, "it'")).await.unwrap()), ["it's.txt"]);
        assert_eq!(names(&lookups.lookup(&client, &dir_req(&w, ".")).await.unwrap()), [".hidden"]);
        assert!(lookups.lookup(&client, &dir_req(&w, "zzz")).await.unwrap().entries.is_empty());
        // A pattern character is text, not a pattern.
        assert!(lookups.lookup(&client, &dir_req(&w, "*")).await.unwrap().entries.is_empty());
        assert!(lookups.lookup(&client, &dir_req(&w, "?")).await.unwrap().entries.is_empty());
        assert_eq!(names(&lookups.lookup(&client, &dir_req(&format!("{w}/src"), "")).await.unwrap()), ["main.rs"]);

        // A folder that isn't there is an empty answer, not an error and not a refusal.
        assert!(lookups.lookup(&client, &dir_req("/no/such/folder", "")).await.unwrap().entries.is_empty());
        assert!(!lookups.refused());

        // ~ and ~/ reach the home folder: a folder made in it is found by its prefix.
        if let Some(home) = std::env::var_os("HOME") {
            let tmp = tempfile::Builder::new().prefix("sshv-ac4-").tempdir_in(&home).unwrap();
            touch(tmp.path(), "inside.txt");
            let base = tmp.path().file_name().unwrap().to_string_lossy().to_string();
            let top = lookups.lookup(&client, &dir_req("~", "sshv-ac4-")).await.unwrap();
            assert!(names(&top).contains(&format!("{base}/")), "{:?}", names(&top));
            let inner = lookups.lookup(&client, &dir_req(&format!("~/{base}"), "")).await.unwrap();
            assert_eq!(names(&inner), ["inside.txt"]);
            let slash = lookups.lookup(&client, &dir_req(&format!("~/{base}/"), "ins")).await.unwrap();
            assert_eq!(names(&slash), ["inside.txt"]);
        }
    }

    fn dir_req(dir: &str, prefix: &str) -> Request {
        Request::Dir { dir: dir.into(), prefix: prefix.into(), limit: None }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn names_that_look_like_commands_are_only_names() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (client, _) = open_client(&target(&sshd, &sshd.client_key, dir.path().join("kh")), None).await.unwrap();
        let lookups = Lookups::new();
        let work = tempfile::TempDir::new().unwrap();
        let w = work.path();
        let canary = |n: &str| w.join(n);

        // Folders and files named like attacks; the lookups below must list them and run nothing.
        let hostile = [
            format!("$(touch {})", canary("c1").display()),
            format!("`touch {}`", canary("c2").display()),
            format!("'; touch {}; '", canary("c3").display()),
            format!("\"; touch {}; \"", canary("c4").display()),
            format!("x\ntouch {}\n", canary("c5").display()).replace('\n', "_"),
            "$HOME".to_string(),
            "${IFS}".to_string(),
        ];
        // `/` is not allowed in a name, so the canary paths above are rewritten for the folder names.
        let hostile: Vec<String> = hostile.iter().map(|h| h.replace('/', "%")).collect();
        for h in &hostile {
            std::fs::create_dir(w.join(h)).unwrap();
            touch(&w.join(h), "inner");
        }
        let wd = w.to_string_lossy().to_string();

        let all = lookups.lookup(&client, &dir_req(&wd, "")).await.unwrap();
        for h in &hostile {
            assert!(names(&all).contains(&format!("{h}/")), "{h} missing from {:?}", names(&all));
        }
        // As the folder, as the prefix, and with real canary paths that would work if anything were evaluated.
        for h in &hostile {
            let inner = lookups.lookup(&client, &dir_req(&format!("{wd}/{h}"), "")).await.unwrap();
            assert_eq!(names(&inner), ["inner"], "{h}");
            let pre = lookups.lookup(&client, &dir_req(&wd, h)).await.unwrap();
            assert_eq!(names(&pre), [format!("{h}/")], "{h}");
        }
        let real = [
            format!("$(touch {})", canary("p1").display()),
            format!("`touch {}`", canary("p2").display()),
            format!("'; touch {}; '", canary("p3").display()),
            format!("; touch {} #", canary("p4").display()),
        ];
        for r in &real {
            let _ = lookups.lookup(&client, &dir_req(&format!("{wd}/{r}"), r)).await.unwrap();
            let _ = lookups.lookup(&client, &dir_req(&wd, r)).await.unwrap();
            let _ = lookups.lookup(&client, &Request::Generator { id: "git-branches".into(), dir: Some(r.clone()) }).await.unwrap();
        }
        for n in ["c1", "c2", "c3", "c4", "c5", "p1", "p2", "p3", "p4"] {
            assert!(!canary(n).exists(), "something ran: {n} was created");
        }
        // Nor in the folder the host runs commands from.
        if let Some(home) = std::env::var_os("HOME") {
            for n in ["p1", "p2", "p3", "p4"] {
                assert!(!Path::new(&home).join(n).exists(), "{n} appeared in HOME");
            }
        }
        assert!(!lookups.refused());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_host_that_allows_one_session_fails_quietly_and_is_not_asked_again() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd_config(dir.path(), "MaxSessions 1") else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (client, _) = open_client(&target(&sshd, &sshd.client_key, dir.path().join("kh")), None).await.unwrap();
        // The pane's own shell takes the one session.
        let shell = client.channel_open_session().await.unwrap();
        shell.request_shell(true).await.unwrap();

        let lookups = Lookups::new();
        let r = lookups.lookup(&client, &dir_req("/", "")).await;
        assert!(matches!(r, Err(LookupError::Refused)), "{r:?}");
        assert!(lookups.refused());
        assert_eq!(lookups.attempts(), 1);
        for _ in 0..5 {
            assert!(matches!(lookups.lookup(&client, &dir_req("/", "")).await, Err(LookupError::Refused)));
            assert!(matches!(
                lookups.lookup(&client, &Request::Generator { id: "git-branches".into(), dir: None }).await,
                Err(LookupError::Refused)
            ));
        }
        assert_eq!(lookups.attempts(), 1, "no further channel was asked for");

        // The connection and the shell are untouched.
        assert!(!client.is_closed());
        shell.data(&b"echo still-here\n"[..]).await.unwrap();
        let mut shell = shell;
        let mut seen = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !seen.contains("still-here") {
            if let Ok(Some(ChannelMsg::Data { data })) = tokio::time::timeout(Duration::from_millis(500), shell.wait()).await {
                seen.push_str(&String::from_utf8_lossy(&data));
            }
        }
        assert!(seen.contains("still-here"), "the shell stopped answering: {seen:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fifty_thousand_entries_return_the_first_few_within_the_cap_and_say_so() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (client, _) = open_client(&target(&sshd, &sshd.client_key, dir.path().join("kh")), None).await.unwrap();
        let lookups = Lookups::new();
        let big = tempfile::TempDir::new().unwrap();
        for i in 0..50_000 {
            std::fs::File::create(big.path().join(format!("file-{i:05}.log"))).unwrap();
        }
        let w = big.path().to_string_lossy().to_string();

        let started = std::time::Instant::now();
        let r = lookups.lookup(&client, &dir_req(&w, "")).await.unwrap();
        let took = started.elapsed();
        eprintln!("50,000 entries: {} returned in {:?}", r.entries.len(), took);
        assert_eq!(r.entries.len(), MAX_ENTRIES);
        assert!(r.truncated);
        assert!(took < TIMEOUT, "{took:?}");

        // The prefix is applied on the host before the cut, so a narrow prefix is complete.
        let few = lookups.lookup(&client, &dir_req(&w, "file-4999")).await.unwrap();
        assert_eq!(few.entries.len(), 10);
        assert!(!few.truncated);
        // A smaller limit is kept.
        let ten = lookups.lookup(&client, &Request::Dir { dir: w, prefix: "".into(), limit: Some(10) }).await.unwrap();
        assert_eq!((ten.entries.len(), ten.truncated), (10, true));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn works_through_a_jump_host() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let mut jumped = t.clone();
        jumped.jump = Some(Box::new(t));
        let (client, _) = open_client(&jumped, None).await.unwrap();
        assert_eq!(client.jump_hosts().len(), 1);
        let work = tempfile::TempDir::new().unwrap();
        touch(work.path(), "via-jump.txt");
        let r = Lookups::new().lookup(&client, &dir_req(&work.path().to_string_lossy(), "")).await.unwrap();
        assert_eq!(names(&r), ["via-jump.txt"]);
    }

    #[test]
    fn a_local_listing_matches_the_prefix_marks_folders_and_refuses_odd_folders() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
        std::fs::write(dir.path().join("alps"), "").unwrap();
        std::fs::create_dir(dir.path().join("alpine")).unwrap();
        std::fs::write(dir.path().join("beta"), "").unwrap();
        let d = dir.path().to_string_lossy().to_string();
        let mut r = list_local(&d, "alp", None).unwrap();
        r.entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(r.entries, [Entry { name: "alpha.txt".into(), dir: false }, Entry { name: "alpine".into(), dir: true }, Entry { name: "alps".into(), dir: false }]);
        assert!(!r.truncated);
        let cut = list_local(&d, "", Some(2)).unwrap();
        assert_eq!((cut.entries.len(), cut.truncated), (2, true));
        assert!(list_local(&format!("{d}/missing"), "", None).unwrap().entries.is_empty());
        assert!(matches!(list_local("relative/dir", "", None), Err(LookupError::Invalid(_))));
        assert!(matches!(list_local("/tmp\0x", "", None), Err(LookupError::Invalid(_))));
    }

    #[cfg(unix)]
    #[test]
    fn the_shell_history_lookup_reads_bash_zsh_and_fish_files() {
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(home.path().join(".bash_history"), "ls -la\ncd /srv\n").unwrap();
        std::fs::write(home.path().join(".zsh_history"), ": 1700000000:0;git status\n: 1700000001:0;make test\n").unwrap();
        std::fs::create_dir_all(home.path().join(".local/share/fish")).unwrap();
        std::fs::write(home.path().join(".local/share/fish/fish_history"), "- cmd: echo hi\n  when: 1700000002\n- cmd: uptime\n  when: 1700000003\n").unwrap();
        let (cmd, _, _) = build(&Request::Generator { id: "shell-history".into(), dir: None }).unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(&cmd).env("HOME", home.path()).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines, ["ls -la", "cd /srv", "git status", "make test", "echo hi", "uptime"], "{text}");
        // No history files: an empty answer, quietly.
        let empty = tempfile::TempDir::new().unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(&cmd).env("HOME", empty.path()).output().unwrap();
        assert!(out.stdout.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn generators_list_names_and_a_missing_tool_is_an_empty_answer() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let (client, _) = open_client(&target(&sshd, &sshd.client_key, dir.path().join("kh")), None).await.unwrap();
        let lookups = Lookups::new();
        let git = |args: &[&str], cwd: &Path| {
            std::process::Command::new("git").args(args).current_dir(cwd).env("GIT_CONFIG_GLOBAL", "/dev/null").status().map(|s| s.success()).unwrap_or(false)
        };
        let repo = tempfile::TempDir::new().unwrap();
        if git(&["init", "-q", "-b", "trunk"], repo.path())
            && git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"], repo.path())
            && git(&["branch", "feature/it's-a-branch"], repo.path())
            && git(&["tag", "v1.0"], repo.path())
        {
            let d = repo.path().to_string_lossy().to_string();
            let b = lookups.lookup(&client, &Request::Generator { id: "git-branches".into(), dir: Some(d.clone()) }).await.unwrap();
            let mut got = names(&b);
            got.sort();
            assert_eq!(got, ["feature/it's-a-branch", "trunk"]);
            let t = lookups.lookup(&client, &Request::Generator { id: "git-tags".into(), dir: Some(d) }).await.unwrap();
            assert_eq!(names(&t), ["v1.0"]);
            // Not a repository: nothing, quietly.
            let none = lookups.lookup(&client, &Request::Generator { id: "git-branches".into(), dir: Some("/".into()) }).await.unwrap();
            assert!(none.entries.is_empty());
        }
        // A tool that isn't installed (or has no daemon or cluster to talk to) is an empty answer, never a
        // refusal. On a machine that has kubectl but no cluster it may also run out of time; that is the
        // lookup's own cap working, and says nothing about the host refusing anything.
        for id in ["docker-containers", "kubectl-pods", "kubectl-contexts"] {
            let r = lookups.lookup(&client, &Request::Generator { id: id.into(), dir: None }).await;
            assert!(matches!(r, Ok(_) | Err(LookupError::Timeout)), "{id}: {r:?}");
        }
        assert!(!lookups.refused());
        // An id outside the list never reaches the host.
        let before = lookups.attempts();
        assert!(matches!(lookups.lookup(&client, &Request::Generator { id: "rm -rf".into(), dir: None }).await, Err(LookupError::Invalid(_))));
        assert_eq!(lookups.attempts(), before);
    }
}
