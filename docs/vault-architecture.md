# Portable vault: architecture and threat model

This describes how SSHVault stores hosts, credentials and keys in a folder that a file-sync service (Dropbox, OneDrive, Google Drive, iCloud Drive, Nextcloud, Syncthing and others) copies between computers. The central rule: **the sync provider never has to be trusted.** Everything it stores is encrypted and authenticated on the device, and nothing it holds is enough to open the vault.

Code references are relative to `src-tauri/src/`.

## 1. Goals and non-goals

**Goals**

- A vault is a plain folder. Moving it, syncing it or backing it up needs no server and no account.
- A copy of the folder reveals no secrets and no inventory: no host names, user names, addresses, group names or key material.
- Several devices can use the vault at once. Edits merge when they touch different fields. Conflicting edits are reported and never silently dropped.
- A crash, full disk or interrupted sync never corrupts the vault, and never makes the app fall back to writing plaintext.
- Old app versions refuse newer formats instead of damaging them. Upgrades never touch the only copy of the data.

**Non-goals**

- **A compromised device while the vault is unlocked.** Malware running as the user on an unlocked device can read memory, keystrokes and the screen, and can use the SSH sessions. Encryption at rest can't prevent that, and SSHVault doesn't claim to.
- **Hiding activity.** An observer with access to the folder sees how many records of each type exist, their sizes, and when they change. It does not see their contents.
- **Rollback of the whole folder.** Someone who controls the sync service can restore an older complete, validly encrypted copy of the folder. Revisions make a rollback of *individual* records visible to devices that already saw newer ones (their writes are refused as conflicts), but an old complete snapshot is still a valid vault.
  Since 0.27 each device also keeps its own record of the newest revision it has seen of every record (`vault/highwater.rs`, stored outside the synced folder). A record that later shows an older revision, or a live record that disappears, is reported on unlock and on the Vault screen. This detects a rollback of anything this device has already seen; it does not stop it, and a device that has never seen the newer data can't tell.

## 2. Keys

```
master password ──Argon2id(salt, m, t, p)──▶ KEK ─┐
                                                  ├─unwrap (XChaCha20-Poly1305, AAD binds vault + slot + KDF params)─▶ VMK
recovery key ─────Argon2id(salt, m, t, p)──▶ KEK ─┘

VMK ──XChaCha20-Poly1305 (random 192-bit nonce, AAD = vault/collection/id)──▶ each record, backup and key check
```

- **Vault master key (VMK).** 256 random bits from the OS CSPRNG, generated once when the vault is created. Every record is encrypted with it. It never changes, so changing the password doesn't mean re-encrypting records.
- **Key slots** in `vault.json` hold the VMK wrapped by a key-encryption key (KEK):
  - a `password` slot, whose KEK is derived from the master password;
  - an optional `recovery` slot, whose KEK is derived from a 256-bit random recovery key shown to the user once.
  - Each slot has its own random 16-byte salt and records its Argon2id version and parameters. The slot's AAD binds the vault ID, the slot kind and every KDF parameter, so tampering with the parameters (for example lowering the cost) makes unwrapping fail. Parameters below a floor (19 MiB, 2 passes) are refused.
