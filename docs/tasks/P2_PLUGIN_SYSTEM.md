# P2: plugin system

---
## SSHV-019 Plugin architecture
```yaml
id: SSHV-019
title: Evaluate sandboxing, then a permissioned plugin API
module: plugins
priority: P2
status: BLOCKED
dependencies: [SSHV-005, SSHV-023]
risk: high
```
**Blocked on:** owner decision on sandbox technology (see IMPLEMENTATION_ROADMAP, decisions).
**Problem.** No extension mechanism exists. Third-party code in a process that holds an unlocked vault and SSH sessions is
the highest-risk feature in this backlog.
**Plan.** Phase 0 (design doc only): compare WASM (wasmtime, capability-based, no ambient authority) against out-of-process
plugins over a local socket with an explicit permission manifest. Phase 1: a *declarative* plugin type with no code
(inventory sources, notification webhooks, snippet packs) which the app already half-supports via `snippetpacks.ts` and
`inventory.rs`. Phase 2: executable plugins only through the chosen sandbox, with a host API that exposes resource ids and
never credentials; install requires signature or explicit local-file trust prompt.
**Acceptance (phase 1).** A plugin cannot read the vault or open a socket it did not declare.
**Tests.** Permission-denied tests per capability. **Rollback.** Uninstall removes all plugin state. Evidence pending.

**Progress (2026-10-09).** Design written: `docs/design/PLUGINS.md` (declarative first, sandboxed WebAssembly only if needed,
native code not built; host API, permissions, install flow, decisions). No code, by design. Stays BLOCKED on the three decisions
listed there.
