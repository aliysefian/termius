# Technical debt

Ordered by cost of leaving it. Evidence paths as in the other audit files.

| # | Item | Why it hurts | Fix | Task |
|---|---|---|---|---|
| D1 | No logging, panic hook or shutdown handler | A silent abort (the 0.26.3 bug) was undiagnosable; sessions/locks leak on quit | `tracing` + redaction layer, `panic::set_hook` writing to the app-data dir, `RunEvent::Exit` cleanup | SSHV-004 |
| D2 | Sync-command/runtime-context bug class is untestable | Command wiring has no tests; only runtime catches it | Lint/test that no sync `#[tauri::command]` reaches `tokio::spawn`; prefer `async` commands | SSHV-004 |
| D3 | `commands.rs` 4665 lines | Review and merge pain; error mapping lives in it | Split by domain behind the same `invoke_handler` | SSHV-027 |
| D4 | `ssh.rs` 1968 lines, `TerminalPane.svelte` 1675 | Same | Split connect/auth/channels; extract reconnect, search, profile logic | SSHV-027 |
| D5 | Error codes collapse at IPC | UI cannot branch on timeout/refused/DNS/proxy | Extend `ApiError` codes; carry code in `SessionStatus::Error` | SSHV-003 |
| D6 | One SSH login per feature | Slow, MFA-hostile, extra auth-log noise | Shared per-host connection pool with refcounts | SSHV-003 |
| D7 | No component tests; e2e scripts are Python, manual, not in CI | UI regressions invisible; 90 components uncovered | Add jsdom/testing-library for hot components; move e2e to Playwright or wire the Python suite into CI | SSHV-026 |
| D8 | No lint/format tooling for TS; Rust is not in default rustfmt style (1679 diffs), so no fmt gate | Style drift | ESLint+Prettier; decide rustfmt.toml vs one reformat commit, then gate | SSHV-026 |
| D9 | Floating toolchain and tag-pinned actions | Non-reproducible CI | `rust-toolchain.toml`, pin actions by SHA | SSHV-026 |
| D10 | Vendored `picky`, `sspi`, `ironrdp-session` patches | No automatic advisories | Track upstream, document patch deltas, drop patches when upstreamed | SSHV-026 |
| D11 | Stray planning files at repo root (`auto.md`, `task-feat.md`; `feat*.md` currently deleted in the working tree) | Noise; unclear source of truth | Decide and move into `docs/` or delete | user decision |
| D12 | Local-only stores: DB query history in browser storage, runbook history not synced, alerts cleared on lock, `monitored` in localStorage | Inconsistent persistence; some data outside the vault threat model | Decide per item: vault-backed, local encrypted, or explicitly ephemeral | SSHV-005 |
| D13 | Docs claim macOS build; CI has none | Claim without evidence | Add job or correct docs | SSHV-026 |
| D14 | `ssh.write` serialises bytes as a JS number array | Slow large pastes | Use typed binary IPC | SSHV-028 |
