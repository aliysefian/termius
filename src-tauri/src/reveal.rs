//! Showing and copying stored secrets (passwords, private keys, passphrases)
//! requires re-entering the master password, like a password manager.
//!
//! * A correct password opens a short grace window, so revealing several
//!   secrets in a row asks once.
//! * Wrong passwords are counted; after [`MAX_FAILURES`] the gate refuses
//!   every attempt for a growing lockout, without even checking the password.
//! * Locking the vault or pressing "hide" closes the window.
//!
//! The gate is time-injectable so its rules can be unit tested.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::models::AuthMethod;

pub const GRACE: Duration = Duration::from_secs(120);
pub const MAX_FAILURES: u32 = 5;
pub const BASE_LOCKOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    /// No password given and no open grace window: ask the user.
    ReauthRequired,
    WrongPassword {
        attempts_left: u32,
    },
    LockedOut {
        retry_in: Duration,
    },
}

#[derive(Debug, Default)]
pub struct RevealGate {
    open_until: Option<Instant>,
    failures: u32,
    locked_until: Option<Instant>,
    lockouts: u32,
}

impl RevealGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self, now: Instant) -> bool {
        self.open_until.is_some_and(|t| now < t)
    }

    /// Seconds left in the grace window, for the UI.
    pub fn remaining(&self, now: Instant) -> Duration {
        self.open_until
            .map(|t| t.saturating_duration_since(now))
            .unwrap_or_default()
    }

    /// Decide whether a reveal may proceed. `verify` checks the supplied
    /// master password and is only called when one was given and the gate
    /// isn't locked out.
    pub fn check(
        &mut self,
        now: Instant,
        password: Option<&[u8]>,
        verify: impl FnOnce(&[u8]) -> bool,
    ) -> Result<(), GateError> {
        if let Some(until) = self.locked_until {
            if now < until {
                return Err(GateError::LockedOut {
                    retry_in: until - now,
                });
            }
            self.locked_until = None;
        }
        match password {
            None if self.is_open(now) => Ok(()),
            None => Err(GateError::ReauthRequired),
            Some(pw) => {
                if verify(pw) {
                    self.failures = 0;
                    self.lockouts = 0;
                    self.open_until = Some(now + GRACE);
                    Ok(())
                } else {
                    self.failures += 1;
                    self.open_until = None;
                    if self.failures >= MAX_FAILURES {
                        self.failures = 0;
                        self.lockouts += 1;
                        // 30s, 60s, 120s ... capped at 15 minutes.
                        let factor = 1u32 << (self.lockouts - 1).min(5);
                        let wait = (BASE_LOCKOUT * factor).min(Duration::from_secs(900));
                        self.locked_until = Some(now + wait);
                        Err(GateError::LockedOut { retry_in: wait })
                    } else {
                        Err(GateError::WrongPassword {
                            attempts_left: MAX_FAILURES - self.failures,
                        })
                    }
                }
            }
        }
    }

    /// If reveals are locked out, how long until they're allowed again.
    /// Checked before the (slow) password derivation, so a lockout also stops
    /// an attacker from making the app burn CPU on guesses.
    pub fn locked_out(&self, now: Instant) -> Option<Duration> {
        self.locked_until.filter(|t| now < *t).map(|t| t - now)
    }

    /// Close the grace window (vault locked, or the user pressed "hide").
    pub fn close(&mut self) {
        self.open_until = None;
    }
}

/// The secret parts of an identity, as shown to the user.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revealed {
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
}

impl From<&AuthMethod> for Revealed {
    fn from(a: &AuthMethod) -> Self {
        match a {
            AuthMethod::Password { password } => Self {
                password: Some(password.clone()),
                ..Self::default()
            },
            AuthMethod::PrivateKey {
                private_key,
                passphrase,
                ..
            } => Self {
                private_key: Some(private_key.clone()),
                passphrase: passphrase.clone(),
                ..Self::default()
            },
            AuthMethod::KeyFile { passphrase, .. } => Self {
                passphrase: passphrase.clone(),
                ..Self::default()
            },
            // Key Manager keys are revealed from the Key Manager.
            AuthMethod::Key { .. } | AuthMethod::Agent => Self::default(),
        }
    }
}

