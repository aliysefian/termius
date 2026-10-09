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
status: TESTING
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

**Progress (2026-10-09).** Built as a derived, read-only graph (`src/lib/resources/graph.ts`), nothing new stored: hosts, proxies, tunnel
rules and databases become nodes with a stable id, label, environment, tags, group and address; the only edges are links the data
holds (a host's jump host after group defaults, the proxy a host or the start of its jump chain uses, a tunnel rule and its host,
a database and the SSH host it is reached through). A reference to something that no longer exists makes no line and no dangling
end; a jump loop cannot hang it. Credentials appear nowhere in it. Tests: `graph.test.ts` (6). **Not done:** containers and
Kubernetes resources as nodes (they exist only while a source is open, not in the vault), last observed state beyond a host's
reachability dot, the decision on where alert history, query history and runbook history should live (D12).


## SSHV-006 Host management polish
```yaml
id: SSHV-006
title: Verify host management gaps (bulk edit, import/export, search at scale)
module: hosts
priority: P1
status: TESTING
dependencies: []
risk: low
```
Much exists already (groups, tags, favorites, ssh_config/Ansible/CSV/PuTTY/MobaXterm import, OpenSSH export, duplicate,
recent, virtual list, `bigfleet.py` e2e). **First step is an audit**, not code: list what the prompt asks for that is
actually absent (bulk edit and nested-folder depth are unverified). Evidence pending.

**Audit result (2026-10-09).** Checked each item in the prompt against the code and, for scale, ran the browser test: grouping and
nested folders (group path, `GroupRow`/`HostTreeNode`), tags, favorites, quick search and the command palette, bulk editing
(`BulkEditForm.svelte`, a multi-select bar in the host list), import from `~/.ssh/config`, Ansible, CSV, PuTTY and MobaXterm,
OpenSSH config export, host duplication, jump hosts and proxies per host or per group, environment labels with production
markers, recent hosts, and a virtualized list. **All exist.** Scale: `e2e/bigfleet.py` with 5,000 hosts draws 23 rows, keeps the
scrollbar for the whole list, End/Home/type-ahead/search/fold all work, search takes 445 ms, heap 87 MB, no page errors (run
here, headless Chromium, mocked backend). **No code change was needed**; the earlier "unverified" note was caution, not a gap.
Open questions that are not defects: connection "profiles" as a named bundle apart from host defaults and group defaults (groups
already carry shared login, jump host and proxy), and drag-and-drop between nested groups on very large lists (works, not
measured).


## SSHV-007 Host facts discovery
```yaml
id: SSHV-007
title: Explicit, per-host fact collection
module: discovery
priority: P1
status: TESTING
dependencies: [SSHV-005, SSHV-003]
risk: medium
```
**Existing.** `monitor.rs` already samples CPU/memory/disk/processes/ports over one reused connection.
**Expected.** A "collect facts" action on one host (OS, hostname, interfaces, services, docker/kubectl presence), results
stamped with time and source, labelled *observed* vs *cached*. No network scanning (the existing `inventory_scan` stays
separate and explicit).
**Security.** Read-only fixed scripts, quoted, no sudo. **Tests.** Parser tests with captured outputs. Evidence pending.

**Progress (2026-10-09).** Done: a "System" tab in the host monitor window with a "Collect facts" button. Nothing is collected until it
is pressed (checked: zero calls before the click). It runs one fixed, read-only script (`hostfacts.ts`: hostname, OS, kernel,
architecture, CPU model and cores, memory and swap, uptime, mounted disks with use, systemd or container, virtualization, and the
version of docker, podman, nerdctl, kubectl, helm, git, python3 and node where present) and shows the answer stamped "Observed
<time>. These are not live", with Copy as text. No scanning of any network, no sudo, nothing written or installed (a test fails if
the script ever contains a write to a file, a network tool or a package manager). The real script was run on this machine and its
output parsed; the browser test (`e2e/hostfacts.py`) opens it from the Topology page and checks the result. **Not done:**
Windows hosts, listening ports and services as facts (the Ports tab and the service manager already show those live), keeping
collected facts between runs.

