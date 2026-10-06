//! Mosh using the system's `mosh-client`.
//!
//! What Mosh itself does: log in over SSH, run `mosh-server`, read the UDP port
//! and one-time key it prints, then run `mosh-client` against that port. The
//! SSH half is done here, with this app's own engine, so vault keys, jump
//! hosts, proxies and the known-hosts rules all apply. The UDP half is the
//! system's `mosh-client`, run in a local terminal with the key in its
//! environment (never on its command line, where other users could read it).
//!
//! Tauri-agnostic: the commands wire it to a pane.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use crate::containers::transport::{remote_command, shell_quote, SshShell};
use crate::ssh::{SshError, Target};

#[derive(Debug, thiserror::Error)]
pub enum MoshError {
    /// The login worked but the host has no `mosh-server`. The window offers plain SSH.
    #[error("mosh-server isn't installed on {0}, or isn't on its PATH. Install the \"mosh\" package there, or connect with plain SSH")]
    ServerMissing(String),
    #[error("mosh-client isn't installed on this computer. Install Mosh (for example `apt install mosh` or `brew install mosh`), or connect with plain SSH")]
    ClientMissing,
    #[error("{0}")]
    ServerFailed(String),
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("couldn't find an address for {0}")]
    Resolve(String),
}

/// What `mosh-client` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub ip: IpAddr,
    pub port: u16,
    pub key: String,
}

/// `MOSH CONNECT <port> <key>` is printed by `mosh-server` among other text.
pub fn parse_connect(output: &str) -> Option<(u16, String)> {
    output.lines().find_map(|l| {
        let mut w = l.trim().strip_prefix("MOSH CONNECT ")?.split_whitespace();
        let port: u16 = w.next()?.parse().ok().filter(|p| *p != 0)?;
        let key = w.next()?;
        // 22 characters of base64; reject anything that couldn't be put in an environment variable safely.
        let ok = !key.is_empty() && key.len() <= 64 && key.chars().all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c));
        ok.then(|| (port, key.to_string()))
    })
}

/// The server's words for a locale it can't use: the usual first-run trouble.
fn explain_failure(stderr: &str) -> String {
    let text = stderr.trim();
    if text.contains("UTF-8") {
        format!("mosh-server needs a UTF-8 locale on the host, and it has none set up: {text}")
    } else if text.is_empty() {
        "mosh-server ran but didn't say where to connect".into()
    } else {
        format!("mosh-server failed: {text}")
    }
}

/// The remote command: bind to the address this SSH connection arrived on, 256
/// colours, and the same UTF-8 locale as here when there is one.
pub fn server_command(lang: Option<&str>) -> String {
    server_command_for("mosh-server", lang)
}

fn server_command_for(binary: &str, lang: Option<&str>) -> String {
    let mut args: Vec<String> = ["new", "-s", "-c", "256"].map(String::from).into();
    if let Some(l) = lang.filter(|l| is_utf8_locale(l)) {
        args.extend(["-l".to_string(), format!("LANG={l}")]);
    }
    remote_command(binary, &args)
}

pub fn is_utf8_locale(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    (n.contains("utf-8") || n.contains("utf8")) && name.chars().all(|c| c.is_ascii_alphanumeric() || "_.-@".contains(c))
}

/// A UTF-8 locale for `mosh-client`, which refuses to start without one.
pub fn client_locale(current: Option<&str>) -> String {
    match current {
        Some(l) if is_utf8_locale(l) => l.to_string(),
        _ if cfg!(target_os = "macos") => "en_US.UTF-8".into(),
        _ => "C.UTF-8".into(),
    }
}

