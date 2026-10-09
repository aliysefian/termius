//! A vault-backed SSH agent: serves the Key Manager's keys to `ssh`, `git`,
//! `scp` and IDEs on this computer while the vault is unlocked, so private
//! keys never need to exist as files.
//!
//! It speaks the SSH agent protocol (draft-miller-ssh-agent) over a Unix
//! socket or, on Windows, a named pipe. It is deliberately read-only:
//! listing identities and signing are the only requests it answers. Adding,
//! removing and locking keys are refused, since keys come from the vault
//! and nowhere else.
//!
//! Which keys are offered, and whether each signature needs a click, is up
//! to a [`Backend`]; the app's backend reads the unlocked vault and asks
//! the user through the UI.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use russh::keys::signature::Signer;
use russh::keys::ssh_encoding::Encode;
use russh::keys::ssh_key::private::KeypairData;
use russh::keys::ssh_key::{HashAlg, PrivateKey, PublicKey};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// Message numbers (draft-miller-ssh-agent §6.1).
const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const RSA_SHA2_256: u32 = 2;
const RSA_SHA2_512: u32 = 4;

/// Refuse anything bigger: requests are a few KB at most.
const MAX_MESSAGE: usize = 256 * 1024;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A key the agent may offer.
#[derive(Clone)]
pub struct Offered {
    pub public: PublicKey,
    pub comment: String,
}

/// Where keys come from and who approves their use.
pub trait Backend: Send + Sync + 'static {
    /// Keys to list. Only these can be used to sign.
    fn identities(&self) -> Vec<Offered>;
    /// The private key for `public`, if it's still offered. Called only
    /// after [`Backend::approve`] returned true.
    fn private(&self, public: &PublicKey) -> Option<PrivateKey>;
    /// Whether this signature may go ahead (e.g. after asking the user).
    /// `origin` names the remote host when the request came through agent
    /// forwarding; `None` means a program on this computer.
    fn approve(&self, public: &PublicKey, comment: &str, origin: Option<&str>) -> BoxFuture<bool>;
}

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

fn take_string<'a>(buf: &mut &'a [u8]) -> Option<&'a [u8]> {
    if buf.len() < 4 {
        return None;
    }
    let n = u32::from_be_bytes(buf[..4].try_into().ok()?) as usize;
    if buf.len() < 4 + n {
        return None;
    }
    let s = &buf[4..4 + n];
    *buf = &buf[4 + n..];
    Some(s)
}

fn take_u32(buf: &mut &[u8]) -> Option<u32> {
    if buf.len() < 4 {
        return None;
    }
    let v = u32::from_be_bytes(buf[..4].try_into().ok()?);
    *buf = &buf[4..];
    Some(v)
}

/// Sign `data` the way the agent protocol expects: the SSH signature blob.
/// RSA honours the SHA-2 flags (never SHA-1 unless nothing else is asked).
pub fn sign(key: &PrivateKey, data: &[u8], flags: u32) -> Option<Vec<u8>> {
    let sig = match key.key_data() {
        KeypairData::Rsa(rsa) => {
            let hash = if flags & RSA_SHA2_512 != 0 {
                Some(HashAlg::Sha512)
            } else if flags & RSA_SHA2_256 != 0 {
                Some(HashAlg::Sha256)
            } else {
                None
            };
            Signer::try_sign(&(rsa, hash), data).ok()?
        }
        other => Signer::try_sign(other, data).ok()?,
    };
    sig.encode_vec().ok()
}

/// Answer one request. Returns the response body (without its length).
pub async fn handle(backend: &dyn Backend, request: &[u8], origin: Option<&str>) -> Vec<u8> {
    let failure = vec![FAILURE];
    let Some((&kind, mut rest)) = request.split_first() else {
        return failure;
    };
    match kind {
        REQUEST_IDENTITIES => {
            let ids = backend.identities();
            let mut out = vec![IDENTITIES_ANSWER];
            let mut body = Vec::new();
            let mut n = 0u32;
            for id in ids {
                let Ok(blob) = id.public.to_bytes() else { continue };
                put_string(&mut body, &blob);
                put_string(&mut body, id.comment.as_bytes());
                n += 1;
            }
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&body);
            out
        }
        SIGN_REQUEST => {
            let (Some(blob), Some(data)) = (take_string(&mut rest), take_string(&mut rest)) else {
                return failure;
            };
            let flags = take_u32(&mut rest).unwrap_or(0);
            let Ok(public) = PublicKey::from_bytes(blob) else {
                return failure;
            };
            // Only keys we offer, matched on the key itself.
            let Some(offered) = backend
                .identities()
                .into_iter()
                .find(|o| o.public.key_data() == public.key_data())
            else {
                return failure;
            };
            if !backend.approve(&offered.public, &offered.comment, origin).await {
                return failure;
            }
            let Some(private) = backend.private(&offered.public) else {
                return failure;
            };
            match sign(&private, data, flags) {
                Some(sig) => {
                    let mut out = vec![SIGN_RESPONSE];
                    put_string(&mut out, &sig);
                    out
                }
                None => failure,
            }
        }
        // Add, remove, lock, smartcard, extensions: read-only agent.
        _ => failure,
    }
}

