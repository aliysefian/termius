# Host Metrics: what is done and what remains

The spec (Advanced Host Metrics & Infrastructure Management) has 12 phases. This file records where the work stands after
release 0.57.0 so the rest can be picked up without re-reading the conversation.

## Done (shipped in 0.57.0)

| Phase | What exists |
|---|---|
| 1–2 Engine | `src-tauri/src/hostmetrics/`: one read-only sampling script per reading, parser, rate/delta maths (first reading, reboot, stale, counter reset, 32-bit wrap, recreated interface), a shared `HostMetricsManager` (shared recent samples, one collection at a time per host, bounded concurrency, back-off, cancellation, in-memory history ring). The monitoring engine and the metrics share one SSH connection per host (`AppState.alert_host`). |
| 3 Window | Host Metrics window (`components/hostmetrics/`): Overview, CPU, Memory, Disks, Network (interface selector, RX/TX peaks, MB/s vs Mbps), host picker, Live Off/1/2/5 s, range 5 min / 15 min / 1 h. Opened from Host details and the Fleet page. |
| 4 Terminal | Status-bar toolbar (CPU, RAM, Disk, RX/TX) and a resizable side panel (left/right) beside the terminal. Per-host opt-in; the terminal is never rebuilt. |
| 5 Advanced | Temperatures (only a CPU's own sensor counts as the CPU's), NVIDIA (`nvidia-smi` under `timeout`) and AMD (amdgpu files) GPUs, process list with signals (host re-checks name + start time before `kill`), listening ports (tunnel from a port), logins and SSH authentication events. |
| 6 Dashboards | Customizable per-host dashboard: 12-column grid, 12 widget types, drag / keyboard / corner resize, undo-redo, hide/show, several views per widget, save per host, reusable templates applied to chosen hosts. Local to this computer. |

## Not verified when this was written

- No test ran against a real SSH host, or real GPU / temperature hardware (the development machine is a VM).
- The Rust command layer (`commands/hostmetrics.rs`, edits in `commands.rs`, `commands/alerting.rs`, `lib.rs`) could not be
  compiled on the development machine (no webkit); CI is its first real build.
- Nothing audits process signals yet.

## Remaining

### Phase 7: administrative managers

Already in the app (do not rebuild, link to them): **Services** (Ops → Services: list, start/stop/restart/reload/enable/
disable, details, logs, sudo), **Firewall** (Ops → Security → Firewall, with SSH-safety checks and temporary rules) and
**SSH keys** (Ops → Security → SSH keys). All run over `SecuritySession` + `execWithSudoFallback`
(`src/lib/security/`), which asks for a sudo password only when needed and keeps it in memory.

Missing, to build the same way (shell script + pure TypeScript parser + tests; unprivileged by default; sudo only on an
explicit "Read with sudo"):

1. **Packages**: apt, dnf, yum, zypper, pacman, apk. List installed, updates waiting, restart-required, held packages; then
   install / upgrade / upgrade all / remove, each with a confirmation naming the host and the packages, real command output,
   and partial failures reported. No rollback claims.
2. **Cron**: user and system crontabs, `/etc/cron.d`, periodic directories, systemd timers (kept separate: not
   interchangeable); then create / edit / delete with validation, a visual schedule builder, a backup before writing, and a
   check for concurrent external edits.
3. **Users**: accounts, groups, shell, status; then create, disable/enable, group changes, shell, delete. Never show hashes;
   protect system accounts and the account in use; never remove a home directory silently.
4. **SSL certificates**: certbot and acme.sh discovery, domains, issuer, expiry (30/14/7 days, expired), renewal config; then
   renew, guided issue, revoke with strong confirmation. Never change web-server configuration without permission.
5. **WireGuard**: interfaces, peers, handshakes, bytes; then up/down with a warning when the SSH session itself rides the
   interface. Never show private or preshared keys.
6. **Wiring**: a new Ops tab ("Manage": host picker + the sub-tabs above) with deep links to the existing Services and
   Firewall tabs for the same host; i18n key `ops.manage` (en, fa).

Work in progress that is **not** in the release: a branch `wip/phase7-admin` holds `src/lib/admin/` (a tested cron schedule
checker/describer, scripts for packages, cron, users, certificates and WireGuard, and parsers for packages and cron). None of
it is type-checked or wired in yet.

Firewall items from the spec that the existing manager should be checked against: treat firewalld as read-only unless
management support is verified (the existing panel currently allows deleting firewalld rules); revalidate rule identity and
ownership before a change; never retry a timed-out mutation.

### Phase 8: health checks

Unified engine: ping, TCP, HTTP(S), SSH, CPU/memory/disk/bandwidth thresholds, service status, script checks (exit code,
numeric output, bounded text). Default interval 5 min, minimum 1 min, bounded concurrency, history, alert on healthy→warning,
healthy→failed and recovery without repeats. Add the Health Checks widget to the dashboard. The Rust `monitoring/` engine
already has ping, TCP, HTTP and SSH-metrics checks: extend it, do not add a second scheduler.

### Phase 9: alerts and runbook automation

Move the metric alerts onto the new engine and the existing notification channels (Telegram, Bale, email, in-app). Metrics
missing from the Rust rules: disk I/O, temperature, GPU, certificate expiry, service failure, security events. Add
severity, evaluation windows, recovery events, deduplication, cooldown, acknowledgement and history. Runbook triggers
(metric above threshold for N minutes, health check fails/recovers, certificate nearing expiry) with deduplication, cooldown,
execution limits, loop prevention, audit history, permission validation, and never a destructive step started by a threshold
alone. Replace the old collectors (`stores/hostmetrics.svelte.ts`, the monitor window's own sampling, Rust
`monitoring/metrics.rs`'s script) with the shared engine so nothing collects the same metrics twice.

### Phase 10: other platforms

macOS (`sysctl`, `vm_stat`, `diskutil`, `ifconfig`), Windows (CIM / PowerShell), RouterOS as a separate adapter. Today any
non-Linux host is reported as unsupported and no figure is guessed.

### Phase 11: performance, storage and security pass

- **Persistent history** so the 6 h, 24 h, 7 d, 30 d and custom ranges exist (today charts reach back one hour, in memory
  only): raw 24 h, minute aggregates 7 d, longer aggregates 30 d, configurable retention and disk limit. The existing
  IndexedDB archive (`src/lib/ops/archive.ts`) is the place to start.
- Background sampling per host (10 s / 30 s / 60 s / 5 min) and a settings page for it (global and per-host: enable, interval,
  concurrency, command timeout, retention). Today only the live rate is configurable.
- Dashboard "Busiest hosts" widget (the ranking helper `busiest()` in `hostsample.ts` exists; the widget does not).
- An audit record for administrative actions, including process signals.

### Phase 12: validation

Run the Rust tests, frontend tests, type check, lint and production build in CI on all three platforms; an integration run
against a controlled Linux host (connect, discover, two samples, rates, terminal panel, health check, test alert, disconnect
and recover); real NVIDIA / AMD / sensor hardware; screenshots for the documentation.

## Known limits of what shipped

- Linux only; other systems are shown as unsupported.
- Charts reach back one hour and are kept in memory only; locking the vault clears them.
- Dashboard layouts and terminal-panel choices are stored on this computer only.
- Firewall status and Health checks dashboard widgets are not there yet (their features do not exist).
