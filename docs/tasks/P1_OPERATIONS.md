# P1: monitoring, logs, alerts, automation

---
## SSHV-009 Monitoring gaps
```yaml
id: SSHV-009
title: Missing metrics, offline cache, bounded polling
module: monitoring
priority: P1
status: TODO
dependencies: [SSHV-003]
risk: low
```
**Existing.** Fleet poll 30 s, detail 5 s, one reused SSH connection per watched host (`monitor.rs`), opt-in `monitored`,
opt-in history (`metricsKeep`), TCP/banner reachability for 32 hosts at a time (`health.rs`).
**Gaps.** Network throughput, load, uptime, service/container health not confirmed; history does not survive lock;
nothing runs when the app is closed.
**Plan.** Extend the sampler script (read-only), keep retention opt-in, store cached history encrypted. Continuous
monitoring is *not* a desktop feature: it belongs to the optional controller (SSHV-021). Say so in the UI.
**Tests.** Sampler parser tests with captured outputs. Evidence pending.

## SSHV-010 Log explorer
```yaml
id: SSHV-010
title: Export, masking, and one entry point for journal/Docker/kube logs
module: logs
priority: P1
status: TODO
dependencies: [SSHV-004]
risk: low
```
**Existing.** `ops/logs.ts` (journald unit/priority/kernel, absolute file paths via `shq`, 5000-line buffer, ANSI strip,
level colours, filter), `ContainerLogs.svelte`, `KubeLogs.svelte`.
**Gaps.** Export, masking, historical search, unified entry. **Plan.** Mask common secret shapes (bearer tokens, `password=`,
private key blocks) in the *view and export* only; keep raw buffer in memory; add export of the filtered buffer. Bounded
buffers already exist; keep them. Tests: masking table tests. Evidence pending.

## SSHV-011 Alerts
```yaml
id: SSHV-011
title: Severity, acknowledge, persisted history, notification adapters
module: alerts
priority: P1
status: TODO
dependencies: [SSHV-005]
risk: low
```
**Existing.** `alerts.svelte.ts`: down/CPU/memory/disk, N consecutive samples, quiet hours, mute, toast + OS notification,
60 s check while open. **Gaps.** No severity, ack, persisted history (cleared on lock), adapters, tests for the store.
**Plan.** Add fields to rules and events; persist history encrypted; adapter interface with local notification as the
only built-in. Webhook/email adapters only on explicit owner approval because they send data off-device. Tests: unit tests
for the store (none today). Evidence pending.

## SSHV-015 Automation engine
```yaml
id: SSHV-015
title: Sequential/rolling mode, secret references, shared with runbooks
module: automation
priority: P1
status: TODO
dependencies: [SSHV-003]
risk: medium
```
**Existing.** `runner.rs` (8 parallel, per-run timeout, abort, 256 KiB cap), `runbookrun.rs` (parallel hosts, per-step
timeout, cancel). **Gaps.** No sequential/rolling; params are plain strings; cancel kills the local task, not the remote
process. **Plan.** Sequential/rolling option with stop-on-first-failure; secret parameters resolved in Rust from the vault
and never stored in history; cancel closes the channel so the remote process receives SIGHUP. Evidence pending.

## SSHV-016 Runbooks
```yaml
id: SSHV-016
title: Quoting default, output masking, approval, retry, rollback steps
module: runbooks
priority: P0
status: TODO
dependencies: []
risk: medium
```
**Part a (P0, small): DONE in code, TESTING.** Original plan was to quote every bare `{{name}}`. Not done: the existing tests
and docs define bare `{{name}}` as raw, and parameter values are typed by the person running the runbook, so making it an
error would break stored and scheduled runbooks for little gain. Instead (`runbook.rs` `render`): the app's own `{{host}}` and
`{{label}}`, which imports and a synced vault can fill, are now quoted unless `|raw`; a `|raw` filter was added for explicit
pass-through. Sane host names render identically. Remaining risk: a free-text parameter used bare in a scheduled or shared
runbook is still author-responsibility (S11 downgraded to Low). A lint for it is a possible follow-up.
Evidence: new test `the_apps_own_values_are_quoted_because_records_can_hold_anything`; all 22 runbook tests pass in a scratch
crate built from the real `runbook.rs` and `runbook/tests.rs`. Full crate not built; CI pending.
**Part b (P1).** Mask output in history (S12); approval policy per runbook (required on production hosts); retry policy per
step; optional rollback steps run on failure only after confirmation. **No dry-run label** unless a runbook marks steps as
read-only and the engine enforces it; otherwise show a plan preview instead.
**Tests.** Extend `runbook/tests.rs`, `runbookrun/tests.rs`. Evidence pending.

## SSHV-017 Scheduler
```yaml
id: SSHV-017
title: One-time jobs, timezone, missed-job policy, retry
module: scheduler
priority: P1
status: TODO
dependencies: [SSHV-016]
risk: low
```
**Existing.** every-N/daily/weekly, 30 s tick, 10 min grace, production gating, runs only while the app is open and the
vault unlocked. **Gaps.** One-time, timezone (uses local `Date`), per-job missed policy (skip/run-once), retry (it marks
`lastRun` before running). Document plainly in-app that nothing runs when the app is closed. Always-on scheduling belongs to
SSHV-021. Tests: extend `schedule.test.ts`. Evidence pending.
