//! A record of why the app stopped.
//!
//! Release builds abort on a panic and are stripped, so without this the app just disappears (the 0.26.3 container-logs
//! crash left nothing to look at). The hook below runs before the abort and appends one entry to `crash.log` in the
//! app's config folder: when, which thread, and the source line. The message is kept only up to the point where a
//! formatted error value would start, because that value could hold anything (a host name, a path, a secret).
//! Nothing is sent anywhere.

use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};

const FILE: &str = "crash.log";
/// The log is cut back to its newest half when it grows past this.
const MAX_BYTES: u64 = 128 * 1024;
const MAX_MESSAGE: usize = 200;

pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The log is a convenience: failing to write it must not hide the panic or panic again.
        let _ = append(&dir, &entry(info, now()));
        previous(info);
    }));
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn entry(info: &PanicHookInfo<'_>, at: u64) -> String {
    let payload = info.payload();
    let message = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_default();
    let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "unknown".into());
    let thread = std::thread::current();
    format_entry(at, thread.name().unwrap_or("unnamed"), &place, &message)
}

fn format_entry(at: u64, thread: &str, place: &str, message: &str) -> String {
    format!("[{at}] v{} panic in thread '{thread}' at {place}: {}\n", env!("CARGO_PKG_VERSION"), scrub(message))
}

/// The message without a formatted value: `called `Result::unwrap()` on an `Err` value: <anything>` keeps only the
/// part before the value. One line, and a fixed length.
pub fn scrub(message: &str) -> String {
    let cut = message.find("` value: ").map(|i| &message[..i + 1]).unwrap_or(message);
    let one_line: String = cut.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).collect();
    let mut out: String = one_line.chars().take(MAX_MESSAGE).collect();
    if one_line.chars().count() > MAX_MESSAGE {
        out.push('…');
    }
    out
}

fn append(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = path(dir);
    if std::fs::metadata(&file).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let old = std::fs::read(&file)?;
        let keep = &old[old.len() / 2..];
        // Start at a line boundary.
        let start = keep.iter().position(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
        std::fs::write(&file, &keep[start..])?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    f.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formatted_error_value_is_cut_off() {
        let m = "called `Result::unwrap()` on an `Err` value: Os { code: 2, message: \"/home/ana/secret-host-key\" }";
        let s = scrub(m);
        assert_eq!(s, "called `Result::unwrap()` on an `Err`");
        assert!(!s.contains("secret"));
        assert_eq!(scrub("there is no reactor running"), "there is no reactor running");
    }

    #[test]
    fn messages_are_one_short_line() {
        assert_eq!(scrub("first\nsecond"), "first");
        assert_eq!(scrub("a\u{7}b"), "ab");
        let long = scrub(&"x".repeat(1000));
        assert_eq!(long.chars().count(), MAX_MESSAGE + 1);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn entries_are_appended_and_the_log_stays_bounded() {
        let dir = std::env::temp_dir().join(format!("crashlog-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        append(&dir, &format_entry(1, "main", "src/a.rs:1", "boom")).unwrap();
        append(&dir, &format_entry(2, "worker", "src/b.rs:2", "bang")).unwrap();
        let text = std::fs::read_to_string(path(&dir)).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("thread 'worker' at src/b.rs:2: bang"));
        for i in 0..4000 {
            append(&dir, &format_entry(i, "t", "src/c.rs:3", "filler filler filler filler")).unwrap();
        }
        let size = std::fs::metadata(path(&dir)).unwrap().len();
        assert!(size <= MAX_BYTES + 200, "{size}");
        let last = std::fs::read_to_string(path(&dir)).unwrap();
        assert!(last.lines().last().unwrap().starts_with("[3999]"), "the newest entry survives");
        assert!(last.lines().all(|l| l.starts_with('[')), "cut at a line boundary");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hook_records_a_panic_and_still_lets_it_propagate() {
        let dir = std::env::temp_dir().join(format!("crashlog-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        install(dir.clone());
        let r = std::thread::Builder::new().name("probe".into()).spawn(|| {
            // black_box: the unwrap is meant to run, and its message to be formatted like a real one.
            let e: Result<(), String> = std::hint::black_box(Err("hunter2".to_string()));
            e.unwrap();
        });
        assert!(r.unwrap().join().is_err(), "the panic still unwinds the thread");
        let text = std::fs::read_to_string(path(&dir)).unwrap();
        assert!(text.contains("thread 'probe'") && text.contains("crashlog.rs"), "{text}");
        assert!(!text.contains("hunter2"), "the error value must not be logged: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
