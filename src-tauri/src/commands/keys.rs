//! Key Manager.

use super::*;

fn key_err(e: crate::keys::KeyError) -> ApiError {
    ApiError::new("key", e.to_string())
}

pub(super) fn redacted_key(mut r: Record<crate::models::SshKey>) -> Record<crate::models::SshKey> {
    r.data = r.data.as_ref().map(crate::models::SshKey::redacted);
    r
}

pub(super) fn load_key(state: &AppState, id: Uuid) -> ApiResult<Record<crate::models::SshKey>> {
    let r = state
        .session
        .with_vault(|v| v.get::<crate::models::SshKey>(Collection::Keys, id))?;
    if r.data.is_none() {
        return Err(ApiError::new("not_found", "that key was deleted"));
    }
    Ok(r)
}

/// Keys with their private halves and passphrases redacted.
#[tauri::command]
pub fn list_keys(state: State<'_, AppState>) -> ApiResult<Vec<Record<crate::models::SshKey>>> {
    Ok(list_records(&state, Collection::Keys)?
        .into_iter()
        .map(redacted_key)
        .collect())
}

/// Generate a key in the vault. `save_passphrase` keeps the passphrase in
/// the vault too, so connecting doesn't ask for it.
#[tauri::command]
pub async fn generate_ssh_key(
    state: State<'_, AppState>,
    name: String,
    algorithm: crate::keys::KeyAlgorithm,
    comment: String,
    passphrase: Option<String>,
    save_passphrase: bool,
) -> ApiResult<Record<crate::models::SshKey>> {
    let passphrase = passphrase.map(Zeroizing::new);
    let pp = passphrase.clone();
    // RSA generation takes seconds; keep it off the async executor.
    let m = tauri::async_runtime::spawn_blocking(move || {
        crate::keys::generate(algorithm, &comment, pp.as_deref().map(|p| p.as_str()))
    })
    .await
    .map_err(|e| ApiError::new("key", e.to_string()))?
    .map_err(key_err)?;
    let saved = if save_passphrase { passphrase.as_deref().map(|p| p.as_str()) } else { None };
    let key = crate::keymanager::new_key(&name, m, saved);
    save_record(&state, Collection::Keys, None, None, key).map(redacted_key)
}

/// Import a private key from text (OpenSSH, PEM, PKCS#8 or PuTTY). The key
/// is checked before it's stored, including its passphrase if one is given.
#[tauri::command]
pub fn import_private_key(
    state: State<'_, AppState>,
    name: String,
    private_key: String,
    passphrase: Option<String>,
    save_passphrase: bool,
) -> ApiResult<Record<crate::models::SshKey>> {
    let private_key = Zeroizing::new(private_key);
    let passphrase = passphrase.map(Zeroizing::new);
    let pp = passphrase.as_deref().map(|p| p.as_str());
    let m = crate::keys::inspect_private(&private_key, pp).map_err(key_err)?;
    let key = crate::keymanager::new_key(&name, m, if save_passphrase { pp } else { None });
    save_record(&state, Collection::Keys, None, None, key).map(redacted_key)
}

/// Import a private key from a file, e.g. `~/.ssh/id_ed25519`. The file is
/// copied into the vault; the original is left alone.
#[tauri::command]
pub fn import_private_key_file(
    state: State<'_, AppState>,
    name: String,
    path: String,
    passphrase: Option<String>,
    save_passphrase: bool,
) -> ApiResult<Record<crate::models::SshKey>> {
    let text = read_key_file(&path)?;
    import_private_key(state, name, text.to_string(), passphrase, save_passphrase)
}

fn read_key_file(path: &str) -> ApiResult<Zeroizing<String>> {
    let path = PathBuf::from(crate::sshconfig::expand_path(path, &sftp::local::home()));
    let meta = std::fs::metadata(&path)
        .map_err(|e| ApiError::new("key", format!("cannot read {}: {e}", path.display())))?;
    // Keys are a few KB; refuse to slurp something that obviously isn't one.
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return Err(ApiError::new("key", format!("{} is not a key file", path.display())));
    }
    std::fs::read_to_string(&path)
        .map(Zeroizing::new)
        .map_err(|e| ApiError::new("key", format!("cannot read {}: {e}", path.display())))
}

/// Add someone's public key (no private half), e.g. to deploy it to hosts.
#[tauri::command]
pub fn import_public_key(
    state: State<'_, AppState>,
    name: String,
    public_key: String,
) -> ApiResult<Record<crate::models::SshKey>> {
    let m = crate::keys::inspect_public(&public_key).map_err(key_err)?;
    let key = crate::keymanager::new_key(&name, m, None);
    save_record(&state, Collection::Keys, None, None, key).map(redacted_key)
}

/// Rename a key and/or set its OpenSSH certificate (checked against the key).
#[tauri::command]
pub fn update_key(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
    name: String,
    certificate: Option<String>,
) -> ApiResult<Record<crate::models::SshKey>> {
    let mut key = load_key(&state, id)?.data.expect("checked");
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::new("validation", "a key needs a name"));
    }
    key.name = name.to_string();
    key.certificate = match certificate.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) {
        Some(c) => {
            crate::keys::check_certificate(&c, &key.public_key).map_err(key_err)?;
            Some(c)
        }
        None => None,
    };
    save_record(&state, Collection::Keys, Some(id), base_rev, key).map(redacted_key)
}

