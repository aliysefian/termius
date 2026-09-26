# Portable vault v2: plan and task list

Branch: `feature/portable-vault-v2`.

This plan is based on an inspection of the current code, then the
requirements for a portable, encrypted, multi-device vault. Items are ticked
as they land.

## 1. What exists today

| Area | Current state |
|---|---|
| Stack | Tauri v2 desktop app. Rust backend, SvelteKit 5 + Tailwind frontend, xterm.js terminals. |
| SSH | `russh` 0.63 and `russh-sftp` 3. Jump chains, agent and X11 forwarding, port forwarding (L/R/D), SFTP, exec runs, local PTY tabs. |
| Storage | `vault.rs`: a directory of per-record files `<collection>/<uuid>.enc`, encrypted with XChaCha20-Poly1305 and AAD `"<collection>/<uuid>"`. A plaintext `vault.json` holds the salt, the KDF parameters and a key verifier. Writes are temp file, fsync, then rename. Deletes are tombstones. |
| Keys | The record key is derived **directly** from the master password with Argon2id. There is no envelope and no recovery, and changing the password re-encrypts every record. |
| Concurrency | Per-record last-writer-wins on `updated_at`. **Conflicting edits on two PCs are silently resolved by whichever file the sync service keeps**, and conflicted-copy files are ignored. This violates the new requirements. |
| Host keys | Trust on first use, written automatically to a **local, per-machine, plaintext** `known_hosts` file. There is no explicit trust prompt, and nothing syncs through the vault. |
| Entities | Host (group path string, tags, environment, jump host, identity reference), Identity (username plus embedded password or private key or agent), Snippet, ForwardRule. There is no Key, Group, KnownHost, Proxy or Workspace entity. |
| Backups | None. |
| Keychain | None. |
| Secrets in UI | Redacted lists, and reveal needs the master password again. |

## 2. Design decisions

