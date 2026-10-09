# Remaining tasks by priority (2026-10-09, after release 0.29.3)

Everything that could be built and checked has been built. "Needs you" means the next step is a decision, not code.

## P0: security and reliability
| ID | Remaining | Needs |
|---|---|---|
| SSHV-003 | Connection pooling (`docs/design/CONNECTION_POOL.md`) | Owner decision: build or "won't do" |
| SSHV-002 | Version history for records (keep N earlier revisions, encrypted) | Owner decision: changes the vault format |
| SSHV-004 | Structured logging with `tracing` (events only, never content) | Owner decision: more written to disk |
| SSHV-023 | Try the CSP and the native hook dialog in an installed build | You (manual try) |
| SSHV-001 | Windows directory flush after atomic writes (S18 leftover) | Code, needs a Windows machine to check |
| All | Items are TESTING, not DONE, until each has completion evidence recorded | Time |

## P1: infrastructure and operations
| ID | Remaining | Needs |
|---|---|---|
| SSHV-027 | Rest of the `commands.rs` split: vault lifecycle, record helpers, agent, CLI control, hosts/identities/snippets, SSH terminals, reachability/Ansible, groups/proxies, SFTP, registration (about 2,900 lines left) | Code, one area per CI round |
| SSHV-028 | Binary terminal input over IPC (new IPC signature) | Code, risky without a local full build |
| SSHV-013 | Kubernetes exec and scale (read-only kinds are done) | Code, needs your OK for write actions |
| SSHV-009/010/011/014 | Partial: see each section in `P1_OPERATIONS.md` and `P1_INFRASTRUCTURE.md` for the open parts | Code |
| SSHV-015 | Reading secret parameters from the vault in Rust (today a secret is typed at run time) | Code, larger change |
| SSHV-022 | Navigation: already meets its list | Nothing unless you want more |

## P2
| ID | Remaining | Needs |
|---|---|---|
| SSHV-019 | Plugins (`docs/design/PLUGINS.md`) | Owner decision on sandbox technology |

## P3
| ID | Remaining | Needs |
|---|---|---|
| SSHV-020 | Teams (`docs/design/TEAMS.md`) | Product decision; depends on key rotation, which was dropped |
| SSHV-021 | Optional controller (`docs/design/CONTROLLER.md`) | Product decision |

## Removed by the owner
AI assistant; vault data-key rotation.