/// Re-encrypt a key under a new passphrase (empty = none). `current` may be
/// omitted when the vault already stores the passphrase.
#[tauri::command]
pub fn change_key_passphrase(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
    current: Option<String>,
    new: Option<String>,
    save_passphrase: bool,
) -> ApiResult<Record<crate::models::SshKey>> {
    let mut key = load_key(&state, id)?.data.expect("checked");
    let Some(private) = key.private_key.as_deref().map(|p| Zeroizing::new(p.to_string())) else {
        return Err(ApiError::new("key", "this is a public key only"));
    };
    let current = current.map(Zeroizing::new).or_else(|| key.passphrase.clone().map(Zeroizing::new));
    let new = new.map(Zeroizing::new).filter(|p| !p.is_empty());
    let text = crate::keys::change_passphrase(
        &private,
        current.as_deref().map(|p| p.as_str()),
        new.as_deref().map(|p| p.as_str()),
    )
    .map_err(key_err)?;
    key.private_key = Some(text.to_string());
    key.encrypted = new.is_some();
    key.passphrase = if save_passphrase { new.map(|p| p.to_string()) } else { None };
    save_record(&state, Collection::Keys, Some(id), base_rev, key).map(redacted_key)
}

/// Which credentials and hosts use a key.
#[tauri::command]
pub fn key_usage(state: State<'_, AppState>, id: Uuid) -> ApiResult<crate::keymanager::KeyUsage> {
    with_keys(&state, |v| crate::keymanager::key_usage(v, id))
}

/// Delete a key. Refused while credentials still use it.
#[tauri::command]
pub fn delete_key(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    with_keys(&state, |v| crate::keymanager::check_unused(v, id))?;
    delete_record(&state, Collection::Keys, id, base_rev)
}

/// Write a key's private half to a new file. This is the only way a private
/// key leaves the vault: it needs the master password every time (no grace
/// window), won't overwrite an existing file, and the file is owner-only.
#[tauri::command]
pub async fn export_private_key(
    state: State<'_, AppState>,
    id: Uuid,
    master_password: String,
    destination: String,
) -> ApiResult<()> {
    let master_password = Zeroizing::new(master_password);
    verify_master_password(&state, &master_password).await?;
    let key = load_key(&state, id)?.data.expect("checked");
    let Some(private) = key.private_key.map(Zeroizing::new) else {
        return Err(ApiError::new("key", "this is a public key only"));
    };
    let dest = PathBuf::from(crate::sshconfig::expand_path(&destination, &sftp::local::home()));
    write_new_private_file(&dest, private.as_bytes())
        .map_err(|e| ApiError::new("export", format!("cannot write {}: {e}", dest.display())))
}

fn write_new_private_file(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    let res = f.write_all(bytes).and_then(|_| f.sync_all());
    if res.is_err() {
        drop(f);
        let _ = std::fs::remove_file(path);
    }
    res
}

#[tauri::command]
pub fn save_identity(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    identity: Identity,
) -> ApiResult<Record<Identity>> {
    if identity.label.trim().is_empty() || identity.username.trim().is_empty() {
        return Err(ApiError::new(
            "validation",
            "label and username are required",
        ));
    }
    // Secrets left empty keep the stored ones, so the edit form never needs
    // to load them.
    let mut identity = identity;
    if let Some(id) = id {
        let stored =
            state
                .session
                .with_vault(|v| match v.get::<Identity>(Collection::Identities, id) {
                    Ok(r) => Ok(r.data),
                    Err(VaultError::NotFound { .. }) | Err(VaultError::Deleted { .. }) => Ok(None),
                    Err(e) => Err(e),
                })?;
        identity.auth = crate::reveal::merge_auth(identity.auth, stored.as_ref().map(|s| &s.auth))
            .map_err(|m| ApiError::new("validation", m))?;
        // Keep host ownership: the form doesn't send it.
        if identity.for_host.is_none() {
            identity.for_host = stored.and_then(|s| s.for_host);
        }
    } else {
        identity.auth = crate::reveal::merge_auth(identity.auth, None)
            .map_err(|m| ApiError::new("validation", m))?;
    }
    // Return the redacted form.
    let mut rec = save_record(&state, Collection::Identities, id, base_rev, identity)?;
    rec.data = rec.data.as_ref().map(Identity::redacted);
    Ok(rec)
}

#[tauri::command]
pub fn delete_identity(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
) -> ApiResult<()> {
    check_unchanged(&state, Collection::Identities, id, base_rev)?;
    // Detach the identity from any host that references it so the UI never
    // shows a dangling reference.
    let hosts: Vec<Record<Host>> = list_records(&state, Collection::Hosts)?;
    let mut detached = Vec::new();
    for h in hosts {
        if let Some(mut data) = h.data {
            if data.identity_id == Some(id) {
                data.identity_id = None;
                save_record(&state, Collection::Hosts, Some(h.id), Some(h.rev), data)?;
                detached.push(h.id);
            }
        }
    }
    delete_record_with(&state, Collection::Identities, id, None, Vec::new(), detached)
}

#[tauri::command]
pub fn list_snippets(state: State<'_, AppState>) -> ApiResult<Vec<Record<Snippet>>> {
    list_records(&state, Collection::Snippets)
}

#[tauri::command]
pub fn save_snippet(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    snippet: Snippet,
) -> ApiResult<Record<Snippet>> {
    if snippet.label.trim().is_empty() || snippet.command.trim().is_empty() {
        return Err(ApiError::new(
            "validation",
            "label and command are required",
        ));
    }
    save_record(&state, Collection::Snippets, id, base_rev, snippet)
}

#[tauri::command]
pub fn delete_snippet(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
) -> ApiResult<()> {
    delete_record(&state, Collection::Snippets, id, base_rev)
}