/// Merge an edited identity's auth with the stored one: secrets left empty in
/// the form keep their stored value when the auth type is unchanged. Returns
/// an error message when a new auth type is missing its secret.
pub fn merge_auth(edited: AuthMethod, stored: Option<&AuthMethod>) -> Result<AuthMethod, String> {
    match (edited, stored) {
        (AuthMethod::Password { password }, Some(AuthMethod::Password { password: old }))
            if password.is_empty() =>
        {
            Ok(AuthMethod::Password {
                password: old.clone(),
            })
        }
        (AuthMethod::Password { password }, _) if password.is_empty() => {
            Err("Enter a password".into())
        }
        (
            AuthMethod::PrivateKey {
                private_key,
                passphrase,
                certificate,
            },
            Some(AuthMethod::PrivateKey {
                private_key: old_key,
                passphrase: old_pass,
                certificate: old_cert,
            }),
        ) if private_key.is_empty() => Ok(AuthMethod::PrivateKey {
            private_key: old_key.clone(),
            passphrase: match passphrase {
                Some(p) if !p.is_empty() => Some(p),
                _ => old_pass.clone(),
            },
            certificate: certificate.or_else(|| old_cert.clone()),
        }),
        (
            AuthMethod::KeyFile { path, passphrase },
            Some(AuthMethod::KeyFile {
                passphrase: old_pass,
                ..
            }),
        ) if passphrase.as_deref().unwrap_or("").is_empty() => Ok(AuthMethod::KeyFile {
            path,
            passphrase: old_pass.clone(),
        }),
        (AuthMethod::KeyFile { path, .. }, _) if path.trim().is_empty() => {
            Err("Choose a key file".into())
        }
        (AuthMethod::PrivateKey { private_key, .. }, _) if private_key.is_empty() => {
            Err("Paste a private key".into())
        }
        (other, _) => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: &[u8] = b"master";
    fn verify(pw: &[u8]) -> bool {
        pw == RIGHT
    }

    #[test]
    fn grace_window_opens_and_expires() {
        let mut g = RevealGate::new();
        let t0 = Instant::now();
        assert_eq!(g.check(t0, None, verify), Err(GateError::ReauthRequired));
        assert_eq!(g.check(t0, Some(RIGHT), verify), Ok(()));
        // Within the window, no password needed.
        assert_eq!(g.check(t0 + Duration::from_secs(60), None, verify), Ok(()));
        assert!(g.remaining(t0 + Duration::from_secs(60)) <= Duration::from_secs(60));
        // After it, asked again.
        assert_eq!(
            g.check(t0 + GRACE + Duration::from_secs(1), None, verify),
            Err(GateError::ReauthRequired)
        );
        // close() ends it early.
        g.check(t0, Some(RIGHT), verify).unwrap();
        g.close();
        assert_eq!(g.check(t0, None, verify), Err(GateError::ReauthRequired));
    }

    #[test]
    fn wrong_passwords_lock_out_with_backoff() {
        let mut g = RevealGate::new();
        let t0 = Instant::now();
        for left in (1..MAX_FAILURES).rev() {
            assert_eq!(
                g.check(t0, Some(b"nope"), verify),
                Err(GateError::WrongPassword {
                    attempts_left: left
                })
            );
        }
        assert_eq!(
            g.check(t0, Some(b"nope"), verify),
            Err(GateError::LockedOut {
                retry_in: BASE_LOCKOUT
            })
        );
        // Even the right password is refused during the lockout, unchecked.
        let mut called = false;
        let r = g.check(t0 + Duration::from_secs(5), Some(RIGHT), |_| {
            called = true;
            true
        });
        assert!(matches!(r, Err(GateError::LockedOut { .. })));
        assert!(!called);
        // After it expires, the right password works and resets counters.
        let t1 = t0 + BASE_LOCKOUT + Duration::from_secs(1);
        assert_eq!(g.check(t1, Some(RIGHT), verify), Ok(()));
        // A second round of failures locks out for longer.
        let mut g = RevealGate::new();
        for _ in 0..MAX_FAILURES {
            let _ = g.check(t0, Some(b"x"), verify);
        }
        let t2 = t0 + BASE_LOCKOUT + Duration::from_secs(1);
        for _ in 0..MAX_FAILURES - 1 {
            let _ = g.check(t2, Some(b"x"), verify);
        }
        assert_eq!(
            g.check(t2, Some(b"x"), verify),
            Err(GateError::LockedOut {
                retry_in: BASE_LOCKOUT * 2
            })
        );
    }

    #[test]
    fn empty_secrets_keep_stored_ones() {
        let pw = AuthMethod::Password {
            password: "old".into(),
        };
        assert_eq!(
            merge_auth(
                AuthMethod::Password {
                    password: "".into()
                },
                Some(&pw)
            ),
            Ok(pw.clone())
        );
        assert_eq!(
            merge_auth(
                AuthMethod::Password {
                    password: "new".into()
                },
                Some(&pw)
            ),
            Ok(AuthMethod::Password {
                password: "new".into()
            })
        );
        assert!(merge_auth(
            AuthMethod::Password {
                password: "".into()
            },
            None
        )
        .is_err());

        let key = AuthMethod::PrivateKey {
            private_key: "K".into(),
            passphrase: Some("P".into()),
            certificate: None,
        };
        assert_eq!(
            merge_auth(
                AuthMethod::PrivateKey {
                    private_key: "".into(),
                    passphrase: None,
                    certificate: None
                },
                Some(&key)
            ),
            Ok(key.clone())
        );
        assert_eq!(
            merge_auth(
                AuthMethod::PrivateKey {
                    private_key: "".into(),
                    passphrase: Some("P2".into()),
                    certificate: None
                },
                Some(&key)
            ),
            Ok(AuthMethod::PrivateKey {
                private_key: "K".into(),
                passphrase: Some("P2".into()),
                certificate: None
            })
        );
        // Switching type needs the new secret.
        assert!(merge_auth(
            AuthMethod::PrivateKey {
                private_key: "".into(),
                passphrase: None,
                certificate: None
            },
            Some(&pw)
        )
        .is_err());
        assert_eq!(
            merge_auth(AuthMethod::Agent, Some(&pw)),
            Ok(AuthMethod::Agent)
        );
    }

    #[test]
    fn revealed_parts() {
        let r = Revealed::from(&AuthMethod::PrivateKey {
            private_key: "K".into(),
            passphrase: None,
            certificate: None,
        });
        assert_eq!(r.private_key.as_deref(), Some("K"));
        assert_eq!(r.password, None);
        assert_eq!(Revealed::from(&AuthMethod::Agent), Revealed::default());
    }
}
