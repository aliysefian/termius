# DevOps gaps: what Portainer has and SSHVault doesn't

A comparison of SSHVault 0.30 with [Portainer](https://www.portainer.io) (the container management UI for Docker,
Swarm, Kubernetes and Podman), written as a backlog. It lists only what Portainer does that SSHVault doesn't yet.
It is not a plan to copy Portainer: SSHVault is a desktop app with no server, no agent and nothing installed on the
hosts, and every item below should keep that.

Effort: **S** a day or two, **M** about a week, **L** several weeks. Priority: **P1** most useful for people who run
containers over SSH, **P2** useful, **P3** only if there is demand.

## What SSHVault already covers

So the list below isn't read as "nothing exists": containers and images on Docker, Podman and nerdctl (list, start,
stop, restart, remove, logs, inspect, shell, stats), volumes and networks (list, remove unused), image pull, Compose
projects as a whole (start, stop, restart, take down), Kubernetes pods, logs, describe, shell, port-forward and
read-only resource lists, production guards, host and fleet monitoring, systemd services, log following on many
hosts, runbooks with schedules, and databases. Everything goes through the runtime's own command line over SSH.

## 1. Creating and changing things (SSHVault mostly reads and controls)

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 1.1 | **Create a container** from a form: image, name, ports, volumes, environment, restart policy, network, resource limits, labels, command | Yes | Can only start existing ones | M | P1 |
| 1.2 | **Duplicate / edit a container** (recreate with changed settings) | Yes | No | M | P2 |
| 1.3 | **Deploy a Compose stack** from pasted or uploaded YAML (with `.env`), update it, redeploy | Yes (Stacks) | Only acts on projects that already exist | M | P1 |
| 1.4 | **Edit a stack's file** in the app, with validation (`docker compose config`) before it is applied | Yes | No | S | P2 |
| 1.5 | **App templates**: one-click deployment of common apps (Nginx, Postgres, Redis, Traefik…), including your own templates, stored in the vault | Yes | Snippet packs only | M | P2 |
| 1.6 | **Create volumes and networks** (driver, subnet, labels), and **attach / detach** a container to a network | Yes | Lists and removes only | S | P2 |
| 1.7 | **Build an image** from a Dockerfile or a Git repository | Yes | No | M | P3 |
| 1.8 | **Kubernetes: apply a manifest**, scale a deployment, restart a rollout, edit a ConfigMap | Yes | Read-only lists; pods can be deleted | M | P2 |
| 1.9 | **Kubernetes: deploy a Helm chart** from a repository with values | Yes | No | L | P3 |
| 1.10 | **Swarm services**: create, scale, update, roll back, drain a node | Yes | No Swarm support | L | P3 |

## 2. Registries and images

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 2.1 | **Registry credentials**: Docker Hub, GHCR, GitLab, ECR, ACR, private registries, kept encrypted in the vault and used for pulls on any host (`docker login` on the host, never written to disk) | Yes | Pulls public images only | M | P1 |
| 2.2 | **Browse a registry**: repositories, tags, sizes; pick a tag to deploy | Yes | No | M | P2 |
| 2.3 | **Image history and layers**, and "which containers use this image" | Yes | Count of containers only | S | P3 |
| 2.4 | **Vulnerability scan** of an image (Trivy or Grype if present on the host, run on demand) | Business edition | No | M | P3 |
| 2.5 | **Tag, push, export (save) and import (load)** an image | Yes | No | S | P3 |
| 2.6 | **Image update check**: is there a newer digest for this tag? | Business edition | No | M | P2 |

## 3. Observability

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 3.1 | **Live container stats charts** (CPU, memory, network, block I/O) with history | Yes | A CPU and memory column; no charts | S | P1 |
| 3.2 | **Events stream** (`docker events`: start, die, OOM-kill, health change) with a filter and alerts | Yes | No | S | P1 |
| 3.3 | **Container health and restart-loop alerts** through the existing Alerts (a container that keeps restarting, or turns unhealthy) | Partly | Alerts watch hosts and systemd services only | S | P1 |
| 3.4 | **Logs across several containers** at once, merged and tagged, as Ops → Logs does for hosts | Partly | One container's log per window | S | P2 |
| 3.5 | **Kubernetes events and node pressure** surfaced as findings | Yes | Events are listed, not highlighted | S | P2 |
| 3.6 | **Dashboard**: one page with counts (running, stopped, unhealthy), disk used by images and volumes, and what needs attention, across every opened host | Yes | Per-source lists; Fleet covers hosts, not containers | M | P1 |
| 3.7 | **Disk usage report** (`docker system df`) with a breakdown and a guided clean-up | Yes | "Remove unused" per type | S | P2 |

## 4. Many environments at once

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 4.1 | **A container view across all hosts**: every container on every host in one searchable list ("where is `shop-api` running?") | Yes (Environments) | One source at a time | M | P1 |
| 4.2 | **Container Fleet tiles**: per-host container counts and health on the Fleet page | Yes | Host metrics only | S | P2 |
| 4.3 | **Compare / promote**: show the image and settings of one service on staging versus production | Business edition | No | M | P3 |
| 4.4 | **Environment tags and groups** shared with the host groups, to act on "all Production hosts" | Yes | Hosts have groups; the container view doesn't use them | S | P2 |
| 4.5 | **Remote Docker over TLS or a socket tunnel** without SSH (the Docker API) | Yes | SSH only | M | P3 |

