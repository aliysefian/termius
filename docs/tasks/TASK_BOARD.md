# Task board

Statuses: TODO -> IN_PROGRESS -> TESTING -> DONE. BLOCKED when a dependency or decision prevents progress.
A task is DONE only with recorded test evidence. This machine cannot compile the Tauri crate (no webkit/glib), so Rust
changes stay at TESTING until CI compiles and passes them.

| ID | Title | Pri | Status | Depends | File |
|---|---|---|---|---|---|
| SSHV-004 | Diagnosable crashes, shutdown, spawn safety | P0 | TESTING (spawn fixes + panic hook; tracing, exit handler, lint TODO) | - | P0 |
| SSHV-023 | CSP + hook approval | P0 | TESTING (needs a real-app try) | - | P0 |
| SSHV-024 | ProxyCommand quoting + per-device approval | P0 | TESTING (both parts; awaiting CI) | - | P0 |
| SSHV-025 | Download path safety, read cap | P0 | TESTING (branch fix/ssh-025-file-name-safety) | - | P0 |
| SSHV-016 | Runbooks (part a: quote built-ins, |raw) | P0 | TESTING (part a) | - | P1_OPERATIONS |
| SSHV-026 | CI gates and supply chain | P0 | IN_PROGRESS (audit workflow + dependabot added) | - | P0 |
| SSHV-001 | Vault hardening (no key rotation) | P0 | TESTING (S18 leftovers open) | 004 | P0 |
| SSHV-002 | Sync rollback detection, status | P0 | TESTING (detection done; status UI, history open) | 001 | P0 |
| SSHV-003 | Connection engine | P0 | TESTING (partial) | 004 | P0 |
| SSHV-009 | Monitoring gaps | P1 | TESTING (partial) | 003 | P1_OPERATIONS |
| SSHV-010 | Log explorer | P1 | TESTING (partial) | 004 | P1_OPERATIONS |
| SSHV-011 | Alerts | P1 | TESTING (partial) | 005 | P1_OPERATIONS |
| SSHV-015 | Automation engine | P1 | TESTING (rolling mode for commands) | 003 | P1_OPERATIONS |
| SSHV-017 | Scheduler | P1 | TESTING | 016 | P1_OPERATIONS |
| SSHV-005 | Resource model | P1 | TESTING (derived graph) | 001 | P1_INFRASTRUCTURE |
| SSHV-006 | Host management audit | P1 | TESTING (audit: all present; 5,000-host run passes) | - | P1_INFRASTRUCTURE |
| SSHV-007 | Host facts | P1 | TESTING | 005, 003 | P1_INFRASTRUCTURE |
| SSHV-008 | Topology | P1 | TESTING | 005 | P1_INFRASTRUCTURE |
| SSHV-012 | Docker gaps | P1 | TESTING (stats, health) | 004 | P1_INFRASTRUCTURE |
| SSHV-013 | Kubernetes resources | P1 | TESTING (read-only kinds; exec/scale open) | 004 | P1_INFRASTRUCTURE |
| SSHV-014 | Database gaps | P1 | TESTING (partial) | - | P1_INFRASTRUCTURE |
| SSHV-019 | Plugins | P2 | BLOCKED (sandbox decision) | 005, 023 | P2_PLUGIN |
| SSHV-022 | Navigation | P2 | TODO | 005 | P1_INFRASTRUCTURE |
| SSHV-027 | Split large modules | P2 | TODO | 003 | P1_INFRASTRUCTURE |
| SSHV-028 | Binary terminal input | P2 | TODO | - | P1_INFRASTRUCTURE |
| SSHV-020 | Teams (design) | P3 | BLOCKED (product decision) | 001, 002, 021 | P3 |
| SSHV-021 | Controller (design) | P3 | BLOCKED (product decision) | 009, 017 | P3 |

## Checkpoint (2026-10-09, end of the second session)

- **Branch:** `feat/backlog-hardening`, on top of `fix/ssh-025-file-name-safety`. Nothing is merged to `main`. No release was cut from it.
- **Last completed:** every task that can be done and checked on this machine. Statuses above are TESTING, not DONE, because **CI has
  not run on the branch**: the Tauri crate (`commands.rs`, `lib.rs`, the database drivers) cannot be compiled here, so those edits
  are unproven until CI compiles them. Open a pull request and read the `Build` run first.
- **Verified here:** `pnpm check` 0 errors; 740 frontend tests; the real source files of the vault, session, sync, SSH engine, runner,
  containers, Kubernetes and runbooks compiled and tested in scratch crates (about 360 Rust tests, including live-`sshd` ones);
  `clippy -D warnings` clean on all of it; browser tests in headless Chromium for the CSP, Topology, host facts, 5,000 hosts,
  accessibility (0 findings in three themes), Kubernetes, Ops, Databases and Runbooks.
- **Needs you:** try the installer from CI: the CSP (set `csp` back to `null` in `tauri.conf.json` if the window opens blank),
  the native "Allow this command?" dialog for hooks, Lock now asking for the password on a remembered device, the idle lock.
- **Not done, and why:** SSHV-027 (splitting `commands.rs`, a 4,700-line mechanical move that cannot be compiled here), SSHV-028
  (binary terminal input needs a new IPC signature), SSHV-022 (the existing navigation already meets the prompt's list; nothing
  to add that would not be a placeholder), connection pooling and per-host algorithm settings (SSHV-003), runbook approval policy
  and rollback steps (SSHV-016), a sync status indicator and version history (SSHV-002), structured logging (SSHV-004),
  `subtle`/unlock delay (SSHV-001), pinning actions by SHA and a rustfmt decision (SSHV-026). SSHV-019/020/021 have design
  documents in `docs/design/` and wait for owner decisions. AI assistant and data-key rotation were removed at the owner's request.
- **Next recommended:** open the pull request and fix whatever CI reports; then decide the questions in `docs/design/*.md`.
