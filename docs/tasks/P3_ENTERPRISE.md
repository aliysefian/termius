# P3: teams and optional controller

Both tasks are design-only until the owner commits to a product direction. Neither may make the desktop app depend on a
server.

---
## SSHV-020 Teams and organizations
```yaml
id: SSHV-020
title: Design shared inventory, roles, audit, approvals
module: enterprise
priority: P3
status: BLOCKED
dependencies: [SSHV-001, SSHV-002, SSHV-021]
risk: high
```
**Blocked on** product decision. Today's vault is single-owner: one VMK, anyone with the master password has everything.
Per-member access means per-member key wrapping (new key slots keyed by member) and a trust model for who may add slots;
that is a vault format change and needs the same migration discipline as v1->v2. Deliverable now: a design document with
threat model; no code. **Written 2026-10-09: `docs/design/TEAMS.md`.** Its main finding: removing a member cannot honestly be offered
without data-key rotation, which was dropped from SSHV-001 at the owner's request, so teams depend on a decision that was made the
other way. Still BLOCKED on the decisions listed there.

## SSHV-021 Optional self-hosted controller
```yaml
id: SSHV-021
title: Design always-on monitoring/scheduling service
module: controller
priority: P3
status: BLOCKED
dependencies: [SSHV-009, SSHV-017]
risk: high
```
**Blocked on** product decision. The controller is the correct home for monitoring while the desktop is closed and for
unattended schedules. Needs: enrollment, mutual TLS, least-privilege SSH credentials (probably certificate-based, short
lived), audit log, backup. Deliverable now: a design document; no code. **Written 2026-10-09: `docs/design/CONTROLLER.md`** (credentials are the hard part:
recommends a read-only monitoring account first, short-lived certificates for anything that changes hosts, and never handing the
controller the vault). Still BLOCKED on the decisions listed there. Keep SSHV-009 and SSHV-017 honest in the meantime by
stating in the UI that they run only while the app is open.