/// Serve one client connection until it closes.
pub async fn serve_conn<S: AsyncRead + AsyncWrite + Unpin>(stream: S, backend: Arc<dyn Backend>) {
    serve_conn_from(stream, backend, None).await
}

/// Like [`serve_conn`], for a forwarded agent channel from `origin`.
pub async fn serve_conn_from<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    backend: Arc<dyn Backend>,
    origin: Option<String>,
) {
    loop {
        let mut len = [0u8; 4];
        if stream.read_exact(&mut len).await.is_err() {
            return;
        }
        let n = u32::from_be_bytes(len) as usize;
        if n == 0 || n > MAX_MESSAGE {
            return;
        }
        let mut req = vec![0u8; n];
        if stream.read_exact(&mut req).await.is_err() {
            return;
        }
        let resp = handle(backend.as_ref(), &req, origin.as_deref()).await;
        let mut framed = Vec::with_capacity(resp.len() + 4);
        put_string(&mut framed, &resp);
        if stream.write_all(&framed).await.is_err() {
            return;
        }
    }
}

/// A running agent. Dropping it stops listening and removes the socket.
pub struct AgentHandle {
    pub path: String,
    task: tokio::task::JoinHandle<()>,
    #[cfg(unix)]
    socket: std::path::PathBuf,
}

impl Drop for AgentHandle {
    fn drop(&mut self) {
        self.task.abort();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Listen on `dir/agent.sock` (created owner-only). Must run inside a tokio
/// runtime.
#[cfg(unix)]
pub fn start(dir: &std::path::Path, backend: Arc<dyn Backend>) -> std::io::Result<AgentHandle> {
    use std::os::unix::fs::PermissionsExt;
    crate::vault::atomic::create_private_dir(dir)?;
    let socket = dir.join("agent.sock");
    // A socket left by a crash would make bind fail.
    if std::fs::symlink_metadata(&socket).is_ok() {
        std::fs::remove_file(&socket)?;
    }
    let listener = tokio::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    // spawn-ok: started from the async unlock and agent_set_enabled commands
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let backend = Arc::clone(&backend);
            tokio::spawn(serve_conn(stream, backend));
        }
    });
    Ok(AgentHandle {
        path: socket.to_string_lossy().into_owned(),
        task,
        socket,
    })
}