- **Default KDF:** Argon2id v1.3, 64 MiB, 3 passes, 4 lanes (RFC 9106's second recommended profile).
- **Changing the password** rewraps the VMK under a new salt and writes `vault.json` atomically. Records don't change. An interrupted change leaves the old password working.
- **Resetting with the recovery key** unwraps the VMK with the recovery slot and writes a new password slot. The recovery key keeps working until it's replaced or removed.
- **Key check.** `vault.json` contains a fixed plaintext encrypted under the VMK (AAD `sshvault/v2/key-check/<vault id>`). It confirms that a VMK loaded from the OS keychain belongs to this vault before any record is read.
- **No custom cryptography.** The primitives come from RustCrypto: `argon2`, `chacha20poly1305` and `sha2`/`hmac`, with OS randomness from `rand::rngs::OsRng` and `getrandom`. XChaCha's 192-bit random nonces make nonce reuse negligible without a counter. Secrets live in `Zeroizing` buffers and are wiped on drop.

### Remember on this device

When chosen, the VMK (never the master password) is stored in the OS credential store under the service `SSHVault`, account `vault-<vault id>` (`keychain.rs`):

- Windows: Credential Manager (DPAPI)
- macOS: Keychain
- Linux: Secret Service

Anyone who can read the user's keychain on that machine can open the vault; that's the trade-off the user opts into. If the keychain is unavailable or refuses, the app reports it and asks for the password. It never falls back to storing the key anywhere else. `forget_device` removes the entry.

## 3. On-disk format (v2)

```
MySSHVault/
├── vault.json                   format, version, vault id, cipher, key slots, key check. No secrets, no names.
├── hosts/<uuid>.enc             one encrypted file per record
├── identities/  keys/  groups/  known_hosts/  proxies/  snippets/  forwards/  workspaces/  settings/
├── devices/<device uuid>.enc    device registry (name, platform, app version, last seen)
├── locks/<device uuid>.enc      advisory "open on this device" markers (not merged, not backed up)
└── backups/YYYYMMDDTHHMMSSZ-<device8>.enc
```

- Every record file is `magic | version | nonce | ciphertext‖tag`, with AAD `sshvault/v2/<vault id>/<collection>/<uuid>`. A file copied to another ID, another collection or another vault fails authentication (`vault/tests.rs::tampered_or_transplanted_records_fail_authentication`).
- The decrypted envelope has these fields:
  - `id`
  - `rev` (starts at 1)
  - `base_rev` (the revision this write was based on)
  - `device_id`, `updated_at`
  - `deleted` (tombstone)
  - `content_hash`: SHA-256 of `(deleted, data)`
  - `data`
  - `prev`: the previous revision, used as the common ancestor for three-way merges. Tombstones keep no data.
- **Known-host IDs** are `HMAC-SHA256(HMAC-SHA256(VMK, "sshvault/v2/subkey/known-host-id"), host + port)` truncated to a UUID (`hostkeys.rs`). Two devices trusting the same server converge on one record, and the file name reveals nothing about the host.
- **Directories are 0700 and files 0600** on Unix, and are created with those modes rather than tightened afterwards.

### Atomic writes (`vault/atomic.rs`)

Every write follows the same steps, and a failure at any step leaves the previous file intact:

1. Write to an `O_EXCL` temp file in the same directory, with mode 0600.
2. `fsync` the temp file.
3. Read it back and compare with what was written.
4. `rename` it over the target.
5. `fsync` the directory.

Stale temp files from crashed runs are removed on open. Test failpoints simulate a full disk, a failed sync and a crash before the rename.

### Reading untrusted files

Record, manifest, backup, journal and conflict-copy reads go through `read_regular_file`:

- Symlinks and other non-regular files are refused.
- Size caps apply: 16 MiB per record, 1 MiB for the manifest, 512 MiB per backup.

A hostile or broken folder therefore can't redirect reads outside the vault or exhaust memory.

## 4. Multiple devices

### Optimistic concurrency (`Vault::put`)

Each save says which revision it was based on:

- `Base::New` for a record that must not exist yet.
- `Base::Rev(n)` for the revision the user was editing. The UI captures it when a form opens.
- `Base::Latest` for internal housekeeping only.

If the file on disk is still at `n`, the write goes ahead as `n + 1`. If another device moved it on:

- Identical content is a no-op.
- Otherwise a **field-level three-way merge** runs against `prev` (`vault/merge.rs`). Fields changed on only one side are combined; `tags` merge as a set. Nested values, which include every credential (`auth`) and key, are atomic, so two different secrets are never spliced together.
- If both sides changed the same field differently, or the record was deleted elsewhere, the save is refused with a `Conflict` error naming the fields and the other device. The UI reloads and shows the other version. **A credential is never silently overwritten.**

### Sync-service conflicted copies (`vault/conflicts.rs`)

When two devices write the same file while offline, sync services keep both, as copies such as:

- `…(conflicted copy …)` (Dropbox)
- `….sync-conflict-…` (Syncthing)
- `… (1).enc` (Google Drive)
- `…-DESKTOP-ABC.enc` (OneDrive)
- `….conflict….` (Nextcloud)

On unlock and whenever the watcher sees one, `reconcile()` authenticates the copy and handles it:

- If it merges cleanly with the current version, the merge is written and the copy removed.
- Otherwise it stays and appears under *Vault → Sync conflicts* with both versions side by side, secrets redacted.

Resolution validates the file name, so no path separators or `..` get through.

### Live updates

A file watcher (`sync.rs`) decrypts changed records and sends them to the UI. Records with secrets (credentials, keys, proxies) are redacted exactly as the list commands redact them (`models::redact_record`), so a record edited on another device never pushes a password or private key into the webview.

### Devices and locks

- Each device registers itself in `devices/`.
- While unlocked, it keeps a lock file refreshed every 60 seconds, which expires after 2 minutes.
- Locks are advisory: they let the UI say "also open on PC-B" and never block anything.

## 5. Backups and restore (`vault/backup.rs`)

- A backup is one file holding every synced record. It is encrypted with the VMK and AAD `sshvault/v2/<vault id>/backup`, and stored in `backups/` inside the vault, so it syncs too.
- An automatic backup is taken at most once a day on unlock. Manual backups can be made from the Vault screen.
- Retention is a shared setting (30 by default) and prunes the oldest backups.
- **Restore** works like this:
  1. Decrypt and validate the whole backup first: correct vault, format and every record.
  2. Take a safety backup of the current state.
  3. Write each differing record as a new revision, so other devices receive the restore as ordinary changes rather than a rollback that conflicts.
- A corrupted backup, or one from another vault, is refused before anything is changed.

## 6. Integrity verification (`vault/integrity.rs`)

The check is read-only. It checks:

- the manifest and key check
- that every record authenticates and decodes
- that ID/revision/hash metadata is consistent
- that references resolve: host → credential, jump host, proxy; credential → key; forward → host; group defaults
- every backup
- for leftover temp files and conflict copies

It reports errors and warnings and never modifies anything. A test snapshots the folder before and after to prove it.

## 7. Versions and migration (`vault/format.rs`, `vault/migrate.rs`)

- `vault.json` carries `format: "sshvault"` and `version: 2`. Newer versions are refused with `UnsupportedManifestVersion`, never rewritten.
- **v1 → v2** runs on the first unlock with the password. It never touches the only copy:
  1. Unlock v1 to verify the password.
  2. Copy the untouched encrypted v1 files to `backups/v1-<stamp>/`.
  3. Re-encrypt everything into `.staging-v2/` under a new VMK (the vault ID is kept).
  4. Decrypt the staged copy and compare it with the original.
  5. Write a journal, swap the folders one by one, then replace `vault.json` atomically.
  6. Clean up.
- If the process dies mid-swap, the next open reads the journal: it rolls back if `vault.json` is still v1, or finishes the clean-up if it's v2.
- A journal from another device less than 10 minutes old means "migration in progress elsewhere", and this device waits.

## 8. Connections

- **Credentials** (`models::Identity`) are separate from hosts. The `auth` field is one of:
  - `password`
  - `private_key` (embedded; older records)
  - `key`: a reference into the Key Manager
  - `key_file`: a path on this computer, never copied into the vault
  - `agent`
- **Groups** carry defaults: credential, jump host, proxy and environment. `keymanager::effective` applies the nearest group's value when a host doesn't set its own. `keymanager::resolve_target` turns a host into an SSH target: group defaults, jump chain, key references, certificate, proxy and keep-alive. The desktop commands and the acceptance test share it.
- **Key Manager** (`keys.rs`, `keymanager.rs`):
  - Generates Ed25519, ECDSA P-256/P-384 and RSA 3072/4096 keys from the OS CSPRNG, optionally encrypted in OpenSSH format (bcrypt-pbkdf + AES-256-CTR).
  - Validates imported keys (OpenSSH, PEM, PKCS#8, PuTTY) and their passphrases before storing them.
  - Checks certificates against their key.
  - Refuses to delete a key that credentials still use.
  - **Private keys only leave the vault through `export_private_key`.** It asks for the master password every time (no grace window), won't overwrite a file, and creates the file owner-only.
- **Host keys** (`ssh.rs`, `hostkeys.rs`):
  - Trusted keys live in the vault's `known_hosts` collection.
  - An *unknown* key asks the user, showing the fingerprint. A *changed* key shows the old and new fingerprints, and replacing it means typing `replace`. Cancel is the default.
  - Replaced keys go into the record's history.
  - With no one to ask (for example a background job), unknown or changed keys are refused.
  - The pre-v2 per-device `known_hosts` file is imported once and deleted. The user's `~/.ssh/known_hosts` can be imported on request; hashed entries are skipped.
- **Proxies** (`dial.rs`) are applied to the first hop only:
  - SOCKS5 (RFC 1928/1929) and HTTP CONNECT, both of which resolve names at the proxy.
  - `ProxyCommand`. It only runs once `approved` is set by an explicit save in the UI. Imported commands are always unapproved.
- **Tunnels** bind to `127.0.0.1` unless the user chooses otherwise.

## 9. What the UI adds

- **Paste protection:** a paste that would run several commands, or any command on a production host, is shown and confirmed first. The threshold is a shared setting.
- **Destructive-command warnings on production hosts:** Enter is held when the typed line matches a pattern such as `rm -rf /`, `DROP TABLE` or `terraform destroy`. The app only sees keystrokes (not history expansion, completion, aliases or scripts), so this is a reminder and never a guarantee, and the UI says so.
- A red **production banner** on sessions to production hosts.
- **Clipboard:** copied secrets are cleared after a shared interval (30 seconds by default) if the clipboard still holds them.
- **Auto-lock** (never, or 5, 15, 30 or 60 minutes). Locking closes every session and removes remote-edit temp files.
- **Remote edit** downloads to a per-user cache folder, creates it 0700 and the file 0600, and deletes both when the edit ends, when the vault locks, and at start-up.

## 10. Security review (this branch)

| Area | Finding | Resolution |
|---|---|---|
| Hard-coded keys | None. Every key is derived (Argon2id) or random (OS CSPRNG). | — |
| Plaintext persistence | The pre-v2 per-device known_hosts file was left behind. | Imported into the vault, then deleted (only after the import fully succeeded). |
| Plaintext persistence | Session logs were created with default permissions. | Created 0600. Logs are an explicit user action and live where the user chooses. |
| Temp files | Remote-edit copies used a shared `/tmp/sshvault-edit` base, which another local user could pre-create and race. | Moved to the per-user app cache folder. The folder is created 0700 and files 0600 with `create_new`. |
| Permissions | Private folders were created then chmodded, leaving a brief window. | Created with mode 0700 directly (`DirBuilder`). |
| Live events | Keys and proxies edited on another device were sent to the webview unredacted. | Every change event is redacted like the lists (`models::redact_record`), with a test. |
| Symlinks / DoS | A planted symlink or huge file in the synced folder was read blindly. | Only regular files within size caps are read, with a test. Writes replace links rather than following them. |
| Path traversal | Backup and conflict file names are user-supplied. | Validated before any filesystem access. `verify_backup` used to stat first; fixed. |
| Nonce reuse | Random 192-bit XChaCha nonces from the OS CSPRNG on every encryption. | — |
| Randomness | Only `OsRng`/`getrandom` is used for secrets. UI IDs are not security-relevant. | — |
| Logging | No logging of secrets or vault contents. `eprintln!` only appears in tests. | — |
| Errors | Errors carry paths, kinds and counts, never key material or passwords. Conflict details carry field names only. | — |
| Destructive patterns | The default `(?i)` patterns were invalid in JavaScript, so SQL patterns never matched. `reboot` also matched inside words. | `(?i)` is translated to the `i` flag, the reboot pattern is anchored to command position, with tests. |
| Backups | Encrypted with the VMK and bound to the vault ID. Corrupted or foreign backups are refused before restore. | — |

### The SSH agent (`agent.rs`)

- An opt-in, per-computer agent serves Key Manager keys while the vault is
  unlocked:
  - on Unix, a socket in `$XDG_RUNTIME_DIR/sshvault` (or the per-user app cache
    folder), inside a 0700 folder and with the socket itself 0600;
  - on Windows, a named pipe that refuses remote clients.
- Each key is off by default. When switched on, it is either "ask" (the UI
  approves every signature, and an unanswered request times out after 60 s)
  or "allow" (signs without asking while the vault is unlocked).
- The agent is read-only: it answers list-identities and sign requests only,
  and refuses add, remove, lock, smartcard and extension requests.
- Any process running as the user can talk to the agent. That is the same
  exposure as `ssh-agent`, and "ask" mode exists for keys where that matters.
- The agent stops when the vault locks.

Known limitations:

- **ProxyCommand approval syncs:** approving on one device approves on all devices using the vault.
- **Credential choices:**
  - Saving a key passphrase in the vault is a convenience the user chooses.
  - Key files referenced by path depend on each computer's own file.
- **Older records:** credentials that embed a private key keep working as they are. They aren't auto-migrated into the Key Manager, because two devices doing that at once would create duplicate keys.
