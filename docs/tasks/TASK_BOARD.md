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
| SSHV-016 | Runbooks (quoting, approval, retry, rollback) | P0 | TESTING | - | P1_OPERATIONS |
| SSHV-026 | CI gates and supply chain | P0 | TESTING (audit, dependabot, SHA-pinned actions, branch CI; toolchain pin, lint, macOS need owner) | - | P0 |
| SSHV-001 | Vault hardening (no key rotation) | P0 | TESTING (unlock delay, `subtle` done) | 004 | P0 |
| SSHV-002 | Sync rollback detection, status | P0 | TESTING (detection and status badge done; version history open) | 001 | P0 |
| SSHV-003 | Connection engine | P0 | TESTING (pooling is a design note awaiting a decision) | 004 | P0 |
| SSHV-009 | Monitoring gaps | P1 | TESTING (partial) | 003 | P1_OPERATIONS |
| SSHV-010 | Log explorer | P1 | TESTING (partial) | 004 | P1_OPERATIONS |
| SSHV-011 | Alerts | P1 | TESTING (partial) | 005 | P1_OPERATIONS |
| SSHV-015 | Automation engine | P1 | TESTING (rolling mode, secret parameters; remote cancel open) | 003 | P1_OPERATIONS |
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

## Checkpoint (2026-10-09, after release 0.28.0)

- **Merged and released:** 0.27.0 and 0.28.0 are on `main`. The owner publishes the draft releases by hand.
- **Verified:** `pnpm check` 0 errors; 746 frontend tests; the real Rust sources compiled and tested in scratch crates; CI (`Build`)
  runs on pushes to `main`, `feat/**` and `fix/**`, and was green on the branches before each merge.
- **Needs you:** try the installer (the CSP, the native hook dialog, Lock asking for the password). Decisions in
  `docs/design/*.md`: plugins, teams, controller, connection pooling. Also: macOS builds, rust-toolchain pin, ESLint/Prettier.
- **Not done, and why:** SSHV-027 (splitting `commands.rs`) and SSHV-028 (binary terminal input) are large changes that I judged
  too risky without a local full build; SSHV-022 already meets its list; version history for records (SSHV-002); structured
  logging (SSHV-004); cancel that signals the remote process (SSHV-015). AI assistant and data-key rotation were removed at the
  owner's request.