/// Listen on a per-user named pipe. Windows' OpenSSH uses it when
/// `SSH_AUTH_SOCK` points at it.
#[cfg(windows)]
pub fn start(_dir: &std::path::Path, backend: Arc<dyn Backend>) -> std::io::Result<AgentHandle> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
    let name = format!(r"\\.\pipe\sshvault-agent-{user}");
    // Create the first instance now so errors surface to the caller.
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    let pipe = name.clone();
    // spawn-ok: started from the async unlock and agent_set_enabled commands
    let task = tokio::spawn(async move {
        loop {
            if server.connect().await.is_err() {
                return;
            }
            let connected = server;
            server = match ServerOptions::new().reject_remote_clients(true).create(&pipe) {
                Ok(s) => s,
                Err(_) => return,
            };
            let backend = Arc::clone(&backend);
            tokio::spawn(serve_conn(connected, backend));
        }
    });
    Ok(AgentHandle { path: name, task })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{generate, KeyAlgorithm};
    use russh::keys::signature::Verifier;
    use russh::keys::ssh_key::Signature;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixed {
        keys: Vec<PrivateKey>,
        allow: bool,
        asked: AtomicUsize,
    }

    impl Backend for Fixed {
        fn identities(&self) -> Vec<Offered> {
            self.keys
                .iter()
                .map(|k| Offered { public: k.public_key().clone(), comment: format!("vault:{}", k.algorithm()) })
                .collect()
        }
        fn private(&self, public: &PublicKey) -> Option<PrivateKey> {
            self.keys.iter().find(|k| k.public_key().key_data() == public.key_data()).cloned()
        }
        fn approve(&self, _: &PublicKey, _: &str, _: Option<&str>) -> BoxFuture<bool> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let ok = self.allow;
            Box::pin(async move { ok })
        }
    }

    fn key(alg: KeyAlgorithm) -> PrivateKey {
        let m = generate(alg, "t", None).unwrap();
        PrivateKey::from_openssh(m.private_key.unwrap().as_str()).unwrap()
    }

    fn sign_req(public: &PublicKey, data: &[u8], flags: u32) -> Vec<u8> {
        let mut r = vec![SIGN_REQUEST];
        put_string(&mut r, &public.to_bytes().unwrap());
        put_string(&mut r, data);
        r.extend_from_slice(&flags.to_be_bytes());
        r
    }

    #[tokio::test]
    async fn lists_signs_verifies_and_refuses_writes() {
        let ed = key(KeyAlgorithm::Ed25519);
        let ec = key(KeyAlgorithm::EcdsaP256);
        let b = Fixed { keys: vec![ed.clone(), ec.clone()], allow: true, asked: AtomicUsize::new(0) };

        let resp = handle(&b, &[REQUEST_IDENTITIES], None).await;
        assert_eq!(resp[0], IDENTITIES_ANSWER);
        assert_eq!(u32::from_be_bytes(resp[1..5].try_into().unwrap()), 2);

        for k in [&ed, &ec] {
            let resp = handle(&b, &sign_req(k.public_key(), b"challenge", 0), None).await;
            assert_eq!(resp[0], SIGN_RESPONSE, "{}", k.algorithm());
            let mut rest = &resp[1..];
            let blob = take_string(&mut rest).unwrap();
            let sig = Signature::try_from(blob).unwrap();
            Verifier::verify(k.public_key(), b"challenge", &sig).expect("valid signature");
        }
        assert_eq!(b.asked.load(Ordering::SeqCst), 2);

        // A key it doesn't offer, and every write request, fail.
        let other = key(KeyAlgorithm::Ed25519);
        assert_eq!(handle(&b, &sign_req(other.public_key(), b"x", 0), None).await, [FAILURE]);
        for kind in [17u8, 18, 19, 22, 23, 25, 27] {
            assert_eq!(handle(&b, &[kind], None).await, [FAILURE]);
        }
        assert_eq!(handle(&b, &[], None).await, [FAILURE]);
        assert_eq!(handle(&b, &[SIGN_REQUEST, 0, 0], None).await, [FAILURE]);
    }

    #[tokio::test]
    async fn refused_approval_means_no_signature() {
        let ed = key(KeyAlgorithm::Ed25519);
        let b = Fixed { keys: vec![ed.clone()], allow: false, asked: AtomicUsize::new(0) };
        assert_eq!(handle(&b, &sign_req(ed.public_key(), b"x", 0), None).await, [FAILURE]);
    }

    #[tokio::test]
    async fn rsa_uses_the_requested_sha2() {
        let rsa = key(KeyAlgorithm::Rsa3072);
        let b = Fixed { keys: vec![rsa.clone()], allow: true, asked: AtomicUsize::new(0) };
        let resp = handle(&b, &sign_req(rsa.public_key(), b"data", RSA_SHA2_512), None).await;
        let mut rest = &resp[1..];
        let sig = Signature::try_from(take_string(&mut rest).unwrap()).unwrap();
        assert_eq!(sig.algorithm().to_string(), "rsa-sha2-512");
        Verifier::verify(rsa.public_key(), b"data", &sig).unwrap();
    }

    /// Real `ssh-add -L` and `ssh` using the agent against a throw-away sshd.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn openssh_clients_use_the_agent() {
        use crate::ssh::testutil::spawn_sshd;
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let client = PrivateKey::from_openssh(&sshd.client_key).unwrap();
        let b = Arc::new(Fixed { keys: vec![client.clone()], allow: true, asked: AtomicUsize::new(0) });
        let handle = start(&dir.path().join("agentdir"), b.clone()).unwrap();

        let path = handle.path.clone();
        let listed = tokio::task::spawn_blocking(move || {
            std::process::Command::new("ssh-add").arg("-L").env("SSH_AUTH_SOCK", &path).output()
        })
        .await
        .unwrap()
        .unwrap();
        let out = String::from_utf8_lossy(&listed.stdout);
        assert!(out.contains(&client.public_key().to_openssh().unwrap().split(' ').nth(1).unwrap().to_string()), "{out}");

        // ssh-add can't smuggle keys in.
        let path = handle.path.clone();
        let other = dir.path().join("other_key");
        let added = tokio::task::spawn_blocking(move || {
            std::process::Command::new("ssh-add").arg(&other).env("SSH_AUTH_SOCK", &path).output()
        })
        .await
        .unwrap()
        .unwrap();
        assert!(!added.status.success());

        let (path, port, user) = (handle.path.clone(), sshd.port, sshd.user.clone());
        let ran = tokio::task::spawn_blocking(move || {
            std::process::Command::new("ssh")
                .args(["-F", "/dev/null", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "IdentitiesOnly=no", "-o", "PubkeyAuthentication=yes", "-p"])
                .arg(port.to_string())
                .arg(format!("{user}@127.0.0.1"))
                .arg("echo agent-ok")
                .env("SSH_AUTH_SOCK", &path)
                .env_remove("SSH_ASKPASS")
                .output()
        })
        .await
        .unwrap()
        .unwrap();
        assert!(String::from_utf8_lossy(&ran.stdout).contains("agent-ok"), "{}", String::from_utf8_lossy(&ran.stderr));
        assert!(b.asked.load(Ordering::SeqCst) >= 1);

        let sock = std::path::PathBuf::from(&handle.path);
        drop(handle);
        assert!(!sock.exists(), "socket removed when the agent stops");
    }
}
