# Architecture audit

Audited at v0.26.3 (`32be5c1`, plus the `monitor.rs` fix described in SSHV-004). Method: source inspection by area, plus
`pnpm check` and `pnpm test` run locally. **The Rust crate was not compiled or tested here**: the machine has no
webkit/glib, so `cargo check` stops at system libraries. Every Rust finding below is from reading source. Where a
statement rests on a grep rather than a full read, it says so.

## 1. Shape of the system

| Layer | Size | Notes |
|---|---|---|
| Rust backend `src-tauri/src` | ~45k lines, 54 top-level modules | Tauri 2, tokio, russh 0.63. `commands.rs` (4665 lines) is the single IPC surface. |
| Frontend `src` | 90 Svelte components, 156 TS files | Svelte 5 runes, SvelteKit (static), Tailwind. State in `src/lib/stores/*.svelte.ts`. |
| Vault | `vault/` (atomic, backup, conflicts, devices, format, integrity, legacy_v1, merge, migrate) | Folder of per-record encrypted files; sync is delegated to a file-sync tool. |
| Tests | 55 vitest files / 695 tests (pass); ~537 Rust `#[test]`; 13 Python e2e scripts (not in CI) | |
| CI | One workflow, `.github/workflows/build.yml` | Linux + Windows. No macOS. |

The prompt describes a platform to be built. The repository is already far along: v2 vault, SSH/SFTP/SCP/FTP, jump hosts,
proxies, agent, X11, Mosh, RDP/VNC, Docker/Podman, kubectl, eight database engines, monitoring, alerts, runbooks, a
scheduler, snippets, Ansible/inventory import and a self-updater. The roadmap in this audit therefore focuses on
**defects, hardening and the genuinely missing pieces**, not on rebuilding.

## 2. Request flow

```
Svelte component -> src/lib/api.ts (typed invoke wrappers) -> #[tauri::command] in commands.rs
   -> manager structs held in AppState (SshManager, MonitorManager, ContainerManager, DbManager, ...)
   -> russh / tokio / driver crates -> remote host
Events return through tauri::ipc::Channel<T> (terminal output, logs, run progress).
```

Strengths: the webview never sees secrets in list calls (`models.rs` `redacted()` plus tests); secret reads are gated by
`reveal.rs` (master-password re-entry, lockout); remote command construction uses argv + `shell_quote`
(`containers/transport.rs:34`) with name validation instead of escaping (`kube.rs`, `containers/mod.rs check_ref`).

## 3. Architectural findings

### A1. Sync `#[tauri::command]` calling `tokio::spawn` (BROKEN, fixed for three sites)
Tauri runs non-`async` commands on the main thread with no tokio context. `tokio::spawn` there panics, and the release
profile is `panic = "abort"` with `strip = true` (`Cargo.toml:123-128`), so the app exits silently. Found and fixed:
`containers/mod.rs:534` (shipped in 0.26.3), `kube.rs:399` (0.26.3), `monitor.rs:76` (this session, unreleased).
The SSH audit checked every other `tokio::spawn` site against its caller and found no further sync command reaching one.
**No test can catch this class**: Tauri command wiring is untested. See SSHV-004.

### A2. No logging, panic hook or graceful shutdown (NOT IMPLEMENTED)
No `log`/`tracing` crate in `Cargo.toml`; no `panic::set_hook`; `lib.rs:262` calls `.run(generate_context!())` with no
`RunEvent::Exit`/`ExitRequested` handler. Consequence: panics leave no trace (A1 was undiagnosable for the user), and
quitting does not release SSH sessions, the agent socket, forwards or the vault's advisory lock file.

### A3. Connection reuse (NOT IMPLEMENTED)
Every pane, SFTP session, monitor, runner, forward and runbook calls `open_client` and re-authenticates (`ssh.rs:742`,
`forward.rs:161`, `containers/transport.rs:233`, `runbookrun.rs:206`). `MonitorManager` and `SshShell` reuse within their
own scope only. Cost: N logins to a bastion-fronted host, MFA hosts unusable for fan-out features.

### A4. `commands.rs` is a 4665-line module (NEEDS REFACTORING)
Single file holds every command and the `From<SshError> for ApiError` mapping. Splitting by domain is mechanical and low
risk but should not be mixed with behaviour changes. Also large: `ssh.rs` 1968, `TerminalPane.svelte` 1675.

### A5. Error model loses information at the IPC edge (NEEDS REFACTORING)
`SshError` has good variants (`ssh.rs`), but `commands.rs:195` collapses `Timeout`, `Connect`, `JumpUnreachable` to
`"unreachable"` and `Proxy`, `KeyFile`, `NoAgent`, russh errors to `"ssh"`. Terminal panes receive only a string
(`SessionStatus::Error { message }`, `ssh.rs:305-312`). DNS failure vs refused vs unreachable is one `Connect` variant.

### A6. Two update paths (NEEDS SECURITY REVIEW)
`selfupdate.rs`/`release.rs` (GitHub Releases, checksum-verified per project memory) coexist with `tauri-plugin-updater`
(`capabilities/default.json`, injected only when signing secrets exist at build time). Verification in `selfupdate.rs`
was not re-read in this audit.

### A7. Absent subsystems
There is **no** unified resource model, topology view, discovery module, plugin system, teams/RBAC or
controller. (Grep for anthropic/openai/llm hit only unrelated Mongo code.) `inventory.rs` imports hosts from Tailscale,
AWS, GCP, kube and Terraform CLIs and has an SSH port scan; that is import, not discovery of host facts.

## 4. What the prompt assumes vs reality

| Prompt item | Reality |
|---|---|
| Vault with strong AEAD/KDF | Already XChaCha20-Poly1305 + Argon2id 64 MiB/t3/p4, key slots, recovery key, migrations. Gaps are rollback protection and VMK rotation (see SECURITY_AUDIT). |
| Multi-device sync via cloud providers | By design the app has no sync server or provider integration; a file-sync tool moves the folder. Provider adapters are not needed for E2E; a sync-status UI and rollback detection are. |
| Hosts/Docker/K8s/DB workspaces | Exist; Kubernetes is pods only. |
| Runbooks/automation | Exist; no approval, retry, rollback, dry-run or secret references. |
| AI / plugins / enterprise | Do not exist. |
