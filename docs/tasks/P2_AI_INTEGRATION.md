# P2: AI operations assistant

---
## SSHV-018 AI operations assistant
```yaml
id: SSHV-018
title: Opt-in assistant with explain-only, suggest, and approve-to-run modes
module: ai
priority: P2
status: TODO
dependencies: [SSHV-001, SSHV-010, SSHV-016, SSHV-023]
risk: high
```
**Problem.** None exists (no provider code anywhere in the repo). The product is local-first with a stated
"nothing leaves this computer" posture for alerts, so any model call is a new data-egress path.
**Existing to reuse.** Backend-enforced confirmation pattern from `db/mod.rs` (`NeedsConfirmation`), CLI `run` approval
(`commands.rs:1484`), `reveal.rs` gate, runbook engine.
**Expected.** Provider trait with OpenAI-compatible endpoint and local runtime first (a local model keeps data on-device);
Anthropic as another implementation. Three modes only: Explain, Suggest, Execute with approval. No autonomous mode.
**Plan.** (1) Rust-side provider client so keys stay out of the webview; key stored in the vault; (2) redaction pass on
everything sent (private key blocks, passwords, tokens, vault ids); (3) a consent dialog showing exactly what leaves the
machine, per provider, remembered per session only; (4) logs and command output are wrapped as untrusted data in the prompt;
(5) suggested commands render as inert text with target host, run only via an approval modal that shows the exact command
and host and uses the existing runner path; (6) append-only local audit of prompts sent, suggestions shown, commands
approved.
**Security considerations.** Prompt injection from logs is the main threat; mitigation is structural (nothing auto-executes)
rather than prompt wording. Depends on SSHV-023 (CSP) because a webview compromise would otherwise be able to drive the
approval modal.
**Acceptance.** With the feature off, no network call to any model host (test by asserting no client is constructed);
sentinel secrets in a log never appear in the outgoing request body; a log line saying "run rm -rf" produces no execution.
**Tests.** Redaction table tests; provider mocked at the HTTP layer; approval flow test.
**Rollback.** Feature flag, default off; removal leaves no data in the vault format beyond an optional key record.
**Completion evidence.** Pending; needs owner approval of the egress model before any code.
