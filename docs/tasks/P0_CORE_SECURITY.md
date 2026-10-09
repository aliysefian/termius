# P0: core security and stability

Statuses: TODO -> IN_PROGRESS -> TESTING -> DONE (BLOCKED when waiting). Findings referenced as S#/D# are in
`docs/engineering/`.

---
## SSHV-004 Application reliability
```yaml
id: SSHV-004
title: Diagnosable crashes, clean shutdown, spawn-context safety
module: core
priority: P0
status: TESTING
dependencies: []
risk: low
```
**Problem.** Release builds abort on panic with no output. `tokio::spawn` from sync Tauri commands killed the app when
opening container logs (S3). No logging crate, no panic hook, no exit handler (D1, D2).
**Existing.** Fixes for three spawn sites: `containers/mod.rs:534`, `kube.rs:399` (released in 0.26.3),
`monitor.rs:76` (in working tree, unreleased).
**Expected.** (a) every panic writes a redacted line + backtrace to `<app-data>/crash.log` before aborting; (b) `tracing`
with a redacting formatter, off by default for content, on for events; (c) `RunEvent::Exit` closes sessions, forwards,
agent socket and vault lock; (d) a test or lint that fails when a sync `#[tauri::command]` can reach `tokio::spawn`.
**Plan.** `panic::set_hook` in `lib.rs` before `.run`; `tracing` + `tracing-appender`; exit handler calling the manager
`close_all`s; a `build.rs`/test grep of `commands.rs` for `pub fn` (non-async) bodies calling `.start_`/`spawn`, or
convert those commands to `async`.
**Files.** `lib.rs`, `main.rs`, `Cargo.toml`, `monitor.rs`, `commands.rs`, new `logging.rs`.
**Security.** Hook and logs must never print secrets: log only ids/kinds, never request payloads; test with a vault
holding a known sentinel password.
**Acceptance.** Induced panic produces `crash.log` without the sentinel; quitting releases the advisory lock file;
lint test fails on a deliberately bad sync command.
**Tests.** Rust unit test for the redaction formatter; test for the lint; manual: open container, kube and monitor logs
on a release build.
**Rollback.** Revert the commit; no data format change.
**Progress (panic hook).** New `crashlog.rs`, installed first in `commands::setup`: a std-only `panic::set_hook` appends
`[unix-time] vX.Y.Z panic in thread 'name' at file:line: message` to `<config dir>/crash.log` (0600, kept under 128 KiB) and then
lets the previous hook run. The message is cut where an `Err` value would be printed, so error values never reach the log.
Tests (4, scratch crate on the real file): value scrubbing, one-line/length cap, bounded append, and a real panicking thread
whose error value `hunter2` does not appear. Not done: `tracing`/structured logging, `RunEvent::Exit` cleanup, the lint for
sync commands reaching `tokio::spawn`; the hook cannot report a crash in a thread that dies before `setup` runs.
**Completion evidence.** Pending.* The wiring in `lib.rs` and `commands.rs` was not compiled (two lines); CI must pass; the
`monitor.rs` spawn fix is verified only by reading. The `monitor.rs` one-line change is verified only by reading (cannot compile
here); CI must pass before DONE.

---
## SSHV-023 Webview hardening
```yaml
id: SSHV-023
title: Content-Security-Policy and backend approval for hooks
module: security
priority: P0
status: TODO
dependencies: []
risk: medium
```
**Problem.** `"csp": null` (S1). `run_hook` trusts the caller (S13).
**Expected.** A restrictive CSP (`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data: blob:; connect-src ipc: http://ipc.localhost`), tested against every view; `run_hook` accepts only
a one-time approval token minted by an approval command.
**Plan.** Add CSP, run the app, fix violations (xterm, fonts, inline styles); add token flow for hooks.
**Files.** `tauri.conf.json`, `hooks.rs`, `commands.rs`, `HookApproval` UI.
**Security.** CSP must not use `unsafe-eval`. Check `opener:default` scope against `linksafety.ts`.
**Acceptance.** No CSP violations in console across all views including RDP/VNC canvases and the xterm renderer;
`run_hook` without a token fails.
**Tests.** Rust test for token expiry/single use; e2e smoke (`src/lib/__tests__/e2e`) with console-error check.
**Rollback.** Revert CSP string only (config change).
**Evidence.** Pending. Needs a runnable build, which this machine lacks.

