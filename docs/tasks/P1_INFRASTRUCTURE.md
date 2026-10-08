# P1: infrastructure model, containers, databases, workspace

Each task keeps the standard sections; where a section is "n/a until started" the task has not been designed beyond what
the audit proved. Do not start any of these before the P0 items that block it.

---
## SSHV-005 Unified resource model
```yaml
id: SSHV-005
title: Shared resource model referencing vault credentials
module: model
priority: P1
status: TODO
dependencies: [SSHV-001]
risk: medium
```
**Problem.** Hosts, groups, proxies, forwards, DB connections, containers and kube contexts are separate types with no
shared identity or relationships (ARCHITECTURE A7). Some state sits outside the vault (D12).
**Existing.** `models.rs` records per type; `inventory/` import; `hostmetrics` keyed by host id.
**Expected.** A read-only *derived* graph (no new stored copy) of nodes (host, group, environment, tunnel, container,
database, kube resource) and typed edges (jumps-through, forwards-to, runs-on, uses-credential), each with stable id,
tags, environment, last-observed state and a credential *reference*.
**Plan.** Build the graph in the frontend from existing stores first; persist nothing new. Decide D12 per store.
**Files.** new `src/lib/resources/*`, stores.
**Security.** Graph carries ids, never secrets.
**Acceptance.** Graph built from a fixture vault has every jump/forward/db-tunnel edge and no invented edges.
**Tests.** Vitest on the builder with fixtures. **Rollback.** Pure addition. **Evidence.** Pending.

## SSHV-006 Host management polish
```yaml
id: SSHV-006
title: Verify host management gaps (bulk edit, import/export, search at scale)
module: hosts
priority: P1
status: TODO
dependencies: []
risk: low
```
Much exists already (groups, tags, favorites, ssh_config/Ansible/CSV/PuTTY/MobaXterm import, OpenSSH export, duplicate,
recent, virtual list, `bigfleet.py` e2e). **First step is an audit**, not code: list what the prompt asks for that is
actually absent (bulk edit and nested-folder depth are unverified). Evidence pending.

## SSHV-007 Host facts discovery
```yaml
id: SSHV-007
title: Explicit, per-host fact collection
module: discovery
priority: P1
status: TODO
dependencies: [SSHV-005, SSHV-003]
risk: medium
```
**Existing.** `monitor.rs` already samples CPU/memory/disk/processes/ports over one reused connection.
**Expected.** A "collect facts" action on one host (OS, hostname, interfaces, services, docker/kubectl presence), results
stamped with time and source, labelled *observed* vs *cached*. No network scanning (the existing `inventory_scan` stays
separate and explicit).
**Security.** Read-only fixed scripts, quoted, no sudo. **Tests.** Parser tests with captured outputs. Evidence pending.

## SSHV-008 Topology view
```yaml
id: SSHV-008
title: Interactive topology from real relationships
module: ui
priority: P1
status: TODO
dependencies: [SSHV-005]
risk: low
```
Zoom/pan/filter/search, open terminal/monitor from node. Edges only from the SSHV-005 graph; no inference. Blocked until
the graph exists. Evidence pending.

## SSHV-012 Docker gaps
```yaml
id: SSHV-012
title: Close Docker gaps found by audit
module: containers
priority: P1
status: TODO
dependencies: [SSHV-004]
risk: low
```
Docker/Podman list, actions, logs, inspect, volumes, networks, prune, pull, compose verbs and shell tab exist. Unverified:
`stats`, health column, compose `up`. Verify, then add only what is missing. Destructive actions already confirm via
`prune_preview`/`prune_run`; keep that. Tests: `containers/live_tests.rs` need a Docker host (NOT VERIFIED here).

## SSHV-013 Kubernetes workspace
```yaml
id: SSHV-013
title: Kubernetes resources beyond pods
module: kubernetes
priority: P1
status: TODO
dependencies: [SSHV-004]
risk: medium
```
**Existing.** `kube.rs`: contexts, namespaces, pods, describe, delete, logs, port-forward through `kubectl`, name
validation, hints for expired logins.
**Missing.** Deployments, services, configmaps, events, exec, RBAC-aware messaging.
**Plan.** Add read-only resource kinds through the same `kubectl -o json` path; events and exec after. Destructive verbs
need confirmation naming context+namespace. Respect kubeconfig; never read tokens into the app.
**Tests.** Extend the fake-kubectl harness. Evidence pending.

## SSHV-014 Database workspace gaps
```yaml
id: SSHV-014
title: Read-only sessions, export, safer statement guard
module: databases
priority: P1
status: TODO
dependencies: []
risk: medium
```
**Existing.** Eight engines, SSH tunnels, history (browser storage), backend `NeedsConfirmation`, production banners.
**Gaps.** Read-only only for MSSQL; no export; guard (`db/safety.rs:14-22`) misses `WITH ... DELETE`, `MERGE`, `GRANT`,
`REVOKE`, `EXPLAIN ANALYZE`, `EXEC`, and `WHERE 1=1`; IBM Db2 absent; history not in the vault.
**Plan.** Engine-level read-only (`SET TRANSACTION READ ONLY` for Postgres, `SET SESSION TRANSACTION READ ONLY` for MySQL,
equivalents); extend the guard conservatively (unknown leading keyword on a production connection => confirm); CSV/JSON
export of the loaded page with a row cap; Db2 only if the owner wants it (no pure-Rust driver known).
**Tests.** Table-driven `safety.rs` tests including every bypass above. Evidence pending.

## SSHV-022 Workspace navigation
```yaml
id: SSHV-022
title: Navigation and layout refinement without removing views
module: ui
priority: P2
status: TODO
dependencies: [SSHV-005]
risk: low
```
The rail already has hide/reorder (`railitems.ts`), palette, shortcuts, themes, i18n, saved workspaces and layouts. Do not
add the prompt's proposed tree (it lists pages that do not exist: Topology, Discovery, Alerts, Audit, AI). Re-group
only existing views; add entries only when the page exists. Accessibility: add axe checks to CI (SSHV-026).

## SSHV-027 Split large modules
```yaml
id: SSHV-027
title: Split commands.rs, ssh.rs, TerminalPane.svelte mechanically
module: refactor
priority: P2
status: TODO
dependencies: [SSHV-003]
risk: low
```
No behaviour change; one domain per commit; CI must stay green between commits.

## SSHV-028 Binary terminal input over IPC
```yaml
id: SSHV-028
title: Send terminal bytes as binary
module: terminal
priority: P2
status: TODO
dependencies: []
risk: low
```
`src/lib/ssh.ts:~148` sends `Array.from(bytes)`. Use a typed binary invoke payload. Measure large paste before and after.
