# P1: monitoring, logs, alerts, automation

---
## SSHV-009 Monitoring gaps
```yaml
id: SSHV-009
title: Missing metrics, offline cache, bounded polling
module: monitoring
priority: P1
status: TESTING
dependencies: [SSHV-003]
risk: low
```
**Existing.** Fleet poll 30 s, detail 5 s, one reused SSH connection per watched host (`monitor.rs`), opt-in `monitored`,
opt-in history (`metricsKeep`), TCP/banner reachability for 32 hosts at a time (`health.rs`).
**Gaps (as audited).** Network throughput, load, uptime, service/container health not confirmed; history does not survive lock;
nothing runs when the app is closed.
**Verified 2026-10-09 (the audit's "not confirmed" was too cautious).** The detail sampler (`hostdetail.sh`/`hostdetail.ts`) already
reads per-interface throughput, load, uptime, processes, listening ports and interfaces, and the Ops view has a systemd service
manager. **Done now:** the fleet sampler (`METRICS_SCRIPT`) also reports failed systemd units, "not available" on hosts without
systemd, tested by running the real script on this machine and by parser tests; a new opt-in alert rule raises each failed
service once and says when none are left (SSHV-011). **Still open:** container health as an alert; history that survives a lock
(it is cleared on lock by design; `metricsKeep` already keeps longer history on this computer on request); Windows hosts.
**Plan.** Extend the sampler script (read-only), keep retention opt-in, store cached history encrypted. Continuous
monitoring is *not* a desktop feature: it belongs to the optional controller (SSHV-021). Say so in the UI.
**Tests.** Sampler parser tests with captured outputs. Evidence pending.

## SSHV-010 Log explorer
```yaml
id: SSHV-010
title: Export, masking, and one entry point for journal/Docker/kube logs
module: logs
priority: P1
status: TESTING
dependencies: [SSHV-004]
risk: low
```
**Existing.** `ops/logs.ts` (journald unit/priority/kernel, absolute file paths via `shq`, 5000-line buffer, ANSI strip,
level colours, filter), `ContainerLogs.svelte`, `KubeLogs.svelte`.
**Gaps.** Export, masking, historical search, unified entry. **Plan.** Mask common secret shapes (bearer tokens, `password=`,
private key blocks) in the *view and export* only; keep raw buffer in memory; add export of the filtered buffer. Bounded
buffers already exist; keep them. Tests: masking table tests.
**Progress (2026-10-09).** Done: (1) `ops/mask.ts`, a port of the Rust masker (same cases; 5 + 2 tests in `opsmask.test.ts`),
used by `LogBuffer` as lines arrive (per host, so a private key block split across chunks is dropped whole); a "hide secrets"
switch in the log panel, on by default, applies to new lines only. (2) "Save the lines shown" writes a `.log` file through
`export_text_file`, which now accepts `.json` and `.log` only (2 tests, scratch crate). The Docker log window (`ContainerLogs.svelte`) has the same "Hide secrets" switch, and the Kubernetes one masks through the shared buffer. **Not done:** one
entry point for journal, Docker and Kubernetes logs, historical search.
Masking cannot be undone for lines already kept; switching it off stops hiding new lines only.

## SSHV-011 Alerts
```yaml
id: SSHV-011
title: Severity, acknowledge, persisted history, notification adapters
module: alerts
priority: P1
status: TESTING
dependencies: [SSHV-005]
risk: low
```
**Existing.** `alerts.svelte.ts`: down/CPU/memory/disk, N consecutive samples, quiet hours, mute, toast + OS notification,
60 s check while open. **Gaps.** No severity, ack, persisted history (cleared on lock), adapters, tests for the store.
**Plan.** Add fields to rules and events; persist history encrypted; adapter interface with local notification as the
only built-in. Webhook/email adapters only on explicit owner approval because they send data off-device. Tests: the engine is covered by `ops.test.ts`.
**Progress (2026-10-09).** Done: severity (`info`/`warning`/`critical`: a host that stopped answering, or a reading at 97% or more,
is critical), acknowledge one or all open alerts (recoveries need none), a floor for what is announced ("everything" / "warnings
and up" / "critical only"; the list always has all), and notification adapters (`ops/notify.ts`: the in-app toast and the system
notification are adapters now; a failing adapter doesn't stop the others). The alert list can be kept between runs on this
computer (off by default, saved with the other settings, never in the vault; switching it off deletes it). 4 new tests in
`ops.test.ts`. **Not done, on purpose:** webhook, email or chat adapters, because they send host names off this computer and
need your decision first; alerts while the app is closed (needs the controller, SSHV-021); the shown list is still capped at 100.

## SSHV-015 Automation engine
```yaml
id: SSHV-015
title: Sequential/rolling mode, secret references, shared with runbooks
module: automation
priority: P1
status: TESTING
dependencies: [SSHV-003]
risk: medium
```
**Existing.** `runner.rs` (8 parallel, per-run timeout, abort, 256 KiB cap), `runbookrun.rs` (parallel hosts, per-step
timeout, cancel). **Gaps.** No sequential/rolling; params are plain strings; cancel kills the local task, not the remote
process. **Plan.** Sequential/rolling option with stop-on-first-failure; secret parameters resolved in Rust from the vault
and never stored in history; cancel closes the channel so the remote process receives SIGHUP. 
**Progress (2026-10-09).** Done: a rolling mode for "Run on hosts" (`runner.rs` `Order`): all at once (the default, unchanged), one
host at a time, or one at a time stopping at the first failure (a non-zero exit, no answer or a failed login); later hosts are
reported as skipped and never contacted. The window has a "Hosts" selector for it. Tested against a real `sshd` in the scratch
crate (event order proves nothing overlaps and the fourth host is never started); the command layer (`run_on_hosts` takes an
optional `order`; the CLI sink maps `Skipped`) was not compiled. **Runbooks (2026-10-09, later):** `runbookrun.rs` takes the same `Order` (a "Hosts" selector in the runbook view); hosts not reached are
recorded as "skipped: an earlier host failed". **Secret parameters:** a runbook parameter of kind `secret` is typed when it runs
(password field), is not kept in schedules (a schedule refuses a required one), is `[hidden]` in the record of the run, and its
value is replaced by `[hidden]` wherever it appears in step output, errors or notes; the preview shows `<name>`. It is typed, not
read from the vault: reading vault secrets from Rust is a larger change left undone. **Not done:** cancel that signals the remote
process.

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
**Part b progress (2026-10-09).** Done: (1) output masking in the kept run record (`mask.rs`, used by `runbookhistory::masked`):
private key blocks, `name=value`/`name: value` for password, token, secret, api key, authorization and similar names,
`Bearer`/`Basic` values, `user:pass@` in URLs, and well-known token prefixes; parameters with secret-looking names are hidden
too. The live view is not masked, only what is saved. It recognises common shapes and is not a guarantee. (2) Opt-in retries:
`retries` (0-5) and `retry_delay_secs` on `run`/`upload` steps, checked by `parse`; only a command that ran and failed is retried
(not a lost connection), and the note says how many tries it took. Tests: 6 in `mask.rs`, 1 in `runbookhistory.rs`, 2 in
`runbook/tests.rs`; the whole runbook suite (35 tests) passes in a scratch crate. (2026-10-09, later) **Rollback steps done:** a runbook may carry a `rollback` list (checked like the steps, with errors reported as "Rollback step N"); a run or schedule started with rollback on gives a host whose run failed (and was not cancelled) those steps afterwards, best effort, each step's own `on_error` deciding whether to go on; the host still counts as failed; results carry a `phase`; the dry run lists them marked "only on a host where the run failed, with rollback on"; the window offers the switch only for a runbook that has rollback steps, never remembers it, and names the rollback in the production confirmation; history records that it was on. 4 engine tests (39 runbook tests, scratch crate) and a browser test (`e2e/rollback.py`, 9 checks; the existing automation suite still passes). **Not done:** per-runbook approval policy
(production runs already ask for confirmation in the window; a runbook-level policy is open), rollback steps, secret-reference
parameters, a plan preview that is honest about mutation (the existing `plan` only renders text and never runs anything).
**Tests.** Extend `runbook/tests.rs`, `runbookrun/tests.rs`. Evidence: as above.

## SSHV-017 Scheduler
```yaml
id: SSHV-017
title: One-time jobs, timezone, missed-job policy, retry
module: scheduler
priority: P1
status: TESTING
dependencies: [SSHV-016]
risk: low
```
**Existing.** every-N/daily/weekly, 30 s tick, 10 min grace, production gating, runs only while the app is open and the
vault unlocked. **Gaps.** One-time, timezone (uses local `Date`), per-job missed policy (skip/run-once), retry (it marks
`lastRun` before running). Document plainly in-app that nothing runs when the app is closed. Always-on scheduling belongs to
SSHV-021. Tests: extend `schedule.test.ts`.
**Progress (2026-10-09).** Done (`schedule.ts`, `scheduler.svelte.ts`, `SchedulesPanel.svelte`): one-time schedules (switch
themselves off after starting); a time zone for daily and weekly times, read with `Intl` and tested across the New York and Berlin
daylight-saving changes, a half-hour zone (Kolkata) and the zone's own weekday, under three different computer time zones; a
missed-time policy (skip, the default and the old behavior, or run once on open); retry of failed hosts (0-3 times, every N
minutes) while the app stays open, with the notice saying so. 15 new tests (19 in the file). Old schedules load unchanged (every
new field is optional). **Not done, by design:** nothing runs while the app is closed; that is the controller (SSHV-021). The
in-app notice and the schedule screen say so plainly.
