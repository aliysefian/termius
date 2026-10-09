//! Which ProxyCommands this computer has been told it may run.
//!
//! A ProxyCommand runs a program on this computer, so the person has to approve it, and the approval has to be about
//! this computer: the proxy record itself is synced, so a flag stored in it would arrive already set from any device
//! (or from anything that can write the vault folder). Approvals are therefore kept in this computer's own config,
//! keyed by a hash of the command text, so editing the command withdraws the approval.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

/// The approved fingerprints. Separate from the process-wide copy so it can be tested alone.
#[derive(Default)]
struct Approvals(BTreeSet<String>);

impl Approvals {
    fn is_approved(&self, command: &str) -> bool {
        self.0.contains(&fingerprint(command))
    }

    fn set(&mut self, command: &str, allow: bool) -> Vec<String> {
        if allow {
            self.0.insert(fingerprint(command));
        } else {
            self.0.remove(&fingerprint(command));
        }
        self.0.iter().cloned().collect()
    }
}

fn shared() -> &'static Mutex<Approvals> {
    static SET: OnceLock<Mutex<Approvals>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

/// What identifies a command: the hash of its trimmed text.
pub fn fingerprint(command: &str) -> String {
    Sha256::digest(command.trim().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Replace the whole set, from the saved config.
pub fn load(fingerprints: impl IntoIterator<Item = String>) {
    shared().lock().unwrap_or_else(|p| p.into_inner()).0 = fingerprints.into_iter().collect();
}

pub fn is_approved(command: &str) -> bool {
    shared().lock().unwrap_or_else(|p| p.into_inner()).is_approved(command)
}

/// Approve or withdraw one command. Returns the full set, to save.
pub fn set(command: &str, allow: bool) -> Vec<String> {
    shared().lock().unwrap_or_else(|p| p.into_inner()).set(command, allow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_is_per_command_text() {
        let mut a = Approvals::default();
        let c = "nc %h %p";
        assert!(!a.is_approved(c));
        let saved = a.set(c, true);
        assert!(a.is_approved(c) && a.is_approved(&format!("  {c}\n")), "surrounding space doesn't matter");
        assert!(!a.is_approved("nc %h 22"), "an edited command is not approved");
        assert_eq!(saved, vec![fingerprint(c)]);
        a.set(c, false);
        assert!(!a.is_approved(c));
    }

    #[test]
    fn fingerprints_are_stable_hex() {
        assert_eq!(fingerprint("a"), "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb");
        assert_eq!(fingerprint(" a "), fingerprint("a"));
    }

    #[test]
    fn the_shared_set_follows_set_and_load() {
        // Only this test touches the shared set; the commands are unique to it.
        let (a, b) = ("shared-test-a", "shared-test-b");
        assert!(!is_approved(a));
        set(a, true);
        assert!(is_approved(a));
        load([fingerprint(b)]);
        assert!(is_approved(b) && !is_approved(a), "load replaces what was there");
        set(b, false);
        assert!(!is_approved(b));
    }
}
