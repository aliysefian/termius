# Feature inventory

Status words: **IMPLEMENTED**, **PARTIAL**, **BROKEN**, **MISSING** (not implemented), **REVIEW** (needs security
review), **REFACTOR**. Evidence is a file path (line where opened) or a named test. "Grep" means absence was established
by search, not by reading every file. Nothing here was exercised at runtime except `pnpm check` and `pnpm test`.

## Vault and sync
| Capability | Status | Evidence / gap |
|---|---|---|
| AEAD + KDF | IMPLEMENTED | `crypto.rs:183-235`, `:80-89`; `vault/format.rs:142` slot AAD |
| Tamper rejection | IMPLEMENTED | `vault/mod.rs:659-678`; test `tampered_or_transplanted_records_fail_authentication` |
| Atomic writes | IMPLEMENTED | `vault/atomic.rs:85-144`; dir fsync best-effort, no-op on Windows |
| Backup / restore | IMPLEMENTED | `vault/backup.rs`; tests `backup_restore_verifies_first_and_keeps_a_safety_copy` |
| Password change / recovery key | IMPLEMENTED | `vault/mod.rs:581`; test `interrupted_password_change_keeps_the_old_password` |
| VMK (data key) rotation | NOT PLANNED (owner decision) | password/recovery change rewraps only the slot; risk S9 accepted |
| Versioned format + migration | IMPLEMENTED | `format.rs:184-193`, `migrate.rs`; tests `v1_vaults_migrate_...`, `interrupted_migration_rolls_back_...` |
| Conflict detection / 3-way merge | IMPLEMENTED | `vault/merge.rs`, `conflicts.rs`; test `concurrent_edits_merge_or_conflict_never_silently_overwrite` |
| Rollback / replay protection | MISSING | no per-device high-water mark, no signed manifest |
| Version history UI | PARTIAL | one `prev` per record; timestamped backups only |
| Sync status UI | PARTIAL | conflict counts only; no syncing/last-synced/error state (grep) |
| Backend auto-lock | MISSING | timer is in the webview (`+page.svelte:126-144`), default off (`settings.svelte.ts:168`) |
| Lock clears remembered key | MISSING | `unlock_with_device` reopens without password after lock |
| Unlock attempt lockout | MISSING | only Argon2 cost; reveal gate has lockout (`reveal.rs`) |
| Structured logging / redaction | MISSING | no logging crate |
| Panic hook / crash report | MISSING | |
| Graceful shutdown | MISSING | no `RunEvent` handler |

## Connections
| Capability | Status | Evidence / gap |
|---|---|---|
| Password, key, certificate, agent auth | IMPLEMENTED | `ssh.rs:984, 1033-1048, 1093-1170` |
| Keyboard-interactive / 2FA | BROKEN | answers every prompt with the password, `ssh.rs:1068-1090`; key+2FA not supported |
| ProxyJump chains | IMPLEMENTED | `ssh.rs:742-785`, max 8 hops; tests: one and two hop |
| SOCKS5 / HTTP CONNECT | IMPLEMENTED | `dial.rs:34-218`; 5 tests |
| ProxyCommand | REVIEW | raw token substitution `dial.rs:245`; approval flag synced and client-set (`models.rs:432`, `commands.rs:3285`) |
| Host-key verification | IMPLEMENTED | `ssh.rs:582-633`; no production auto-accept path found; tests: unknown/changed key prompts |
| Host-key algorithm tolerance | PARTIAL | one key per host:port; CA/cert principals not checked (`hostkeys.rs:44-56`, `ssh.rs:589`) |
| Legacy cipher/KEX configuration | MISSING | no algorithm preferences anywhere |
| Error classification | PARTIAL | typed in Rust, collapsed at IPC (`commands.rs:195`); DNS/refused merged |
| Timeouts | PARTIAL | connect 20 s only; auth/channel/pty have none (`ssh.rs:900-940`) |
| Keepalive | IMPLEMENTED | 30 s, max 3 (`ssh.rs:33, 886`) |
| Reconnect | PARTIAL | frontend only, panes only (`TerminalPane.svelte:626`) |
| Connection reuse | MISSING | see ARCHITECTURE_AUDIT A3 |
| Cancel while connecting | PARTIAL | not interruptible during dial/auth |
| Port forwarding L/R/D | IMPLEMENTED | `forward.rs`; default bind 127.0.0.1; no cap, accept-error busy loop |
| SFTP / SCP / FTP(S) | IMPLEMENTED | `sftp.rs`, `files/*`; download name traversal REVIEW |
| X11, agent forwarding | IMPLEMENTED | gated per host (`ssh.rs:638-698`) |
| Mosh, serial, telnet, RDP, VNC | IMPLEMENTED (Mosh PARTIAL) | `mosh.rs`, `rdp/`, `vnc/`; only skimmed |
| Connection diagnostics | PARTIAL | `health.rs` TCP+banner; no staged DNS/TCP/KEX/auth trace |

## Operations
| Capability | Status | Evidence / gap |
|---|---|---|
| Monitoring (CPU/mem/disk/proc/ports) | PARTIAL | `monitor.rs`, `hostmetrics.svelte.ts` (30 s fleet, 5 s detail); no network/load/uptime/service/container health confirmed; no polling when closed |
| Log explorer | PARTIAL | `ops/logs.ts` (journald + files, 5000-line buffer); no export, masking, history search; Docker/kube logs separate |
| Alerts | PARTIAL | `alerts.svelte.ts`: down/CPU/mem/disk, quiet hours; no severity, ack, persisted history, adapters |
| Docker / Podman | IMPLEMENTED | `containers/`; stats and health not confirmed |
| Kubernetes | PARTIAL | `kube.rs`: contexts, namespaces, pods, logs, port-forward. No deployments/services/events/exec |
| Databases | IMPLEMENTED | MySQL, Postgres, MSSQL, Oracle, rqlite, Redis, Mongo, Elastic; tunnel, history, destructive guard (`db/safety.rs`). Export exists (corrected). On branch: read-only setting for MySQL/Postgres/SQL Server and a tighter guard. Gaps: Db2 not present; originally the guard missed `WITH..DELETE`, `MERGE`, `GRANT`, `EXEC` |
| Multi-host command runner | IMPLEMENTED | `runner.rs` (8 parallel, 256 KiB cap, abort); no sequential mode |
| Runbooks | PARTIAL | steps/params/when/on_error/history. No approval, retry, rollback, preview, secret refs; `{{x}}` unquoted by default (`runbook.rs:367`) |
| Scheduler | PARTIAL | every-N/daily/weekly while app open; no one-time, no timezone, misses skipped, no retry (`scheduler.svelte.ts`) |
| Hooks | REVIEW | `run_hook` does not itself verify approval (`commands.rs:~2692`) |

## Platform
| Capability | Status |
|---|---|
| Unified resource model, topology, discovery | MISSING |
| AI assistant | NOT PLANNED (owner decision) |
| Plugin system | MISSING |
| Teams / RBAC / controller | MISSING |
| Command palette, shortcuts, themes, i18n, layouts, virtualized lists | IMPLEMENTED (`shortcuts.ts`, `themes.ts`, `i18n/`, `layout.ts`, `virtuallist.ts`) |
| CSP | MISSING (`tauri.conf.json:21` `"csp": null`) |
| macOS CI/build | MISSING in CI (README/roadmap claim a universal macOS build; `build.yml` has no macOS job) |
