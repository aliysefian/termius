//! Two-factor codes end to end: a small SSH server of our own that asks for a verification code the way
//! `pam_google_authenticator` does, and the real client logging in to it. No `sshd` or PAM module is needed.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::PublicKey;
use russh::server::{self, Auth, Handler};
use russh::{MethodKind, MethodSet};
use tokio::net::TcpListener;

use crate::models::AuthMethod;
use crate::ssh::{open_client, testutil_target, HostKeyPolicy, SshError, Target};
use crate::totp::Totp;

const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const PASSWORD: &str = "hunter2";

#[derive(Clone)]
enum First {
    /// `AuthenticationMethods password,keyboard-interactive`
    Password,
    /// `AuthenticationMethods publickey,keyboard-interactive`
    Key,
    /// Only keyboard-interactive, asking for the password in the same round as the code (a PAM stack).
    PamBoth,
}

/// (prompt, what the client answered), in the order the server saw them.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

struct Fake {
    first: First,
    seen: Seen,
}

fn kbd() -> Option<MethodSet> {
    Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..]))
}

impl Fake {
    fn prompts(&self) -> Vec<(&'static str, bool)> {
        match self.first {
            First::PamBoth => vec![("Password: ", false), ("Verification code: ", false)],
            _ => vec![("Verification code: ", false)],
        }
    }
}

impl Handler for Fake {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        let first_step_done = matches!(self.first, First::Password) && password == PASSWORD;
        Ok(Auth::Reject { proceed_with_methods: kbd(), partial_success: first_step_done })
    }

    async fn auth_publickey(&mut self, _user: &str, _key: &PublicKey) -> Result<Auth, Self::Error> {
        Ok(Auth::Reject { proceed_with_methods: kbd(), partial_success: matches!(self.first, First::Key) })
    }

    async fn auth_keyboard_interactive<'a>(&'a mut self, _user: &str, _submethods: &str, response: Option<server::Response<'a>>) -> Result<Auth, Self::Error> {
        let prompts = self.prompts();
        let Some(response) = response else {
            let list: Vec<(Cow<'static, str>, bool)> = prompts.iter().map(|(p, echo)| (Cow::Borrowed(*p), *echo)).collect();
            return Ok(Auth::Partial { name: "".into(), instructions: "".into(), prompts: Cow::Owned(list) });
        };
        let answers: Vec<String> = response.map(|b| String::from_utf8_lossy(&b).into_owned()).collect();
        let mut seen = self.seen.lock().unwrap();
        for (i, a) in answers.iter().enumerate() {
            seen.push((prompts.get(i).map(|p| p.0).unwrap_or("?").to_string(), a.clone()));
        }
        let code_at = prompts.iter().position(|p| p.0.starts_with("Verification"));
        let totp = Totp::parse(SECRET).unwrap();
        let now_step = totp.step_at(crate::totp::unix_now());
        // The server accepts the neighbouring periods too, as pam_google_authenticator does, so a test that runs across
        // a boundary does not flake.
        let code_ok = code_at.and_then(|i| answers.get(i)).is_some_and(|a| (now_step.saturating_sub(1)..=now_step + 1).any(|s| totp.code_for_step(s) == *a));
        let password_ok = !matches!(self.first, First::PamBoth) || answers.first().is_some_and(|a| a == PASSWORD);
        if code_ok && password_ok {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }
}

/// Start the server; returns its port and a record of what the client answered.
async fn serve(first: First) -> (u16, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let host_key = russh::keys::decode_secret_key(&crate::keys::generate_ed25519("host").unwrap().private_key, None).unwrap();
    let config = Arc::new(server::Config {
        keys: vec![host_key],
        methods: MethodSet::from(&[MethodKind::Password, MethodKind::PublicKey, MethodKind::KeyboardInteractive][..]),
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let record = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { return };
            let (config, handler) = (config.clone(), Fake { first: first.clone(), seen: record.clone() });
            tokio::spawn(async move {
                if let Ok(session) = server::run_stream(config, socket, handler).await {
                    let _ = session.await;
                }
            });
        }
    });
    (port, seen)
}

fn target(port: u16, dir: &std::path::Path, user: &str, auth: AuthMethod, totp: Option<Totp>) -> Target {
    Target {
        hostname: "127.0.0.1".into(),
        port,
        username: user.into(),
        auth,
        host_keys: HostKeyPolicy::tofu_file(dir.join("known_hosts")),
        totp,
        ..testutil_target()
    }
}

fn password() -> AuthMethod {
    AuthMethod::Password { password: PASSWORD.into() }
}

fn a_key() -> AuthMethod {
    AuthMethod::PrivateKey { private_key: crate::keys::generate_ed25519("client").unwrap().private_key, passphrase: None, certificate: None }
}

fn secret() -> Option<Totp> {
    Some(Totp::parse(SECRET).unwrap())
}

async fn login(t: &Target) -> Result<(), SshError> {
    open_client(t, None).await.map(|_| ())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn password_then_code_logs_in_when_a_secret_is_saved() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::Password).await;
    login(&target(port, dir.path(), "pw-code", password(), secret())).await.expect("password, then the code");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].0, "Verification code: ");
    assert_eq!(seen[0].1.len(), 6);
    assert_ne!(seen[0].1, PASSWORD, "the password never goes to a code prompt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_secret_the_code_prompt_gets_nothing_and_the_login_fails() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::Password).await;
    let err = login(&target(port, dir.path(), "no-secret", password(), None)).await.expect_err("a code is needed");
    assert!(matches!(err, SshError::AuthFailed { .. }), "{err}");
    let seen = seen.lock().unwrap();
    assert!(seen.iter().all(|(_, answer)| answer.is_empty()), "nothing is sent to a code prompt without a secret: {seen:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_then_a_code_logs_in() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::Key).await;
    login(&target(port, dir.path(), "key-code", a_key(), secret())).await.expect("key, then the code");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_that_needs_a_code_fails_as_before_without_a_secret() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::Key).await;
    let err = login(&target(port, dir.path(), "key-nosecret", a_key(), None)).await.expect_err("a code is needed");
    assert!(matches!(err, SshError::AuthFailed { method: "private key", .. }), "{err}");
    assert!(seen.lock().unwrap().is_empty(), "no keyboard-interactive exchange is started without a secret");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pam_stack_asking_for_both_in_one_round_gets_each_its_own_answer() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::PamBoth).await;
    login(&target(port, dir.path(), "pam-both", password(), secret())).await.expect("password and code");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0], ("Password: ".to_string(), PASSWORD.to_string()));
    assert_eq!(seen[1].0, "Verification code: ");
    assert_eq!(seen[1].1.len(), 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_login_never_answers_a_password_question() {
    // The key is not accepted here, and the server offers keyboard-interactive with a password question. A login that
    // has a key and a secret, but no password, must drop that exchange rather than send a wrong password.
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::PamBoth).await;
    let err = login(&target(port, dir.path(), "key-password-question", a_key(), secret())).await.expect_err("no password to give");
    assert!(matches!(err, SshError::AuthFailed { .. }), "{err}");
    assert!(seen.lock().unwrap().is_empty(), "nothing was answered");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_secret_fails_cleanly() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, seen) = serve(First::Password).await;
    let wrong = Some(Totp::parse("MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43UOV3HO6DZPI").unwrap());
    let err = login(&target(port, dir.path(), "wrong-secret", password(), wrong)).await.expect_err("the code is wrong");
    assert!(matches!(err, SshError::AuthFailed { .. }), "{err}");
    assert_eq!(seen.lock().unwrap().len(), 1, "one attempt, not a retry loop");
}
