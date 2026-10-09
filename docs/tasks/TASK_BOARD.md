# Task board

Statuses: TODO -> IN_PROGRESS -> TESTING -> DONE. BLOCKED when a dependency or decision prevents progress.
A task is DONE only with recorded test evidence. This machine cannot compile the Tauri crate (no webkit/glib), so Rust
changes stay at TESTING until CI compiles and passes them.

| ID | Title | Pri | Status | Depends | File |
|---|---|---|---|---|---|
| SSHV-004 | Diagnosable crashes, shutdown, spawn safety | P0 | TESTING (spawn fixes + panic hook; tracing, exit handler, lint TODO) | - | P0 |
| SSHV-023 | CSP + hook approval | P0 | TODO | - | P0 |
| SSHV-024 | ProxyCommand quoting + per-device approval | P0 | TESTING (both parts; awaiting CI) | - | P0 |
| SSHV-025 | Download path safety, read cap | P0 | TESTING (branch fix/ssh-025-file-name-safety) | - | P0 |
| SSHV-016 | Runbooks (part a: quote built-ins, |raw) | P0 | TESTING (part a) | - | P1_OPERATIONS |
| SSHV-026 | CI gates and supply chain | P0 | IN_PROGRESS (audit workflow + dependabot added) | - | P0 |
| SSHV-001 | Vault hardening (no key rotation) | P0 | TESTING (S18 leftovers open) | 004 | P0 |
| SSHV-002 | Sync rollback detection, status | P0 | TESTING (detection done; status UI, history open) | 001 | P0 |
| SSHV-003 | Connection engine | P0 | TESTING (partial) | 004 | P0 |
| SSHV-009 | Monitoring gaps | P1 | TODO | 003 | P1_OPERATIONS |
| SSHV-010 | Log explorer | P1 | TESTING (partial) | 004 | P1_OPERATIONS |
| SSHV-011 | Alerts | P1 | TESTING (partial) | 005 | P1_OPERATIONS |
| SSHV-015 | Automation engine | P1 | TODO | 003 | P1_OPERATIONS |
| SSHV-017 | Scheduler | P1 | TODO | 016 | P1_OPERATIONS |
| SSHV-005 | Resource model | P1 | TODO | 001 | P1_INFRASTRUCTURE |
| SSHV-006 | Host management audit | P1 | TODO | - | P1_INFRASTRUCTURE |
| SSHV-007 | Host facts | P1 | TODO | 005, 003 | P1_INFRASTRUCTURE |
| SSHV-008 | Topology | P1 | TODO | 005 | P1_INFRASTRUCTURE |
| SSHV-012 | Docker gaps | P1 | TODO | 004 | P1_INFRASTRUCTURE |
| SSHV-013 | Kubernetes resources | P1 | TODO | 004 | P1_INFRASTRUCTURE |
| SSHV-014 | Database gaps | P1 | TESTING (partial) | - | P1_INFRASTRUCTURE |
| SSHV-019 | Plugins | P2 | BLOCKED (sandbox decision) | 005, 023 | P2_PLUGIN |
| SSHV-022 | Navigation | P2 | TODO | 005 | P1_INFRASTRUCTURE |
| SSHV-027 | Split large modules | P2 | TODO | 003 | P1_INFRASTRUCTURE |
| SSHV-028 | Binary terminal input | P2 | TODO | - | P1_INFRASTRUCTURE |
| SSHV-020 | Teams (design) | P3 | BLOCKED (product decision) | 001, 002, 021 | P3 |
| SSHV-021 | Controller (design) | P3 | BLOCKED (product decision) | 009, 017 | P3 |

## Checkpoint (2026-10-08)

- **Last completed:** repository audit and backlog (docs/engineering/*, docs/tasks/*).
- **Current:** SSHV-025 on branch `fix/ssh-025-file-name-safety`, awaiting CI. SSHV-024 part 2 not started.
- **Modified files, uncommitted:** `src-tauri/src/monitor.rs:76` (`tokio::spawn` -> `tauri::async_runtime::spawn`), `src-tauri/src/dial.rs` (ProxyCommand tokens quoted/refused, SSHV-024 part 1),
  all of `docs/engineering/` and `docs/tasks/`. `feat.md`, `feat2.md`, `feat3.md` are deleted in the working tree and were
  not touched by this work.
- **Tests:** `pnpm check` 0 errors; `pnpm test` 55 files / 695 tests passed (run by an audit agent). Rust not compiled
  or tested here (no webkit/glib).
- **Known problems:** see SECURITY_AUDIT S1-S22.
- **Next recommended:** SSHV-023 (CSP) is highest impact but needs a runnable build to verify; SSHV-025 and SSHV-024 are
  testable as pure Rust unit tests and can be developed in a scratch crate. Release the `monitor.rs` fix as 0.26.4.
