//! Which shells this computer has for local terminals, and which WSL
//! distributions on Windows. Tauri-agnostic.
//!
//! Detection reads the computer through [`Env`], so the Windows and WSL
//! logic is tested on any machine with a stand-in. Each shell has a stable id
//! (`unix:zsh`, `win:gitbash`, `wsl:Ubuntu`): the window asks for a shell by id
//! and the backend turns it into a command, so the window never supplies a
//! program to run.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

pub fn current_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellKind {
    Native,
    Wsl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShellInfo {
    pub id: String,
    pub label: String,
    /// The program and its arguments. Shown as a tooltip; the id is what is sent back.
    pub argv: Vec<String>,
    pub kind: ShellKind,
    /// What a plain "new local terminal" runs on this computer.
    pub is_default: bool,
}

/// Everything detection needs to know about the computer.
pub trait Env {
    fn os(&self) -> Os;
    fn var(&self, name: &str) -> Option<String>;
    fn read_etc_shells(&self) -> Option<String>;
    fn is_file(&self, path: &str) -> bool;
    /// The full path of a program on PATH.
    fn which(&self, name: &str) -> Option<String>;
    /// The raw output of `wsl.exe -l -q`, if WSL answered successfully.
    fn wsl_list(&self) -> Option<Vec<u8>>;
}

// -- parsing ------------------------------------------------------------------

/// The shells listed in `/etc/shells`: absolute paths, no comments, and none of
/// the entries that exist to stop a login.
pub fn parse_etc_shells(text: &str) -> Vec<String> {
    const NOT_SHELLS: [&str; 9] = ["nologin", "false", "true", "git-shell", "sync", "halt", "shutdown", "reboot", "rbash"];
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| l.starts_with('/'))
        .filter(|l| !NOT_SHELLS.contains(&basename(l)))
        .map(str::to_string)
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `wsl.exe -l -q` prints UTF-16LE (usually without a byte-order mark), or UTF-8
/// when WSL is told to. Returns the text either way.
pub fn decode_wsl_output(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8]| {
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16(&bytes[2..]);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    // UTF-16 of mostly-ASCII text has a zero in every second byte; UTF-8 of any text has none.
    let odd_zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    if bytes.len() >= 2 && odd_zeros * 2 >= bytes.len() / 2 {
        utf16(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The distribution names from `wsl -l -q`. Distributions that exist only to
/// back Docker Desktop can't run a shell and are left out, and so is the
/// sentence WSL prints when nothing is installed.
pub fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in decode_wsl_output(bytes).lines() {
        let line = raw.trim_matches(|c: char| c == '\u{feff}' || c == '\0' || c.is_whitespace());
        // Verbose listings mark the default; the quiet one doesn't, but be forgiving.
        let name = line.strip_suffix("(Default)").unwrap_or(line).trim();
        let sentence = name.contains("Windows Subsystem") || name.contains("wsl --") || name.split_whitespace().count() > 3;
        let ok = !name.is_empty() && name.len() <= 100 && !sentence && !name.chars().any(char::is_control) && !name.starts_with("docker-desktop");
        if ok && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

// -- detection -------------------------------------------------------------------

fn native(id: &str, label: &str, argv: Vec<String>, is_default: bool) -> ShellInfo {
    ShellInfo { id: id.into(), label: label.into(), argv, kind: ShellKind::Native, is_default }
}

fn unix_shells(env: &dyn Env) -> Vec<ShellInfo> {
    let login = env.var("SHELL").filter(|s| s.starts_with('/') && env.is_file(s));
    let mut paths: Vec<String> = match env.read_etc_shells() {
        Some(text) => parse_etc_shells(&text),
        // No /etc/shells: look in the usual places.
        None => ["/bin/bash", "/bin/zsh", "/usr/bin/zsh", "/usr/bin/fish", "/bin/fish", "/bin/sh", "/bin/dash", "/bin/ksh"].map(String::from).into(),
    };
    paths.retain(|p| env.is_file(p));
    if let Some(l) = &login {
        paths.insert(0, l.clone());
    }
    let login_name = login.as_deref().map(basename);
    let mut out: Vec<ShellInfo> = Vec::new();
    for p in paths {
        let name = basename(&p).to_string();
        // /bin/bash and /usr/bin/bash are one shell on most systems.
        if out.iter().any(|s| s.id == format!("unix:{name}")) {
            continue;
        }
        let is_default = login_name == Some(name.as_str());
        out.push(native(&format!("unix:{name}"), &name, vec![p], is_default));
    }
    if let Some(p) = env.which("pwsh") {
        out.push(native("unix:pwsh", "PowerShell (pwsh)", vec![p, "-NoLogo".into()], false));
    }
    // Your login shell, then the usual ones, then the rest by name.
    let rank = |s: &ShellInfo| match (s.is_default, s.label.as_str()) {
        (true, _) => 0,
        (_, "bash") => 1,
        (_, "zsh") => 2,
        (_, "fish") => 3,
        _ => 4,
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.label.cmp(&b.label)));
    out
}

fn windows_shells(env: &dyn Env) -> Vec<ShellInfo> {
    let mut out = Vec::new();
    let system_root = env.var("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    // What a plain terminal runs today: %COMSPEC%, which is cmd.exe.
    let comspec = env.var("COMSPEC").filter(|c| env.is_file(c)).unwrap_or_else(|| format!("{system_root}\\System32\\cmd.exe"));
    out.push(native("win:cmd", "Command Prompt", vec![comspec], true));

    let ps = format!("{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    if env.is_file(&ps) || env.which("powershell.exe").is_some() {
        out.insert(0, native("win:powershell", "Windows PowerShell", vec!["powershell.exe".into(), "-NoLogo".into()], false));
    }
    if let Some(p) = env.which("pwsh.exe") {
        out.insert(0, native("win:pwsh", "PowerShell 7", vec![p, "-NoLogo".into()], false));
    }

    // Git for Windows: its bash is not the one in System32, which is the WSL launcher.
    let git_roots = [env.var("ProgramFiles"), env.var("ProgramFiles(x86)"), env.var("LOCALAPPDATA").map(|l| format!("{l}\\Programs"))];
    if let Some(bash) = git_roots.into_iter().flatten().map(|r| format!("{r}\\Git\\bin\\bash.exe")).find(|p| env.is_file(p)) {
        out.push(native("win:gitbash", "Git Bash", vec![bash, "--login".into(), "-i".into()], false));
    }

    for name in env.wsl_list().map(|b| parse_wsl_list(&b)).unwrap_or_default() {
        // `--cd ~` starts in the distribution's own home directory.
        out.push(ShellInfo {
            id: format!("wsl:{name}"),
            label: format!("WSL: {name}"),
            argv: vec!["wsl.exe".into(), "-d".into(), name, "--cd".into(), "~".into()],
            kind: ShellKind::Wsl,
            is_default: false,
        });
    }
    out
}

/// Every shell this computer can start for a local terminal. WSL distributions
/// appear only on Windows.
pub fn detect(env: &dyn Env) -> Vec<ShellInfo> {
    match env.os() {
        Os::Windows => windows_shells(env),
        Os::MacOs | Os::Linux => unix_shells(env),
    }
}

/// The shell with this id, if it is still here.
pub fn resolve(env: &dyn Env, id: &str) -> Option<ShellInfo> {
    detect(env).into_iter().find(|s| s.id == id)
}

// -- the real computer -----------------------------------------------------------

pub struct RealEnv;

impl Env for RealEnv {
    fn os(&self) -> Os {
        current_os()
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    fn read_etc_shells(&self) -> Option<String> {
        std::fs::read_to_string("/etc/shells").ok()
    }

    fn is_file(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn which(&self, name: &str) -> Option<String> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
    }

    #[cfg(windows)]
    fn wsl_list(&self) -> Option<Vec<u8>> {
        use std::os::windows::process::CommandExt;
        // WSL can take a while to start its service the first time; don't hold the app for it.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("wsl.exe").args(["-l", "-q"]).creation_flags(0x0800_0000).output();
            let _ = tx.send(out.ok().filter(|o| o.status.success()).map(|o| o.stdout));
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).ok().flatten()
    }

    #[cfg(not(windows))]
    fn wsl_list(&self) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct Fake {
        os: Option<Os>,
        vars: HashMap<&'static str, &'static str>,
        etc_shells: Option<&'static str>,
        files: HashSet<&'static str>,
        path: HashMap<&'static str, &'static str>,
        wsl: Option<Vec<u8>>,
    }

    impl Env for Fake {
        fn os(&self) -> Os {
            self.os.unwrap_or(Os::Linux)
        }
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).map(|s| s.to_string())
        }
        fn read_etc_shells(&self) -> Option<String> {
            self.etc_shells.map(str::to_string)
        }
        fn is_file(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn which(&self, name: &str) -> Option<String> {
            self.path.get(name).map(|s| s.to_string())
        }
        fn wsl_list(&self) -> Option<Vec<u8>> {
            self.wsl.clone()
        }
    }

    fn utf16le(s: &str, bom: bool) -> Vec<u8> {
        let mut v = if bom { vec![0xFF, 0xFE] } else { vec![] };
        for u in s.encode_utf16() {
            v.extend(u.to_le_bytes());
        }
        v
    }

    fn ids(list: &[ShellInfo]) -> Vec<&str> {
        list.iter().map(|s| s.id.as_str()).collect()
    }

    const ETC_SHELLS: &str = "# /etc/shells: valid login shells\n/bin/sh\n/bin/bash\n/usr/bin/bash\n/bin/rbash\n/usr/bin/zsh\n/usr/bin/fish\n/bin/dash\n/usr/sbin/nologin\n/usr/bin/false\n\n  /usr/bin/git-shell  # not a shell for people\nrelative/not-absolute\n";

    // -- /etc/shells ---------------------------------------------------------

    #[test]
    fn etc_shells_keeps_only_real_shells() {
        let got = parse_etc_shells(ETC_SHELLS);
        assert_eq!(got, vec!["/bin/sh", "/bin/bash", "/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish", "/bin/dash"]);
        assert!(parse_etc_shells("").is_empty());
        assert!(parse_etc_shells("# only a comment\n").is_empty());
        assert_eq!(parse_etc_shells("/bin/bash # trailing comment"), vec!["/bin/bash"]);
    }

    #[test]
    fn linux_lists_installed_shells_login_shell_first_and_no_duplicates() {
        let env = Fake {
            etc_shells: Some(ETC_SHELLS),
            files: ["/bin/sh", "/bin/bash", "/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish"].into(),
            vars: [("SHELL", "/usr/bin/zsh")].into(),
            ..Default::default()
        };
        let list = detect(&env);
        // dash is listed but not installed; bash appears once; the login shell leads.
        assert_eq!(ids(&list), ["unix:zsh", "unix:bash", "unix:fish", "unix:sh"]);
        assert_eq!(list.iter().filter(|s| s.is_default).map(|s| s.id.as_str()).collect::<Vec<_>>(), ["unix:zsh"]);
        assert_eq!(list[0].argv, vec!["/usr/bin/zsh"]);
        assert!(list.iter().all(|s| s.kind == ShellKind::Native));
    }

    #[test]
    fn the_login_shell_is_listed_even_when_etc_shells_forgot_it() {
        let env = Fake { etc_shells: Some("/bin/sh\n"), files: ["/bin/sh", "/opt/custom/xonsh"].into(), vars: [("SHELL", "/opt/custom/xonsh")].into(), ..Default::default() };
        let list = detect(&env);
        assert_eq!(ids(&list), ["unix:xonsh", "unix:sh"]);
        assert!(list[0].is_default);
        assert_eq!(list[0].argv, vec!["/opt/custom/xonsh"]);
    }

    #[test]
    fn a_bogus_login_shell_is_ignored() {
        let env = Fake { etc_shells: Some("/bin/bash\n"), files: ["/bin/bash"].into(), vars: [("SHELL", "/nowhere/gone"), ("X", "y")].into(), ..Default::default() };
        let list = detect(&env);
        assert_eq!(ids(&list), ["unix:bash"]);
        assert!(!list[0].is_default, "nothing is marked default when $SHELL points nowhere");
        let relative = Fake { etc_shells: Some("/bin/bash\n"), files: ["/bin/bash", "bash"].into(), vars: [("SHELL", "bash")].into(), ..Default::default() };
        assert_eq!(ids(&detect(&relative)), ["unix:bash"]);
    }

    #[test]
    fn without_etc_shells_the_usual_places_are_tried() {
        let env = Fake { etc_shells: None, files: ["/bin/bash", "/bin/sh"].into(), ..Default::default() };
        assert_eq!(ids(&detect(&env)), ["unix:bash", "unix:sh"]);
        assert!(detect(&Fake::default()).is_empty());
    }

    #[test]
    fn macos_reads_the_same_list() {
        let env = Fake { os: Some(Os::MacOs), etc_shells: Some("/bin/bash\n/bin/zsh\n/bin/sh\n"), files: ["/bin/bash", "/bin/zsh", "/bin/sh"].into(), vars: [("SHELL", "/bin/zsh")].into(), ..Default::default() };
        let list = detect(&env);
        assert_eq!(ids(&list), ["unix:zsh", "unix:bash", "unix:sh"]);
    }

    #[test]
    fn powershell_core_is_found_on_unix_through_path() {
        let env = Fake { etc_shells: Some("/bin/bash\n"), files: ["/bin/bash"].into(), path: [("pwsh", "/usr/local/bin/pwsh")].into(), ..Default::default() };
        let list = detect(&env);
        let ps = list.iter().find(|s| s.id == "unix:pwsh").unwrap();
        assert_eq!(ps.argv, vec!["/usr/local/bin/pwsh", "-NoLogo"]);
        assert_eq!(ps.label, "PowerShell (pwsh)");
    }

    // -- Windows -------------------------------------------------------------

    fn windows() -> Fake {
        Fake {
            os: Some(Os::Windows),
            vars: [("SystemRoot", "C:\\Windows"), ("COMSPEC", "C:\\Windows\\System32\\cmd.exe"), ("ProgramFiles", "C:\\Program Files"), ("ProgramFiles(x86)", "C:\\Program Files (x86)"), ("LOCALAPPDATA", "C:\\Users\\a\\AppData\\Local")].into(),
            files: ["C:\\Windows\\System32\\cmd.exe", "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"].into(),
            ..Default::default()
        }
    }

    #[test]
    fn windows_always_has_command_prompt_and_powershell_and_never_unix_entries() {
        let list = detect(&windows());
        assert_eq!(ids(&list), ["win:powershell", "win:cmd"]);
        assert_eq!(list.iter().filter(|s| s.is_default).map(|s| s.id.as_str()).collect::<Vec<_>>(), ["win:cmd"], "what a plain terminal runs today");
        assert_eq!(list[1].argv, vec!["C:\\Windows\\System32\\cmd.exe"]);
        assert!(list.iter().all(|s| !s.id.starts_with("unix:")));
    }

    #[test]
    fn windows_finds_powershell_7_and_git_bash_wherever_git_is_installed() {
        let mut env = windows();
        env.path.insert("pwsh.exe", "C:\\Program Files\\PowerShell\\7\\pwsh.exe");
        env.files.insert("C:\\Program Files (x86)\\Git\\bin\\bash.exe");
        let list = detect(&env);
        assert_eq!(ids(&list), ["win:pwsh", "win:powershell", "win:cmd", "win:gitbash"]);
        let git = list.iter().find(|s| s.id == "win:gitbash").unwrap();
        assert_eq!(git.argv, vec!["C:\\Program Files (x86)\\Git\\bin\\bash.exe", "--login", "-i"]);
        let mut per_user = windows();
        per_user.files.insert("C:\\Users\\a\\AppData\\Local\\Programs\\Git\\bin\\bash.exe");
        assert!(ids(&detect(&per_user)).contains(&"win:gitbash"));
    }

    #[test]
    fn the_wsl_launcher_in_system32_is_not_mistaken_for_git_bash() {
        let mut env = windows();
        env.files.insert("C:\\Windows\\System32\\bash.exe");
        assert!(!ids(&detect(&env)).contains(&"win:gitbash"));
    }

    #[test]
    fn windows_lists_each_wsl_distribution_to_open_in_its_home() {
        let mut env = windows();
        env.wsl = Some(utf16le("Ubuntu-22.04\r\ndocker-desktop\r\ndocker-desktop-data\r\nDebian\r\n", false));
        let list = detect(&env);
        let wsl: Vec<&ShellInfo> = list.iter().filter(|s| s.kind == ShellKind::Wsl).collect();
        assert_eq!(wsl.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["wsl:Ubuntu-22.04", "wsl:Debian"]);
        assert_eq!(wsl[0].label, "WSL: Ubuntu-22.04");
        assert_eq!(wsl[0].argv, vec!["wsl.exe", "-d", "Ubuntu-22.04", "--cd", "~"]);
        assert!(wsl.iter().all(|s| !s.is_default));
    }

    #[test]
    fn windows_without_wsl_has_none() {
        let mut env = windows();
        env.wsl = None;
        assert!(detect(&env).iter().all(|s| s.kind == ShellKind::Native));
        env.wsl = Some(vec![]);
        assert!(detect(&env).iter().all(|s| s.kind == ShellKind::Native));
    }

    // -- the line this task promises ------------------------------------------

    #[test]
    fn linux_and_macos_never_show_wsl_entries_even_if_something_claimed_there_were_some() {
        for os in [Os::Linux, Os::MacOs] {
            let env = Fake { os: Some(os), etc_shells: Some("/bin/bash\n"), files: ["/bin/bash"].into(), wsl: Some(utf16le("Ubuntu\r\n", false)), ..Default::default() };
            let list = detect(&env);
            assert!(list.iter().all(|s| s.kind != ShellKind::Wsl && !s.id.starts_with("wsl:")), "{os:?}: {:?}", ids(&list));
        }
    }

    // -- wsl -l -q -------------------------------------------------------------

    #[test]
    fn wsl_output_in_utf16_with_and_without_a_byte_order_mark() {
        for bom in [false, true] {
            assert_eq!(parse_wsl_list(&utf16le("Ubuntu\r\nDebian\r\n", bom)), ["Ubuntu", "Debian"], "bom {bom}");
        }
    }

    #[test]
    fn wsl_output_in_utf8_when_wsl_is_asked_for_it() {
        assert_eq!(parse_wsl_list(b"Ubuntu\r\nkali-linux\r\n"), ["Ubuntu", "kali-linux"]);
        assert_eq!(parse_wsl_list(b"\xEF\xBB\xBFUbuntu\n"), ["Ubuntu"]);
        assert_eq!(parse_wsl_list(b"Ubuntu"), ["Ubuntu"], "no trailing newline");
    }

    #[test]
    fn wsl_names_with_non_ascii_text_survive_utf16() {
        assert_eq!(parse_wsl_list(&utf16le("Ünïcode-Distro\r\n東京\r\n", false)), ["Ünïcode-Distro", "東京"]);
    }

    #[test]
    fn wsl_output_trailing_noise_is_tolerated() {
        // An odd final byte, blank lines, repeats and a default marker.
        let mut v = utf16le("\r\nUbuntu (Default)\r\n\r\nUbuntu\r\nAlpine\r\n", false);
        v.push(0x0D);
        assert_eq!(parse_wsl_list(&v), ["Ubuntu", "Alpine"]);
        assert!(parse_wsl_list(b"").is_empty());
        assert!(parse_wsl_list(&[0xFF, 0xFE]).is_empty());
    }

    #[test]
    fn docker_desktop_helpers_and_wsls_not_installed_message_are_not_distributions() {
        assert_eq!(parse_wsl_list(&utf16le("docker-desktop\r\ndocker-desktop-data\r\nUbuntu\r\n", false)), ["Ubuntu"]);
        let msg = "Windows Subsystem for Linux has no installed distributions.\r\nDistributions can be installed by visiting the Microsoft Store:\r\nUse 'wsl.exe --install' to install.\r\n";
        assert!(parse_wsl_list(&utf16le(msg, false)).is_empty());
        assert!(parse_wsl_list(&utf16le("one two three four five\r\n", false)).is_empty(), "a sentence is not a name");
    }

    #[test]
    fn wsl_names_cannot_smuggle_control_characters_or_run_long() {
        assert!(parse_wsl_list(b"bad\x07name\n").is_empty());
        assert!(parse_wsl_list(format!("{}\n", "a".repeat(101)).as_bytes()).is_empty());
    }

    // -- ids -------------------------------------------------------------------------

    #[test]
    fn ids_are_unique_and_resolve_back_to_their_shell() {
        let mut env = windows();
        env.path.insert("pwsh.exe", "C:\\pwsh.exe");
        env.files.insert("C:\\Program Files\\Git\\bin\\bash.exe");
        env.wsl = Some(utf16le("Ubuntu\r\nDebian\r\n", false));
        let list = detect(&env);
        let unique: HashSet<&str> = ids(&list).into_iter().collect();
        assert_eq!(unique.len(), list.len());
        for s in &list {
            assert_eq!(resolve(&env, &s.id).as_ref(), Some(s));
        }
        assert_eq!(resolve(&env, "wsl:Gone"), None);
        assert_eq!(resolve(&env, "unix:bash"), None, "a Unix id on Windows is not a shell");
        assert_eq!(resolve(&env, ""), None);
    }

    #[test]
    fn this_computers_own_list_is_sane() {
        let list = detect(&RealEnv);
        if cfg!(windows) {
            assert!(list.iter().any(|s| s.id == "win:cmd"));
        } else {
            assert!(!list.is_empty(), "every Unix has a shell");
            assert!(list.iter().all(|s| s.kind == ShellKind::Native && std::path::Path::new(&s.argv[0]).is_absolute()));
        }
    }
}