## 5. Automation and change control

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 5.1 | **Runbook steps for containers**: `pull`, `up`, `restart`, `health-wait`, `rollback to previous image`, with the runbook's dry run and production confirmation | Webhooks, edge jobs | Runbooks can run any command, with no container-aware steps | M | P1 |
| 5.2 | **GitOps for stacks**: keep a Compose repo in sync, redeploy on a new commit (while the app is open) | Yes (Git stacks) | No | L | P3 |
| 5.3 | **Webhook redeploy** (a CI job calls a URL to pull and recreate a service) | Yes | Needs an always-on process; see `docs/design/CONTROLLER.md` | L | P3 |
| 5.4 | **Auto-update containers** to a newer image on a schedule, with a rollback if the health check fails | Business edition | No | M | P3 |
| 5.5 | **Rollback**: keep the previous image and settings of a recreated container, and restore them in one click | Partly | No | M | P2 |
| 5.6 | **Scheduled jobs** (prune, backup a volume, restart) as container tasks | Yes (Edge jobs) | Runbook schedules exist; not container-aware | S | P2 |
| 5.7 | **Volume backup and restore**: archive a volume to a local file or an S3-compatible bucket, and restore it | Yes (backup of Portainer itself) | No | M | P2 |

## 6. Files and debugging

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 6.1 | **Browse and edit files inside a container or volume**, and copy files in and out (`docker cp`) | Yes | Files view covers hosts, not containers | M | P1 |
| 6.2 | **Run a one-off command** in a container and see its output (without opening a shell tab) | Yes | Shell only | S | P2 |
| 6.3 | **Attach** to a container's main process, and a **debug shell** for containers that have no shell (an ephemeral debug container, or `kubectl debug`) | Yes | `exec` into sh/bash only | S | P2 |
| 6.4 | **Port mapping view**: which host ports are used by which containers, conflicts highlighted | Partly | Ports column per container | S | P3 |
| 6.5 | **Inspect with a readable view** (environment, mounts, networks, health, restart policy, limits) rather than raw JSON | Yes | Raw JSON with find | S | P2 |

## 7. Access control and teams

SSHVault is single-user by design (no accounts, no server). Portainer's main business is the opposite, so these are
listed for completeness and mostly belong in `docs/design/TEAMS.md`.

| # | Feature | Portainer | SSHVault today | Priority |
|---|---|---|---|---|
| 7.1 | Users, teams and role-based access control per environment | Yes | No (one vault, one password) | P3 |
| 7.2 | SSO (OAuth, LDAP, SAML) | Yes | No | P3 |
| 7.3 | Audit log of who did what | Yes | A local connection log; runbook history | P3 |
| 7.4 | Read-only roles / "operator" role for production | Yes | Production prompts for everyone | P3 |
| 7.5 | **A middle path that fits SSHVault**: a *read-only vault mode* (open a shared vault that can't be changed, and can't run `exec`, stop or delete on production hosts) | n/a | No | P2 |

## 8. Security posture

| # | Feature | Portainer | SSHVault today | Effort | Priority |
|---|---|---|---|---|---|
| 8.1 | **Container hardening check**: privileged containers, host network, mounted Docker socket, containers running as root, no resource limits, `latest` tags, shown in the Security review | Partly | Security review covers keys, certificates and passwords | S | P1 |
| 8.2 | **Secrets and configs** for Swarm / Kubernetes (create, list names only, never show values) | Yes | Kubernetes Secrets are deliberately never listed | S | P3 |
| 8.3 | **Docker socket exposure** warning: a host whose Docker API listens on TCP without TLS | No | No | S | P2 |

## Suggested order

1. **Create and deploy** (1.1, 1.3, 1.4) together with **registry credentials** (2.1). Without them SSHVault can
   only manage containers somebody else created, which is the biggest gap against Portainer.
2. **See everything** (3.6 dashboard, 4.1 all-hosts view, 3.1 stats charts, 3.2 events, 3.3 restart-loop alerts).
   All of it is read-only and reuses the connections already open.
3. **Automate** (5.1 container runbook steps, 5.5 rollback). Runbooks already have dry runs, production typing
   confirmation and schedules, so container-aware steps get those for free.
4. **Debug** (6.1 files in containers, 6.2 one-off commands, 6.5 readable inspect).
5. **Harden** (8.1 hardening check in the Security review).
6. Everything marked P3 only if people ask: Swarm, Helm, GitOps, webhooks and the multi-user items change what the
   app is, and belong with the always-on controller design in `docs/design/CONTROLLER.md`.

## Constraints that apply to every item

- **Nothing installed on the host.** Use the runtime's own CLI over the existing SSH connection, as the Containers
  view does now. Anything that needs an agent or a listening service is out.
- **No secrets on disk.** Registry credentials and `.env` values live in the vault; `docker login` output and
  environment values are masked in logs and in the saved run history, as runbooks already do.
- **Production asks first.** Create, recreate, redeploy and delete on a Production host ask for the host's name.
  Anything that replaces a running container shows the exact command first (a dry run).
- **Quote every argument.** Names, images and paths reach the host as separate arguments, never inside a shell
  string, as in `containers/` and `kube.rs`.
- **Tests without a real daemon.** Follow the fake-CLI approach used for `kube.rs` and `containers/*`, so each
  feature has tests that run in CI.