/// `mosh-client` (or `mosh-client.exe`) on PATH.
pub fn find_client() -> Option<PathBuf> {
    let name = if cfg!(windows) { "mosh-client.exe" } else { "mosh-client" };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// Whether a failed `mosh-server` run means "not there" (the shell's 127, or its own message).
fn is_missing(code: Option<i32>, stderr: &str) -> bool {
    code == Some(127) || stderr.contains("not found") || stderr.contains("No such file")
}

async fn address_of(hostname: &str, port: u16) -> Result<IpAddr, MoshError> {
    if let Ok(ip) = hostname.parse::<IpAddr>() {
        return Ok(ip);
    }
    let found: Vec<SocketAddr> = tokio::net::lookup_host((hostname, port)).await.map_err(|_| MoshError::Resolve(hostname.into()))?.collect();
    // IPv4 first: it is what `mosh-server -s` most often binds.
    found.iter().find(|a| a.is_ipv4()).or(found.first()).map(SocketAddr::ip).ok_or_else(|| MoshError::Resolve(hostname.into()))
}

/// Log in, start `mosh-server`, and learn where and how to connect. The SSH
/// connection is closed afterwards: the session lives on UDP from here.
pub async fn start_server(target: Target, lang: Option<&str>) -> Result<Launch, MoshError> {
    start_server_with(target, lang, "mosh-server").await
}

/// [`start_server`] with the program named (the tests point it at a specific build).
pub async fn start_server_with(target: Target, lang: Option<&str>, binary: &str) -> Result<Launch, MoshError> {
    let (host, port) = (target.hostname.clone(), target.port);
    let shell = SshShell::connect(target).await.map_err(|e| match e {
        crate::containers::ContainerError::Ssh(s) => MoshError::Ssh(s),
        other => MoshError::ServerFailed(other.to_string()),
    })?;
    let out = shell.exec(&server_command_for(binary, lang), Duration::from_secs(20)).await;
    shell.close().await;
    let out = out.map_err(|e| MoshError::ServerFailed(e.to_string()))?;
    let Some((udp_port, key)) = parse_connect(&out.stdout) else {
        return Err(if is_missing(out.code, &out.stderr) { MoshError::ServerMissing(host) } else { MoshError::ServerFailed(explain_failure(&out.stderr)) });
    };
    Ok(Launch { ip: address_of(&host, port).await?, port: udp_port, key })
}

/// The program, arguments and environment to run `mosh-client`.
pub fn client_command(client: &std::path::Path, launch: &Launch, current_lang: Option<&str>) -> (Vec<String>, Vec<(String, String)>) {
    let argv = vec![client.to_string_lossy().into_owned(), launch.ip.to_string(), launch.port.to_string()];
    let locale = client_locale(current_lang);
    let env = vec![("MOSH_KEY".to_string(), launch.key.clone()), ("LC_ALL".to_string(), locale)];
    (argv, env)
}

/// For messages: the command line without the secret.
pub fn describe(launch: &Launch) -> String {
    format!("mosh-client {} {}", shell_quote(&launch.ip.to_string()), launch.port)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "\n\nMOSH CONNECT 60001 Ab3dEf6hIj9lMn2pQr5tUw\n\nmosh-server (mosh 1.4.0) [build mosh 1.4.0]\nCopyright 2012 Keith Winstein <mosh-devel@mit.edu>\n\n[mosh-server detached, pid = 4242]\n";

    #[test]
    fn reads_the_port_and_key_among_other_output() {
        assert_eq!(parse_connect(REAL), Some((60001, "Ab3dEf6hIj9lMn2pQr5tUw".into())));
        assert_eq!(parse_connect("Welcome!\r\nMOSH CONNECT 60010 k+/=abc\r\n"), Some((60010, "k+/=abc".into())));
    }

    #[test]
    fn refuses_anything_that_isnt_a_connect_line() {
        for bad in ["", "MOSH CONNECT", "MOSH CONNECT 0 key", "MOSH CONNECT 99999 key", "MOSH CONNECT x key", "MOSH CONNECT 60001", "mosh connect 60001 key", "MOSH CONNECT 60001 ke;y", "MOSH CONNECT 60001 $(id)"] {
            assert_eq!(parse_connect(bad), None, "{bad:?}");
        }
        assert_eq!(parse_connect(&format!("MOSH CONNECT 60001 {}", "a".repeat(65))), None);
    }

    #[test]
    fn the_server_command_is_quoted_and_carries_only_a_clean_locale() {
        let plain = server_command(None);
        assert!(plain.starts_with("sh -c "), "{plain}");
        assert!(plain.contains("exec mosh-server new -s -c 256"), "{plain}");
        assert!(!plain.contains("LANG"));
        let with = server_command(Some("en_US.UTF-8"));
        assert!(with.contains("-l LANG=en_US.UTF-8"), "{with}");
        // A locale that isn't UTF-8, or has shell characters in it, is not sent.
        for bad in ["C", "POSIX", "en_US.UTF-8; rm -rf ~", "$(id).UTF-8", "x y.UTF-8"] {
            assert!(!server_command(Some(bad)).contains("LANG"), "{bad}");
        }
    }

    #[test]
    fn locales() {
        assert!(is_utf8_locale("en_US.UTF-8") && is_utf8_locale("C.utf8") && !is_utf8_locale("C") && !is_utf8_locale(""));
        assert_eq!(client_locale(Some("de_DE.UTF-8")), "de_DE.UTF-8");
        assert!(client_locale(Some("C")).to_ascii_lowercase().contains("utf-8"));
        assert!(client_locale(None).to_ascii_lowercase().contains("utf-8"));
    }

    #[test]
    fn the_key_goes_in_the_environment_never_on_the_command_line() {
        let launch = Launch { ip: "192.0.2.7".parse().unwrap(), port: 60001, key: "SECRETKEY".into() };
        let (argv, env) = client_command(std::path::Path::new("/usr/bin/mosh-client"), &launch, Some("en_US.UTF-8"));
        assert_eq!(argv, ["/usr/bin/mosh-client", "192.0.2.7", "60001"]);
        assert!(!argv.join(" ").contains("SECRETKEY"));
        assert!(env.contains(&("MOSH_KEY".to_string(), "SECRETKEY".to_string())));
        assert!(!describe(&launch).contains("SECRETKEY"));
    }

    #[test]
    fn a_missing_server_is_told_apart_from_a_failing_one() {
        assert!(is_missing(Some(127), ""));
        assert!(is_missing(Some(1), "sh: 1: exec: mosh-server: not found"));
        assert!(!is_missing(Some(1), "mosh-server needs a UTF-8 native locale to run."));
        assert!(explain_failure("mosh-server needs a UTF-8 native locale to run.\nThe locale is C").contains("UTF-8 locale on the host"));
        assert!(explain_failure("").contains("didn't say where"));
    }

    #[tokio::test]
    async fn addresses_resolve_without_the_network_for_literals() {
        assert_eq!(address_of("127.0.0.1", 22).await.unwrap(), "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(address_of("::1", 22).await.unwrap(), "::1".parse::<IpAddr>().unwrap());
        assert!(matches!(address_of("no-such-host.invalid", 22).await, Err(MoshError::Resolve(_))));
    }

    // -- against a real sshd and the real Mosh programs ----------------------------------------
    //
    // Needs `SSHVAULT_MOSH_DIR`: a folder holding `mosh-server` and `mosh-client` (and any
    // libraries they need on LD_LIBRARY_PATH). Skipped without it.

    #[cfg(unix)]
    mod live {
        use super::*;
        use crate::localpty::LocalManager;
        use crate::ssh::{SessionStatus, TermSink};
        use crate::ssh::testutil::{spawn_sshd, target};
        use std::sync::{mpsc, Arc};

        enum Ev {
            Data(Vec<u8>),
            Status(SessionStatus),
        }
        struct Sink(std::sync::Mutex<mpsc::Sender<Ev>>);
        impl TermSink for Sink {
            fn data(&self, b: &[u8]) {
                let _ = self.0.lock().unwrap().send(Ev::Data(b.to_vec()));
            }
            fn status(&self, s: SessionStatus) {
                let _ = self.0.lock().unwrap().send(Ev::Status(s));
            }
        }

        fn mosh_dir() -> Option<PathBuf> {
            let d = PathBuf::from(std::env::var_os("SSHVAULT_MOSH_DIR")?);
            (d.join("mosh-server").is_file() && d.join("mosh-client").is_file()).then_some(d)
        }

        /// A `mosh-server` the remote shell can run by absolute path, with the libraries it needs.
        fn wrapper(dir: &std::path::Path, mosh: &std::path::Path) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let w = dir.join("mosh-server");
            let libs = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
            std::fs::write(&w, format!("#!/bin/sh\nLD_LIBRARY_PATH={}\nexport LD_LIBRARY_PATH\nexec {} \"$@\"\n", shell_quote(&libs), shell_quote(&mosh.join("mosh-server").to_string_lossy()))).unwrap();
            std::fs::set_permissions(&w, std::fs::Permissions::from_mode(0o755)).unwrap();
            w
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_real_session_over_a_real_sshd_runs_commands_and_ends() {
            let Some(mosh) = mosh_dir() else {
                eprintln!("skipping: SSHVAULT_MOSH_DIR isn't set");
                return;
            };
            let dir = tempfile::TempDir::new().unwrap();
            let Some(sshd) = spawn_sshd(dir.path()) else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let server = wrapper(dir.path(), &mosh);
            let launch = start_server_with(target(&sshd, &sshd.client_key, dir.path().join("kh")), Some("C.UTF-8"), &server.to_string_lossy()).await.unwrap();
            assert_eq!(launch.ip.to_string(), "127.0.0.1");
            assert!((60000..=61000).contains(&launch.port), "{}", launch.port);

            // The real client, with the key in its environment, in a local terminal.
            let (argv, env) = client_command(&mosh.join("mosh-client"), &launch, Some("C.UTF-8"));
            let (tx, rx) = mpsc::channel();
            let local = Arc::new(LocalManager::new());
            local.spawn_env("p".into(), 100, 30, Some(argv), None, env, Arc::new(Sink(std::sync::Mutex::new(tx)))).unwrap();
            let wait_for = |needle: &str| {
                let mut seen = String::new();
                let deadline = std::time::Instant::now() + Duration::from_secs(20);
                while std::time::Instant::now() < deadline {
                    if let Ok(Ev::Data(d)) = rx.recv_timeout(Duration::from_millis(200)) {
                        seen.push_str(&String::from_utf8_lossy(&d));
                        if seen.contains(needle) {
                            return seen;
                        }
                    }
                }
                panic!("never saw {needle:?}; got {seen:?}");
            };
            // Typing travels over UDP to a real shell on the "host" and the answer comes back.
            std::thread::sleep(Duration::from_millis(800));
            local.write("p", b"echo moshworks$((20+22))\r").unwrap();
            wait_for("moshworks42");
            // Leaving the shell ends the session and the client exits.
            local.write("p", b"exit\r").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            loop {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(Ev::Status(SessionStatus::Disconnected { .. })) => break,
                    _ if std::time::Instant::now() > deadline => panic!("the client never ended"),
                    _ => {}
                }
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_host_without_mosh_server_says_so() {
            let dir = tempfile::TempDir::new().unwrap();
            let Some(sshd) = spawn_sshd(dir.path()) else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let err = start_server_with(target(&sshd, &sshd.client_key, dir.path().join("kh")), None, "mosh-server-surely-not-installed").await.unwrap_err();
            assert!(matches!(err, MoshError::ServerMissing(_)), "{err}");
            assert!(err.to_string().contains("plain SSH"));
        }
    }
}