## SSHV-008 Topology view
```yaml
id: SSHV-008
title: Interactive topology from real relationships
module: ui
priority: P1
status: TESTING
dependencies: [SSHV-005]
risk: low
```
Zoom/pan/filter/search, open terminal/monitor from node. Edges only from the SSHV-005 graph; no inference. Blocked until
the graph exists. Evidence pending.

**Progress (2026-10-09).** Done: a Topology page (Manage menu and command palette) that draws the SSHV-005 graph: columns left to
right from where traffic enters (proxy, jump hosts, host, then the tunnels and databases that ride it), pan, zoom and fit, search
and environment filter (a match is shown with its neighbours), hosts with no links hidden unless asked for (with a count), a
reachability dot on each host from the last check, a details panel with the links in words and "Open a terminal" and "Monitor"
for a host, and the same links listed as text for screen readers. Checked in headless Chromium against the built app with a
mocked fleet (`e2e/topology.py`: 7 items, 6 links, the search and the actions); translations of the menu entry in all eight
languages. **Not done:** drawing containers and Kubernetes (see SSHV-005), live health beyond the last reachability check,
dragging nodes, saving a layout, very large graphs (everything is drawn; with hundreds of linked hosts it needs clustering).


## SSHV-012 Docker gaps
```yaml
id: SSHV-012
title: Close Docker gaps found by audit
module: containers
priority: P1
status: TESTING
dependencies: [SSHV-004]
risk: low
```
Docker/Podman list, actions, logs, inspect, volumes, networks, prune, pull, compose verbs and shell tab exist. Unverified:
`stats`, health column, compose `up`. Verify, then add only what is missing. Destructive actions already confirm via
`prune_preview`/`prune_run`; keep that. Tests: `containers/live_tests.rs` need a Docker host (NOT VERIFIED here).

**Progress (2026-10-09).** Checked against the code: stats, a health indicator and Compose `up` were the real gaps. Done: (1) a health
badge on each container, read from the runtime's status text ("(healthy)", "(unhealthy)", "(health: starting)"), no new command;
(2) a "CPU & memory" switch that adds one `stats --no-stream --format '{{json .}}'` reading to each refresh and shows CPU, memory
(used and percent) and, on hover, network, disk and process count; a failure shows nothing rather than breaking the list. Tests:
2 parser/command tests in `containers/parse.rs` (scratch crate with the real `containers/`), 1 in `containerdata.test.ts`; the
`containers_stats` command in `commands.rs` was not compiled. Not tried against a real Docker or Podman (none here): the Docker
field names are as documented; Podman's `{{json .}}` form is accepted by the parser but unconfirmed. **Not done:** Compose `up`
(needs the project's file path, which the runtime does not always know; stop, start, restart and down exist), health as an alert.

## SSHV-013 Kubernetes workspace
```yaml
id: SSHV-013
title: Kubernetes resources beyond pods
module: kubernetes
priority: P1
status: TESTING
dependencies: [SSHV-004]
risk: medium
```
**Existing.** `kube.rs`: contexts, namespaces, pods, describe, delete, logs, port-forward through `kubectl`, name
validation, hints for expired logins.
**Missing.** Deployments, services, configmaps, events, exec, RBAC-aware messaging.
**Plan.** Add read-only resource kinds through the same `kubectl -o json` path; events and exec after. Destructive verbs
need confirmation naming context+namespace. Respect kubeconfig; never read tokens into the app.
**Progress (2026-10-09).** Done: a "Show" selector in the Kubernetes view lists Deployments, StatefulSets, DaemonSets,
Services, ConfigMaps, Jobs, CronJobs, Ingresses, Events and Nodes with the columns that matter for each (ready counts, service
type and ports, job state, event reason/object/message newest first and cut at 300 characters, node roles and version), searchable,
with Describe. Read-only on purpose. **Secrets are not in the list of kinds and cannot be asked for** (a test checks the
backend refuses the name); ConfigMaps list key names and a count, never values (Describe, on request, shows them). RBAC: a
refusal from the cluster comes back as `kubectl`'s own words plus the existing hint ("this account isn't allowed to do that in that
namespace"); nothing tries another way. Tests: 5 new in `kube.rs` (run in the scratch crate with the real `containers/` and
`kube.rs`: 204 pass), 3 in `kubedata.test.ts`; the Rust and TypeScript kind names are checked against the same list. The two new
commands in `commands.rs` were not compiled. **Not done:** exec (the Pods view already opens a shell in a terminal tab), scale and
rollout (they change the cluster and need their own confirmation design), editing.
**Tests.** Extend the fake-kubectl harness. Evidence: as above.