1. **Keep one encrypted file per record; don't switch to a single
   `vault.enc`.** With a single file, any edit on two machines produces a
   sync conflict on the whole vault. Per-record files let Dropbox and similar
   services merge independent edits naturally (PC A adds `server-A`, PC B adds
   `server-B`, and there's no conflict at all). The requirements allow a
   better layout, and this is the one the app already uses. SQLCipher is
   rejected: a binary database file in a sync folder is exactly the case that
   corrupts or conflicts.
2. **Envelope encryption.** A random 256-bit Vault Master Key (VMK) encrypts
   records. The master password goes through Argon2id to a key-encryption key
   (KEK), which wraps the VMK with XChaCha20-Poly1305. An optional recovery
   key wraps the same VMK independently. Changing the password rewraps the
   VMK only.
3. **Metadata is authenticated.** The wrapped-key AAD binds the vault ID,
   the format version, the slot kind and the KDF parameters. Editing
   `vault.json` makes unwrapping fail. It never yields a different key.
4. **Record AAD binds the vault ID too**
   (`sshvault/v2/<vault_id>/<collection>/<uuid>`). A record can't be moved
   between vaults, collections or IDs.
5. **Optimistic concurrency per record.** Every record carries `rev`,
   `base_rev`, `device_id`, `updated_at`, a SHA-256 `content_hash`, and an
   encrypted copy of its previous version (`prev`). A save states the revision
   it was based on.
   - If the file on disk moved on, the app does a three-way merge at the field
     level against `prev`.
   - Non-overlapping field changes merge automatically.
   - Overlapping changes are reported as a **conflict**. Secret fields are
     never auto-picked.
6. **Sync-service conflicted copies are detected**, not ignored. That covers
   Dropbox, Nextcloud, OneDrive, Google Drive and Syncthing naming. They're
   decrypted and merged where safe, and shown for manual resolution
   otherwise.
7. **Advisory locks and device registry** are encrypted records, informational
   only, and expire.
8. **Backups** are single-file encrypted snapshots (VMK, AEAD, AAD bound to
   the vault ID), with retention. Restoring verifies everything first and
   takes a pre-restore backup.
9. **Migration v1 → v2** happens on first unlock with the new version:
   - Take a raw backup of the v1 files.
   - Re-encrypt into a staging directory and validate it.
   - Swap it in through a journal, so an interrupted migration can be resumed
     or rolled back.
   - Old app versions already refuse any manifest version other than 1.
10. **Known hosts move into the vault** (synced and encrypted), with explicit
    trust. The first connection asks for confirmation and shows the
    fingerprint. A changed key shows the old and new fingerprints and requires
    "Replace". The existing local `known_hosts` entries are imported once.
11. **OS keychain** ("Remember on this device") uses the `keyring` crate:
    Windows Credential Manager, macOS Keychain, and Secret Service through
    zbus on Linux, in pure Rust. It stores the VMK, never the password. If the
    keychain is unavailable, the app asks for the password.

## 3. Task list

### Phase 1: vault core (crypto, format, storage)
- [x] V2 manifest: format ID, version, vault ID and name, algorithms, password slot, optional recovery slot, created_at.
- [x] Envelope encryption: random VMK, KEK from Argon2id, wrap and unwrap with authenticated metadata.
- [x] Recovery key: generate (256-bit, grouped base32), wrap, unlock with it, set a new password, rotate, remove.
- [x] Change password by rewrapping, with an atomic manifest save.
- [x] Record v2 envelope: vault-bound AAD, rev, base_rev, device, updated_at, content hash, prev.
- [x] Optimistic concurrency: `put` with a base revision, three-way field merge, conflict errors, secret fields atomic.
- [x] Conflicted-copy detection for all major sync services, auto-merge, conflict listing and resolution.
- [x] Atomic writes: temp file in the same directory, fsync file and directory, read back and verify, rename; stale temp cleanup; test failpoints.
- [x] Device identity and registry, plus advisory locks with expiry.
- [x] Encrypted backups: create, list, verify, restore (with a pre-restore backup), retention, automatic daily backup.
- [x] Integrity verification, read-only.
- [x] Migration v1 → v2 with a raw backup, staging, validation, journal and crash recovery. Refuse newer formats.
- [x] OS keychain: remember, unlock and forget; a mock backend in tests; fail safely when unavailable.

### Phase 2: entities
- [x] `groups`: nested folders with default jump host, credential and environment.
- [x] `keys` (Key Manager): generate Ed25519, ECDSA or RSA; import private or public; export public; rename; change passphrase; certificates; "used by"; guarded private-key export.
- [x] Credentials (identities) reference keys by ID (`AuthMethod::Key`), or a key file by path. Embedded keys from older records keep working as they are. They are deliberately not auto-migrated: two devices migrating the same credential at once would create duplicate key records.
- [x] `known_hosts` in the vault, with explicit trust and change handling, an interactive prompt, a history of replaced keys, and import from the local file.
- [x] `proxies`: SOCKS5 and HTTP CONNECT, plus ProxyCommand with explicit approval.
- [x] `workspaces`: saved tab layouts.
- [x] Host fields: favorite, custom metadata, keep-alive interval, proxy reference, custom environments.
- [x] Synced vault settings: backup retention, destructive-command patterns, paste threshold, clipboard clear time.

### Phase 3: SSH integration
- [x] Host-key verifier trait: vault store plus prompt. Tests keep the file store.
- [x] Group defaults resolved for jump host and credential.
- [x] Key references and certificates in authentication.
- [x] Proxy and ProxyCommand dialing.
- [x] Keep-alive per host.

### Phase 4: import and export
- [x] SSH config: ProxyCommand, LocalForward, RemoteForward, DynamicForward, ServerAliveInterval, ForwardAgent. Key files are copied into the vault only with approval, otherwise referenced by path.

### Phase 5: UI
- [x] Startup: Create New Vault, Open Existing Vault, Import SSH Config.
- [x] Create: name, location, password and confirm, recovery key, remember on this device. The recovery key is shown once.
- [x] Open: directory, password, remember; "Forgot password" leads to recovery and a new password.
- [x] Sidebar: Hosts, Favorites, Groups, Keys, Credentials, Tunnels, Snippets, Known Hosts, Vault, Settings (plus SFTP).
- [x] Vault screen: details, lock, change password, backup and restore, recovery settings, move vault, verify integrity, conflicts, devices.
- [x] Host-key trust dialog (unknown and changed).
- [x] Key Manager and Credentials screens.
- [x] Terminal: multi-line paste protection, destructive-command warnings, a production banner.
- [x] Settings: clipboard clear time, auto-lock options.

### Phase 6: tests
- [x] Crypto: right and wrong password, tampered ciphertext and metadata, salts, password change, recovery.
- [x] Storage: atomic write failpoints (disk full, crash before rename), stale temp cleanup, backup and restore, corrupted backups, migration and interrupted migration, unsupported version, missing or corrupted metadata, permission denied, missing folder.
- [x] Concurrency: two instances, non-conflicting merge, conflicting credential never silently overwritten, conflicted copies.
- [x] Keychain unavailable, and an interrupted password change.
- [x] Acceptance scenario, end to end against a real sshd: PC A creates and adds Production hosts with an imported Ed25519 key and connects; PC B opens, sees everything and connects; PC A adds redis-01 and PC B sees it; concurrent edits; and an attacker copy reveals nothing.

### Phase 7: security review and documentation
- [x] Repository sweep: hard-coded keys, plaintext persistence, logging, temp files, nonce reuse, randomness, permissions, path traversal, symlinks, backups, errors.
- [x] `docs/vault-architecture.md`.
- [x] `docs/vault-user-guide.md`.