---
## SSHV-024 ProxyCommand safety
```yaml
id: SSHV-024
title: Quote ProxyCommand tokens; approval per device and per command text
module: connections
priority: P0
status: TESTING
dependencies: []
risk: medium
```
**Problem.** Raw `%h/%p/%r` substitution (S2); `approved` synced and client-set (S6).
**Expected.** Substituted values shell-quoted (POSIX) or rejected unless `[A-Za-z0-9._:@-]`; approval stored in local
per-device state keyed by SHA-256 of the template; editing the template clears approval; `test_proxy` cannot self-approve.
**Plan.** Change `expand_proxy_command` (`dial.rs:245`); move approval out of `ProxySpec` with a vault-format-compatible
migration (ignore the old field on read, keep writing it as `false`).
**Files.** `dial.rs`, `models.rs`, `commands.rs`, `sshconfig.rs`, ProxyForm component.
**Security.** Windows `cmd /C` has no safe quoting: reject metacharacters there instead of quoting.
**Acceptance.** Host `a;touch /tmp/x` never runs the command; an approved proxy synced to a new device is unapproved.
**Tests.** `dial.rs` unit tests with hostile hostnames on both shells; vault compatibility test that an old vault with
`approved:true` opens.
**Rollback.** Old field is still read; revert code only.
**Progress.** Part 1 done in `dial.rs`: `expand_proxy_command` now returns `Result`; `%h`/`%r` are refused if they contain
control characters or start with `-`, quoted as one POSIX word on Unix, and limited to plain words on Windows. Evidence:
new test `proxy_command_values_are_data_not_shell` passes in a scratch crate built from the extracted code (the Tauri
crate cannot compile here); CI has not run it. Part 2 done (owner approved 2026-10-09): new `proxyapproval.rs` keeps approvals per computer in `config.json`
(`approved_proxy_commands`, SHA-256 of the trimmed command), loaded at startup. `dial.rs` now ignores the synced `approved` flag
and runs a ProxyCommand only if this computer approved that exact text, so editing the command withdraws approval and a proxy
synced from another device starts unapproved. `save_proxy` records approval locally and always stores `approved: false` in the
vault; `list_proxies` reports this computer's answer; `test_proxy` may approve for the duration of one test only. Existing
approvals in the vault no longer count, so each computer asks once more. Tests: 3 in `proxyapproval.rs` pass in a scratch crate;
`proxy_command_needs_approval` in `dial.rs` was changed to prove a record flagged approved is refused, but was not compiled (full
crate); the command-layer edits in `commands.rs` were not compiled either. UI text in `GroupsPanel.svelte` now says "on this
computer"; `pnpm check`: 0 errors, 0 warnings.
**Evidence.** Partial, as above.