## SSHV-014 Database workspace gaps
```yaml
id: SSHV-014
title: Read-only sessions, export, safer statement guard
module: databases
priority: P1
status: TESTING
dependencies: []
risk: medium
```
**Existing.** Eight engines, SSH tunnels, history (browser storage), backend `NeedsConfirmation`, production banners.
**Gaps.** Read-only only for MSSQL; no export; guard (`db/safety.rs:14-22`) misses `WITH ... DELETE`, `MERGE`, `GRANT`,
`REVOKE`, `EXPLAIN ANALYZE`, `EXEC`, and `WHERE 1=1`; IBM Db2 absent; history not in the vault.
**Plan.** Engine-level read-only (`SET TRANSACTION READ ONLY` for Postgres, `SET SESSION TRANSACTION READ ONLY` for MySQL,
equivalents); extend the guard conservatively (unknown leading keyword on a production connection => confirm); CSV/JSON
export of the loaded page with a row cap; Db2 only if the owner wants it (no pure-Rust driver known).
**Progress (2026-10-09).** Correction: result export already existed (`DatabasesView.svelte` Export menu: copy as CSV/TSV/JSON,
save as CSV/JSON through `db_save_export`, with spreadsheet-formula defusing in `dbdata.ts`); the audit agent's "no export" was
wrong. Done: (1) the statement guard (`db/safety.rs`) now also flags `WHERE 1=1`/`WHERE true`/`WHERE 'a'='a'`, data-changing
`WITH`, `MERGE`, `REPLACE`, `GRANT`/`REVOKE`, `EXEC`/`EXECUTE`/`CALL`, and `EXPLAIN ANALYZE <statement>` (which really runs it);
plain reads and `EXPLAIN` without `ANALYZE` stay quiet. 10 tests pass in a scratch crate. (2) A "Read-only sessions" connection
setting for MySQL (`SET SESSION TRANSACTION READ ONLY` on every pooled connection), PostgreSQL (`default_transaction_read_only`)
and SQL Server (it was read by the backend but never offered in the form); it is a guard against accidents, not a boundary.
`dbengines.test.ts` covers the setting; the Rust driver edits (`pg.rs`, `mysql.rs`, `mssql.rs`, `ConnectSpec::read_only`) were
not compiled (drivers need the full crate) but use APIs checked against the vendored sources (`Config::options`,
`OptsBuilder::init`). **Not done:** Db2 (no pure-Rust driver known), moving query history into the vault.
**Tests.** Table-driven `safety.rs` tests including every bypass above. Evidence: as above.

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
**Progress (2026-10-09).** First domain moved: runbooks, to `src-tauri/src/commands/runbooks.rs` (11 commands; `lib.rs` names them by the new path). CI green. Second step: updates, key manager, local terminals, RDP, telnet/serial, run-on-hosts, known hosts, databases, containers, detail monitoring, ssh-config import and port forwarding moved to `commands/*.rs` (`commands.rs` went from 4,700 to about 2,900 lines). Still in `commands.rs`: vault lifecycle, record helpers, agent, CLI control, hosts/identities/snippets, SSH terminal sessions, reachability/Ansible, groups/proxies, SFTP and registration.

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
