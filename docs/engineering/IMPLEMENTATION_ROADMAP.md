# Implementation roadmap

Principles: fix confirmed defects before adding features; keep each task small enough to ship and verify alone; never
mark DONE without recorded test evidence. Tasks are defined in `docs/tasks/`. Priority P0 means a verified defect or a
security gap that is reachable today.

## Order of work

1. **Stop the bleeding (P0, small, independent)**
   SSHV-004a (done in tree, unreleased: `monitor.rs` spawn) -> SSHV-023 (CSP, hook approval) -> SSHV-024 (ProxyCommand
   injection + per-device approval) -> SSHV-025 (download name traversal, read_file cap) -> SSHV-016a (runbook `|q`
   default).
2. **Make failures visible (P0)** SSHV-004 (logging, panic hook, shutdown, spawn-context lint) so the next silent exit
   is diagnosable.
3. **Vault and sync hardening (P0)** SSHV-001 (backend auto-lock, lock vs remembered key, KDF ceiling),
   SSHV-002 (rollback detection, sync status, version history).
4. **Connection engine (P0/P1)** SSHV-003 (keyboard-interactive, staged timeouts, error codes, connection pool,
   algorithm config).
5. **CI and supply chain (P0)** SSHV-026, early because it protects everything after it: fmt, audit/deny, pinned
   actions, component tests, macOS decision.
6. **Operations gaps (P1)** SSHV-011 (alerts: severity/ack/persistence/adapters), SSHV-010 (log export + masking),
   SSHV-015/016/017 (runbook approval, retry, secret refs; scheduler one-time/timezone), SSHV-013 (Kubernetes
   resources), SSHV-014 (generic read-only DB mode, export, guard gaps), SSHV-009 (missing metrics).
7. **New models (P1/P2)** SSHV-005 resource model -> SSHV-007 discovery -> SSHV-008 topology -> SSHV-006 host
   management polish -> SSHV-022 UI restructure.
8. **Optional platform (P2/P3)** SSHV-019 plugins (needs sandbox decision), SSHV-020/021
   teams and controller (needs product decision; large).

## Explicit non-goals unless the owner decides otherwise

- Provider-specific sync backends (Dropbox/OneDrive/WebDAV adapters). The vault is a folder and is already E2E
  encrypted; adapters add attack surface and credentials without improving confidentiality. Better: sync status and
  rollback detection.
- A mandatory controller or any cloud dependency.
- An AI assistant (SSHV-018 removed) and vault data-key rotation (dropped from SSHV-001; risk S9 accepted). Both removed
  by the owner on 2026-10-09.
- Rebuilding existing modules. Navigation restructure (SSHV-022) must not delete working views.

## Decisions needed from the owner

- D11: fate of `auto.md`, `task-feat.md`, `feat*.md`.
- Lock semantics on "remembered device" (S8): clear key on lock, or keep and rename the button.
- Sync adapters: confirm the non-goal above.
- Plugin sandbox technology (WASM vs out-of-process) before SSHV-019 starts.
- Whether the scheduler may grow an optional background service (SSHV-017/021).
- macOS: ship and test it in CI, or stop claiming it.

## Verification limits on this machine

No webkit/glib, so the Tauri crate does not compile here: Rust changes are verified only by CI or by a scratch crate for
pure modules. Any task touching `commands.rs` stays at TESTING until CI compiles and passes it.
