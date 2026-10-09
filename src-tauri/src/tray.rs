//! The parts of "keep running in the tray" that don't need Tauri: whether this computer can show a tray icon at all,
//! and the handshake between the tray's Exit item and the window (see `commands::tray`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// How long the window has to answer a tray Exit before the app quits without it (a window that is frozen or gone
/// must never make Exit do nothing).
pub const EXIT_ACK_TIMEOUT: Duration = Duration::from_secs(4);

/// The most that closing sessions, tunnels and child programs may take before the app exits anyway.
pub const SHUTDOWN_LIMIT: Duration = Duration::from_secs(5);

/// The libraries the Linux tray loads when it starts, in the order it tries them (libappindicator-sys). It panics
/// when none of them is there, and release builds abort on a panic, so they are looked for first.
#[cfg(target_os = "linux")]
pub const APPINDICATOR_LIBRARIES: [&str; 4] = [
    "libayatana-appindicator3.so.1",
    "libappindicator3.so.1",
    "libayatana-appindicator3.so",
    "libappindicator3.so",
];

/// Can a tray icon be created here? On Linux that needs an AppIndicator library; whether the desktop then shows the
/// icon (GNOME needs the AppIndicator extension) can't be known from here. Windows and macOS always have one.
pub fn system_tray_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        APPINDICATOR_LIBRARIES.iter().any(|name| library_loads(name))
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Whether the dynamic loader finds and loads `name`. The handle is closed again; the tray loads it for itself.
#[cfg(target_os = "linux")]
fn library_loads(name: &str) -> bool {
    let Ok(c) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: dlopen with a valid NUL-terminated name; a null result just means "not found". RTLD_LOCAL keeps its
    // symbols out of the global namespace, and the handle is closed right after.
    unsafe {
        let handle = libc::dlopen(c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            return false;
        }
        libc::dlclose(handle);
    }
    true
}

/// A tray Exit waiting for the window. Exit asks the window first, because it saves the open tabs for next time and,
/// with live sessions, may ask "Close and disconnect?". The window answers at once with [`ExitRequests::acknowledge`];
/// a request still unanswered after [`EXIT_ACK_TIMEOUT`] means the window can't, and the app quits without it.
#[derive(Default)]
pub struct ExitRequests {
    next: AtomicU64,
    pending: Mutex<Option<u64>>,
}

impl ExitRequests {
    /// Start a request; returns its number for [`ExitRequests::unanswered`]. A newer request replaces an older one.
    pub fn begin(&self) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);
        id
    }

    /// The window has the request and takes it from here. Returns whether one was waiting.
    pub fn acknowledge(&self) -> bool {
        self.pending.lock().unwrap_or_else(|p| p.into_inner()).take().is_some()
    }

    /// Is request `id` still waiting for the window? False once acknowledged or replaced by a newer one.
    pub fn unanswered(&self, id: u64) -> bool {
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) == Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unanswered_request_stays_pending() {
        let r = ExitRequests::default();
        let id = r.begin();
        assert!(r.unanswered(id));
        assert!(r.unanswered(id), "checking doesn't consume it");
    }

    #[test]
    fn acknowledging_clears_the_request() {
        let r = ExitRequests::default();
        let id = r.begin();
        assert!(r.acknowledge());
        assert!(!r.unanswered(id));
        assert!(!r.acknowledge(), "nothing left to acknowledge");
    }

    #[test]
    fn a_newer_request_replaces_an_older_one() {
        let r = ExitRequests::default();
        let first = r.begin();
        let second = r.begin();
        assert_ne!(first, second);
        assert!(!r.unanswered(first), "the first watchdog must not fire for the second request");
        assert!(r.unanswered(second));
    }

    #[test]
    fn acknowledging_without_a_request_is_harmless() {
        let r = ExitRequests::default();
        assert!(!r.acknowledge());
        let id = r.begin();
        assert!(r.unanswered(id));
    }

    #[test]
    fn the_shutdown_fits_in_a_reasonable_wait() {
        assert!(EXIT_ACK_TIMEOUT >= Duration::from_secs(2), "the window needs time to answer");
        assert!(SHUTDOWN_LIMIT <= Duration::from_secs(10), "Exit must not hang for long");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn probing_for_the_tray_library_never_panics() {
        // Either answer is fine (CI has the library, a bare container may not); what matters is that a missing
        // library is reported as false instead of aborting the app the way the tray itself would.
        let _ = system_tray_supported();
        assert!(!library_loads("libsshvault-no-such-library.so.0"));
        assert!(!library_loads("bad\0name"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_probe_finds_a_library_the_loader_can_open() {
        // libc is always there, so the probe itself works.
        assert!(library_loads("libc.so.6"));
    }
}