---
## SSHV-025 File transfer path safety
```yaml
id: SSHV-025
title: Reject hostile remote file names; cap remote-edit reads
module: files
priority: P0
status: TESTING
dependencies: []
risk: low
```
**Problem.** Names with `/`, `\`, NUL, `..` or absolute form from SFTP/SCP/FTP listings are joined into local paths (S4);
`read_file` unbounded (S17).
**Expected.** Listing parsers drop or flag such entries; the plan builder asserts every destination stays under the
chosen directory; `read_file` has a configurable cap (default 64 MiB) with a clear error.
**Files.** `sftp.rs`, `files/scp.rs`, `files/ftp.rs`, `files/engine.rs`, `files/local.rs`, `remoteedit.rs`.
**Acceptance.** A fake server returning `../../evil` produces an error entry and no file outside the destination.
**Tests.** Extend `files/memory.rs` fake backend with hostile names; engine-level test that the local path is contained.
**Rollback.** Revert; no format change.
**Progress.** `files::is_plain_name` rejects empty, `.`, `..` and names containing `/`, `\` or NUL. `files/engine.rs` `plan()` skips
such entries (counted as skipped) before joining them onto the destination, which covers SFTP, SCP, FTP and local sources
because they all go through the engine. `SftpConn::read_file` now reads at most `MAX_EDIT_BYTES` (64 MiB) and errors above it.
Not done: the listing parsers still show hostile names in the pane; they are only prevented from being copied.
**Evidence.** Tests `plain_names_are_one_path_part` and `a_server_listing_a_hostile_name_cannot_write_outside_the_destination`
(memory backend gained `list_also` to inject hostile entries). Run in a scratch crate that symlinks the real `engine.rs` and
`memory.rs` and a trimmed `mod.rs`: 15 passed; the hostile-name test fails with the `engine.rs` change stashed and passes
with it. The `read_file` cap composes the existing `read_head` and was not compiled or tested here. The full crate has not
been built; CI must pass before DONE.


---
## SSHV-001 Vault hardening
```yaml
id: SSHV-001
title: Backend auto-lock, lock semantics, KDF bounds
module: security
priority: P0
status: IN_PROGRESS
dependencies: [SSHV-004]
risk: medium
```
**Existing (do not redo).** XChaCha20-Poly1305, Argon2id, key slots, recovery key, tamper tests, atomic writes, backups,
migrations, redacted lists (see FEATURE_INVENTORY).
**Gaps.** S7 backend auto-lock; S8 lock vs remembered key; S16 no KDF ceiling; S18 `subtle` crate, Windows directory flush,
unlock lockout.
**Out of scope by owner decision (2026-10-09).** Data-key (VMK) rotation is not planned; the risk is recorded as S9 and
accepted. Nothing in this task changes how records or backups are encrypted, so no migration is involved.
**Plan.** (1) Idle timer in `session.rs`, reset by IPC activity and by session I/O, calls the existing lock path;
(2) setting "forget remembered key when locking" default on, with UI text; (3) reject `m_cost` above a ceiling;
(4) replace the hand-rolled compare with `subtle`; (5) exponential unlock delay.
**Security.** Lock must still close sessions, agent and control socket exactly as `lock_vault` does today.
**Acceptance.** Existing vaults open unchanged; a hostile manifest with a huge `m` is refused without allocating; the vault
locks after the idle time with the webview frozen; with the new setting on, unlock after lock asks for the password.
**Tests.** Extend `vault/tests.rs`: KDF ceiling, idle lock with an injected clock, unlock delay.
**Rollback.** Revert the commit; no data-format change.
**Progress (2026-10-09).** (a) KDF ceilings (1 GiB, 32 passes, 1-16 lanes) in `vault/format.rs`; evidence: the real vault suite
(79 tests, `vault/*`, `crypto.rs`, `models.rs` in a scratch crate with three stub enums) passes with the new cases in
`tampered_metadata_is_detected`, and with the ceiling stashed that test fails after running ~104 s on a hostile 4 GiB manifest,
which is the denial of service this closes. (b) Lock now forgets the remembered-device key unless `keep_key_on_lock` is set (owner
decision: on by default, i.e. forget). Finding: `UnlockScreen.svelte:172` unlocks with the device key as soon as the lock screen
mounts, so on a remembered device Lock re-opened the vault at once; forgetting the key fixes that too. New `set_keep_key_on_lock`
command, `VaultInfo.keep_key_on_lock`, checkbox in `VaultPanel.svelte`. These command/UI edits were not compiled with the full
crate (`pnpm check` 0 errors). **Not done:** backend idle timer, `subtle`/unlock delay (S18, low; `subtle` would need a Cargo.lock
edit I cannot verify), `mlock`.
**Evidence.** Partial, as above.

---
## SSHV-002 Sync integrity and visibility
```yaml
id: SSHV-002
title: Rollback detection, sync status, version history
module: sync
priority: P0
status: TODO
dependencies: [SSHV-001]
risk: high
```
**Existing.** Three-way merge, conflict copies, device registry, advisory locks, atomic writes.
**Gaps.** S5 rollback; no sync-status UI; one `prev` per record; no retry/backoff on I/O errors.
**Plan.** Per-device local high-water mark of `(record id -> max rev seen)` stored encrypted outside the synced folder;
reading a lower rev, a missing file with a prior live rev, or a vault.json without a newer counter raises a visible
"rollback suspected" state that blocks writes until the user chooses. Sync status model (idle, changed-by-other-device,
conflicts, error). Retry with backoff for transient I/O. Optionally keep N prior revisions in encrypted history.
**Non-goal.** Provider-specific adapters (see roadmap).
**Acceptance.** Replaying an older ciphertext is detected on next scan; legitimate deletion via tombstone is not flagged.
**Tests.** Rollback replay, stale replica, interrupted write, corrupted file, two-device simultaneous edit.
**Rollback.** High-water file is local; deleting it disables the check.
**Evidence.** Pending.

---
## SSHV-003 Connection engine reliability
```yaml
id: SSHV-003
title: Auth prompts, staged timeouts, error codes, pooling, algorithm settings
module: connections
priority: P0
status: TODO
dependencies: [SSHV-004]
risk: medium
```
**Problems.** S10 keyboard-interactive; S14 timeouts; S15 forward bounds; S19 host-key algorithms; D5 error collapse;
D6 no reuse; no legacy algorithm configuration; connect not cancellable.
**Plan.** (1) UI prompt channel for keyboard-interactive, send the saved password only to prompts matching password, and
support publickey then keyboard-interactive; (2) per-phase timeouts (auth, channel, pty) and fix host-key-prompt timer
accounting; (3) extend `ApiError` codes and `SessionStatus::Error` with a code (dns, refused, timeout, auth, hostkey,
proxy, algorithm); (4) per-host connection pool; (5) optional per-host algorithm lists; (6) accept-loop backoff, tunnel cap,
SOCKS handshake timeout; (7) store one key per algorithm per host.
**Security.** No relaxed host-key checks to "make it work"; legacy algorithms opt-in per host with a visible warning.
**Acceptance.** Stalled-server test times out per phase; OTP prompt is not answered with the password; each failure kind
shows its own message.
**Tests.** Extend `ssh.rs` testutil sshd tests (confirm CI actually has sshd so they do not pass vacuously); fake stalled
TCP server; hostile-prompt server.
**Rollback.** Pooling behind a setting for one release.
**Evidence.** Pending.

---
## SSHV-026 CI and supply chain
```yaml
id: SSHV-026
title: Quality gates, dependency audit, pinned actions, component tests
module: ci
priority: P0
status: IN_PROGRESS
dependencies: []
risk: low
```
**Gaps.** S20-S22, D7-D10, D13.
**Plan.** Add `cargo fmt --check`, `cargo audit` or `cargo deny`, `pnpm audit`, `rust-toolchain.toml`, SHA-pinned actions,
Dependabot, ESLint/Prettier, jsdom component tests for the most-edited components, run or replace the Python e2e suite,
fail tagged builds when signing secrets are absent (or print a loud notice), decide macOS. Verify CI installs `sshd` so
`ssh.rs` tests run.
**Acceptance.** A deliberate unformatted file or a known-vulnerable pin fails CI.
**Rollback.** Revert workflow.
**Progress.** Added `.github/dependabot.yml` (cargo, npm, actions; weekly) and `.github/workflows/audit.yml` (`cargo audit`
and `pnpm audit --prod`, weekly and on lockfile changes, non-blocking via `continue-on-error` until findings are triaged).
Both files parse as YAML; neither has run. **Findings that changed the plan:** (1) `cargo fmt --check` reports 1679 diffs, so
the code is not in default rustfmt style; a fmt gate would fail CI at once and a mass reformat would bury real changes, so it
is deliberately not added (needs a rustfmt.toml decision or a one-off reformat commit). (2) CI already installs
`openssh-server` on Linux and Windows jobs (`build.yml:57,154`), so the sshd-based tests do run; the earlier concern that
they pass vacuously is withdrawn. **Not done:** pinning actions by SHA and `rust-toolchain.toml` (need current SHAs and a
toolchain decision), component tests, ESLint/Prettier, macOS decision, failing tagged builds without signing secrets.
**Evidence.** Pending first workflow runs.
