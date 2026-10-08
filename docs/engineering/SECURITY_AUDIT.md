# Security audit

Source review at v0.26.3. No dynamic testing, fuzzing or dependency scanning was run. Severity is the reviewer's
judgement of impact times reachability. "Verified" means the lines were opened in this session; "reported" means a
review agent read them and the claim was not re-opened. IDs map to tasks in `docs/tasks/`.

## Verified in this session

| # | Sev | Finding | Evidence | Task |
|---|---|---|---|---|
| S1 | High | **No CSP.** Any script injection in the webview (terminal-derived text, host names from imports, markdown, DB cells) can call every IPC command, including `reveal_identity` inside its 120 s grace window. | `tauri.conf.json:21` | SSHV-023 |
| S2 | High | **ProxyCommand token injection.** `%h` `%p` `%r` are pasted raw, then run by `sh -c` / `cmd /C`. A hostname/username from ssh_config, Ansible, CSV, PuTTY import or vault sync can carry `;`, `$()`. The user approves the template, never the expanded string. | `dial.rs:245-257` | SSHV-024 |
| S3 | High | **Silent app exit** from `tokio::spawn` in sync commands (`panic=abort`). Availability, not confidentiality, but also means no key zeroize on that exit. Fixed at three sites. | `containers/mod.rs:534`, `kube.rs:399`, `monitor.rs:76` | SSHV-004 |
| S4 | Med | **Remote file name path traversal on download.** Only `.`/`..` are skipped; names are joined into the local path. A hostile server can return `../x` or an absolute name. | `sftp.rs:227`, `files/engine.rs:200-216` | SSHV-025 |

## Reported by review agents (read from source, not re-opened)

| # | Sev | Finding | Evidence | Task |
|---|---|---|---|---|
| S5 | High | **No rollback protection.** A sync host or buggy sync tool can restore an older valid record, tombstone or `vault.json`; AAD binds id/collection/vault but not revision. Documented as a non-goal for whole-folder rollback; per-record rollback is detectable only after newer revisions were seen. | `docs/vault-architecture.md` §1; `vault/mod.rs:659` | SSHV-002 |
| S6 | Med-High | **ProxyCommand approval is synced and client-controlled.** `approved` lives in the synced `ProxySpec`; `save_proxy` takes it from the webview; `test_proxy` runs whatever spec it is given. A compromised peer or webview can arrive pre-approved. | `models.rs:432`, `commands.rs:2815, 3285-3335` | SSHV-024 |
| S7 | Med | **Auto-lock is webview-only and off by default.** Backend has no timer. Terminal output is not "activity" but also does not count as keepalive in the other direction. | `+page.svelte:126-144`, `settings.svelte.ts:168` | SSHV-001 |
| S8 | Med | **Lock is bypassable on a remembered device.** `lock_vault` leaves the keychain entry; `unlock_with_device` reopens without a password. Documented trade-off, but "Lock" does not mean locked. | `commands.rs:661`, `keychain.rs` | SSHV-001 |
| S9 | Med | **VMK is never rotated.** A leaked old password/recovery key plus an old `vault.json` still opens everything. | `vault/mod.rs:581` | SSHV-001 |
| S10 | Med | **Keyboard-interactive sends the saved password to every prompt**, including OTP prompts. | `ssh.rs:1068-1090` | SSHV-003 |
| S11 | Low (was Med; built-ins `{{host}}`/`{{label}}` now quoted, SSHV-016a) | **Runbook templating is unquoted by default** (`{{x}}` raw, `{{x|q}}` quoted). Free-text parameters in a scheduled or shared runbook become shell injection. | `runbook.rs:367-381` | SSHV-016 |
| S12 | Med | **Run output stored unmasked** (16 KiB per step) in local history; code comment admits secrets may appear. | `runbookrun.rs:1-3`, `runbookhistory.rs` | SSHV-016 |
| S13 | Med | **`run_hook` backend does not check approval.** The UI approves, the command trusts the caller. With S1 this widens. | `commands.rs:~2692`, `hooks.rs` | SSHV-023 |
| S14 | Med | **No auth/session timeouts** after TCP connect; a stalling server holds a pane and a task indefinitely. | `ssh.rs:900-940` | SSHV-003 |
| S15 | Med | **Forward listener DoS bounds:** accept-error busy loop, no tunnel cap, no SOCKS5 handshake timeout. | `forward.rs:~295, 340` | SSHV-003 |
| S16 | Low-Med | Manifest KDF memory has a floor but no ceiling; a hostile synced `vault.json` could ask for huge memory. Unconfirmed. | `vault/format.rs:111` | SSHV-001 |
| S17 | Low-Med | `read_file` for remote edit has no size cap (OOM). | `sftp.rs:331`, `remoteedit.rs:~82` | SSHV-025 |
| S18 | Low | Hand-rolled constant-time compare instead of `subtle`; best-effort directory fsync; no `mlock`/core-dump hardening; keychain stores VMK base64. | `crypto.rs:134`, `atomic.rs:137` | SSHV-001 |
| S19 | Low | Host-key store keeps one key per host:port; algorithm change reads as a changed key, training click-through. Cert host keys: CA signature/principals unchecked. | `hostkeys.rs:44-56`, `ssh.rs:589` | SSHV-003 |
| S20 | Info | `suppaftp` built with feature `deprecated`; `des` crate present; `oracle-rs` 0.1; pre-1.0 `ironrdp-*`; three vendored patched crates (picky, sspi, ironrdp-session) will not receive upstream advisories automatically. | `Cargo.toml`, `src-tauri/vendor/README.md` | SSHV-026 |
| S21 | Info | CI has no `cargo audit`/`deny`, `pnpm audit`, `cargo fmt --check`, CodeQL; actions pinned by tag not SHA; toolchain floats on `stable`. | `.github/workflows/build.yml` | SSHV-026 |
| S22 | Info | Tagged releases silently ship unsigned / without update support when signing secrets are missing. | `build.yml` (reported) | SSHV-026 |

## Checked and found sound (reported)

Record AEAD with per-record AAD and 192-bit random nonces; zeroizing key types with redacted `Debug`; list endpoints
return redacted models and have tests; `export_private_key` requires the master password and refuses to overwrite;
host-key verification has no production auto-accept path; agent and X11 forwarding refuse server-opened channels unless
enabled for that host; control/agent sockets are 0600 in verified private directories; remote argv construction uses
`shell_quote`/name validation; no secrets in remote command lines were found; Tauri capabilities grant no fs, shell or
http plugin.

## Not covered

`selfupdate.rs` artifact verification, `hostcreds.rs` body, `keys.rs`/`keymanager.rs`/`certs.rs`/`x11.rs`/`mosh.rs`
(skimmed), `rdp/`, `vnc/`, `db/*` driver TLS handling, `opener:default` scope vs `linksafety.ts`, the frontend XSS
surface (which determines how much S1 matters), and everything at runtime.
