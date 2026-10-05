# feat.md: features from uniTerm that SSHVault does not have yet

Source: https://github.com/ys-ll/uniterm (site: https://uniterm.net).
Compared against SSHVault 0.12.0 (`README.md`, `ROADMAP.md`, `task-feat.md`,
`src-tauri/src/*`). Only gaps are listed. Already covered and not repeated
here: SSH, SFTP, tunnels, Telnet, Serial, raw TCP, local shell, X11, split
panes, themes, custom keybindings, folder sync, host monitoring, scripting CLI.

Overlaps with `task-feat.md`: E14 (ZMODEM) and E16 (localisation) are already
listed there. They are kept below under their uniTerm names, and the work
should be done once, in whichever file is picked first.

## How an agent works on this file

1. Take one task at a time, in the order of "Suggested order" at the bottom.
2. Read every file named in the task's **Touches** line before editing.
3. Match the surrounding code: Svelte 5 runes, the existing Tauri command
   pattern in `commands.rs`, secrets only through the vault (never in
   logs, never in the webview unless the user opens the item to edit it).
4. A task is done only when **every** line of its DoD is true and the
   **Global DoD** below also passes. Tick the box and add a line to
   `CHANGELOG.md`.
5. If a DoD line cannot be verified in this sandbox (no sudo, no webkit,
   no real server), say so in the task under `Not verified:` instead of
   ticking it. Do not tick blind. Test through a scratch crate or a local
   container/server where possible.
6. One task is one commit. Do not mix tasks.

## Global DoD (applies to every task)

- [ ] `cargo test` and `cargo clippy -- -D warnings` pass in `src-tauri`.
- [ ] `pnpm check` and `pnpm test` pass; new pure logic has unit tests.
- [ ] New connection types and credentials are stored in the vault model
      (`models.rs`), encrypted at rest, and survive a sync round trip
      between two vaults. Old vaults open unchanged (no breaking migration).
- [ ] Secrets never appear in logs, error toasts, the connection log, or
      session logs.
- [ ] Everything new is reachable from the keyboard and the command palette,
      has an empty state, an error state with the full message, and uses the
      existing primitives (`Badge`, `Kbd`, `Spinner`, `EmptyState`, `Modal`).
- [ ] Anything that sends data unencrypted is labelled UNENCRYPTED the way
      Telnet is.
- [ ] Off by default if it opens a network listener or runs a remote probe.
- [ ] `docs/USAGE.md` has a section for it; `CHANGELOG.md` has an entry.

---

## Phase 1: file transfer protocols

All of these reuse the existing SFTP view (`SftpView.svelte`,
`FilePane.svelte`, `sftp.rs`). Do F0 first, then the protocols.

- [ ] **F0. A file-backend trait.** Extract what `sftp.rs` does (list, stat,
  read, write, mkdir, rename, delete, chmod where supported) behind a Rust
  trait, and let a pane be bound to any backend, local or remote. **M**
  Touches: `sftp.rs`, `commands.rs`, `SftpView.svelte`, `FilePane.svelte`,
  `sftp.ts`.
  DoD:
  - [ ] SFTP behaves exactly as before; the existing SFTP tests pass unchanged.
  - [ ] A backend can declare which actions it supports (for example no chmod
        on S3) and the right-click menu hides the rest.
  - [ ] A fake in-memory backend exists and is used in tests for copy, move,
        recursive delete, and cancel.
  - [ ] Transfers between any two panes work, even with different backends,
        with progress, cancel, and conflict choices (overwrite, skip, rename).

- [ ] **F1. SCP.** Use SCP as a fallback for servers with the SFTP subsystem
  disabled, and as a per-host choice. **S–M**
  DoD:
  - [ ] Upload and download of a file and a folder work against a server that
        has SFTP turned off (test with a local `sshd` with the subsystem
        removed, or document why not).
  - [ ] When SFTP fails with "subsystem request failed", the pane offers
        "Use SCP for this host" and remembers the choice on the host.
  - [ ] File names with spaces, quotes and unicode are quoted safely; a test
        proves a name like `a b'$(x).txt` cannot run a command.

- [ ] **F2. FTP and FTPS.** Explicit and implicit TLS, passive mode, saved as
  a host type. **M**
  DoD:
  - [ ] A new host type "FTP" with host, port, user, password or anonymous,
        and a TLS mode: none (labelled UNENCRYPTED), explicit, implicit.
  - [ ] Certificate problems show the fingerprint and need a deliberate
        "Trust this certificate", pinned like known hosts.
  - [ ] Browse, upload, download, rename, delete, mkdir, resume of an
        interrupted download. Tested against a local FTP server in CI or a
        scratch test.
  - [ ] Passive mode works behind NAT (uses the control connection's IP when
        the server returns a private one).

- [ ] **F3. SMB shares.** Browse and transfer on Windows and Samba shares. **M**
  DoD:
  - [ ] Host type "SMB" with server, share, domain, user, password;
        SMB2/3 only, SMB1 is refused with a clear message.
  - [ ] Lists shares on a server when none is given.
  - [ ] Browse, upload, download, rename, delete, mkdir work against a local
        Samba server (container or scratch test).
  - [ ] Signing/encryption status is shown in the pane header.

- [ ] **F4. WebDAV.** **S–M**
  DoD:
  - [ ] Host type "WebDAV" with URL, user/password or bearer token; HTTPS
        certificate errors need an explicit pin.
  - [ ] Browse, upload, download, rename, delete, mkdir, with `PROPFIND`
        depth handled for large folders (paged in the UI).
  - [ ] Tested against a local WebDAV server (for example `rclone serve webdav`).

- [ ] **F5. S3-compatible object storage.** AWS S3, MinIO, Cloudflare R2,
  Backblaze, and others by custom endpoint. **M**
  DoD:
  - [ ] Host type "S3" with endpoint, region, access key, secret key, optional
        session token, path-style toggle.
  - [ ] Buckets appear as top-level folders; "folders" are prefixes; large
        files use multipart upload with resume of a failed part.
  - [ ] Delete and overwrite of objects always asks first; no bucket delete.
  - [ ] "Copy pre-signed URL" with a chosen expiry.
  - [ ] Tested against a local MinIO.

- [ ] **F6. ZMODEM (`rz` / `sz`) in the terminal.** Same as E14 in
  `task-feat.md`. **M–L**
  DoD:
  - [ ] The raw byte stream detects the ZMODEM start sequence and shows
        "Receive file(s)" or "Send file(s)" instead of printing garbage.
  - [ ] `sz file` on the remote saves into a chosen folder with progress and
        cancel; `rz` on the remote opens a file picker and uploads.
  - [ ] Works over SSH, Telnet and Serial; a cancelled or corrupted transfer
        returns the terminal to a normal prompt.
  - [ ] Unit tests with recorded ZMODEM byte streams, including a stream
        with escaped control characters and a CRC error.

## Phase 2: terminal and connection types

- [ ] **T1. Mosh.** Start with the decision in `ROADMAP.md` ("Needs a
  decision"). Do the lower-risk option first. **L**
  DoD:
  - [ ] A host option "Use Mosh" that starts `mosh-server` over the existing
        SSH connection (vault keys, jump hosts, and known-hosts rules
        apply), then connects by UDP.
  - [ ] If `mosh-server` is missing on the remote, the error says so and
        offers a plain SSH connection.
  - [ ] Roaming (changing network) keeps the session; the pane shows
        "Reconnecting..." instead of dropping.
  - [ ] Documented limits: no scrollback from the server, no port forwarding
        over Mosh, Windows support status.
  - [ ] Not verified items are listed if no real `mosh-server` was available.

- [ ] **T2. Local shell picker and WSL.** `localpty.rs` exists; add choosing
  between installed shells (PowerShell, CMD, Git Bash, bash, zsh, fish) and
  listing WSL distributions (Windows). **S–M**
  DoD:
  - [ ] "New local terminal" shows detected shells; the default is a setting.
  - [ ] On Windows, `wsl -l -q` output (UTF-16) is parsed correctly, and each
        distro opens as its own terminal in its home directory.
  - [ ] On Linux and macOS the WSL entries do not appear.
  - [ ] Unit test for the shell-detection and the `wsl -l` parser.

- [ ] **T3. Remote desktop: RDP.** **L**
  DoD:
  - [ ] Host type "RDP" with host, port, user, domain, password from the
        vault, NLA on by default, resolution and colour depth options.
  - [ ] Opens in a tab beside the terminals; keyboard (including Ctrl+Alt+Del
        action and key mapping), mouse, wheel and clipboard text work.
  - [ ] Certificate shown and pinned on first connect like SSH host keys.
  - [ ] Tested against a real or containerised RDP server; if not possible
        the task stays unticked.

- [ ] **T4. Remote desktop: VNC.** **M–L**
  DoD:
  - [ ] Host type "VNC" with password from the vault, scaling to fit, view
        only mode, clipboard text.
  - [ ] "VNC over SSH": connect through the host's SSH connection to
        `127.0.0.1:5900` so VNC itself is never exposed (the default
        suggestion when the host has SSH).
  - [ ] Insecure (no-auth) servers show a warning badge.

- [ ] **T5. Remote desktop: SPICE** for KVM/QEMU VMs. **L**
  DoD:
  - [ ] Host type "SPICE" with host, port, password, TLS option.
  - [ ] Display, keyboard, mouse and clipboard text work against a local
        QEMU VM.
  - [ ] Shares the display tab component with RDP and VNC (T3, T4); no
        separate viewer UI.

## Phase 3: databases

One shared "Databases" view with a connection tree, a query editor and a
result grid. Build D0 first.

- [x] **D0. Database view shell.** Done 2026-10-05. A new activity-bar view,
  a connection type "Database" in the vault (`Collection::Databases`), a
  tree browser, tabbed query editors with history, and a result grid. **L**
  Engine: `src-tauri/src/db/` (`mod.rs` types and manager, `safety.rs`,
  `mysql.rs`, `live_tests.rs`); tunnel: `forward.rs` (`open_local_tunnel`).
  UI: `DatabasesView`, `DbConnectionForm`, `ResultGrid`,
  `stores/databases.svelte.ts`, `lib/dbdata.ts`, `lib/dbhistory.ts`.
  DoD:
  - [x] Result grid is virtualised (about 30 rows in the DOM for a 100,000-row
        result), supports copy as CSV/JSON/TSV, save to file, and sort of
        loaded rows. Sorting 100k rows is unit-tested under 2 s.
  - [x] Queries run with a row limit by default (1,000; choices up to 100,000)
        and can be cancelled. Reading stops at the limit and the connection
        is ended rather than drained; a 300,000-row query returns 1,000 rows
        in well under a second against a live server. Cancel is tested live
        and a killed `SLEEP()` reads as cancelled, not as a result.
  - [x] Connections can go through the host's SSH tunnel; the database port is
        never exposed. Tested live through a real `sshd`: the loopback port
        answers while the session lives and is refused after it closes.
  - [x] Destructive statements (`DROP`, `TRUNCATE`, `DELETE` or `UPDATE`
        without `WHERE`, `ALTER ... DROP`) ask first, with the reason; the
        backend refuses them unless the caller confirms. Hosts marked
        production ask for typing the connection's name (also for every
        row edit).
  - [x] Query history is local-only (`localStorage`, never the vault).
  Not verified: the real Tauri window (no webkit here). The UI was driven in
  headless Chromium with a mocked IPC layer; the native save-file dialog and
  the clipboard plugin were mocked. TLS modes were not tried against a real
  TLS server.
  Found and fixed on the way: pressing Enter in the cell editor opened the
  confirm dialog and the same keypress confirmed it (an UPDATE ran with no
  confirmation). Enter no longer propagates and row-edit confirmations are
  always the careful kind, with Cancel focused.

- [x] **D1. MySQL / MariaDB.** Done 2026-10-05, tested against MariaDB 11.
  Tree of databases, tables, views, columns and indexes (your databases
  first, system schemas last); inline cell edit with a preview of the
  generated `UPDATE`, run with bound parameters and `LIMIT 1`; works
  through an SSH tunnel. **M**
  DoD:
  - [x] Tree of databases, tables, columns, indexes (live test).
  - [x] Inline edit previews the `UPDATE` before it runs; the key is the
        primary key; NULL keys, missing keys, binary and cut cells are
        refused with a reason; a value that looks like SQL stays a value
        (tested live with `'; DROP TABLE items; --`).
  - [x] Works through an SSH tunnel (live test).
  - [x] Tested on a local server (MariaDB 11 in Docker; see the note at the
        top of `db/live_tests.rs` for the setup and the env var).
  Not verified: Oracle MySQL 8 (different auth plugin defaults, same driver),
  and TLS against a real TLS server.

- [ ] **D2. PostgreSQL.** DoD: same as D1, plus schemas and views; TLS modes
  `disable`, `require`, `verify-full` offered. **M**
- [ ] **D3. SQL Server.** DoD: same as D1; Windows and SQL logins; tested on a
  container or left unticked with `Not verified:`. **M**
- [ ] **D4. Oracle Database.** DoD: same as D1 for service name connections;
  say clearly in the docs if a client library is needed. **M–L**
- [ ] **D5. rqlite.** DoD: HTTP API client with basic auth and TLS; same tree
  and grid; read-after-write consistency level selectable. **S–M**
- [ ] **D6. Redis.** DoD: key browser with a type icon and TTL, scan-based
  (never `KEYS *`), value viewer/editor for string, hash, list, set, zset,
  stream; a command console; TLS and ACL user support. **M**
- [ ] **D7. MongoDB.** DoD: databases and collections tree, a JSON filter
  bar, a document viewer with inline edit of a field, index list, and
  connection by URI or form. **M**
- [ ] **D8. Elasticsearch.** DoD: indices list with doc count and size, a
  query console (`GET/POST` with JSON body), a document viewer, and API-key
  or basic auth. **M**

## Phase 4: containers

One "Containers" view; each runtime is a provider with the same list,
logs, exec and stats actions. Build C0 first. Run everything over the
existing SSH connection, so nothing is installed on the host.

- [ ] **C0. Container provider interface and view.** **M**
  DoD:
  - [ ] A host (or the local machine) can be opened in the Containers view;
        the runtime is auto-detected and can be set by hand.
  - [ ] Lists containers and images with state, ports, age, and size; search
        and filter; auto-refresh that can be paused.
  - [ ] Actions: start, stop, restart, remove (asks first), view logs
        (follow, search, copy), open a shell (a normal terminal tab),
        inspect (JSON).
  - [ ] Command output is parsed from `--format json`, never from column
        text, and unit tests use recorded outputs.

- [ ] **C1. Docker.** DoD: C0 plus images (pull, remove, prune with a
  preview of what will be removed), volumes, networks, `docker compose`
  projects grouped and actionable; context/remote-socket not needed. **M**
- [ ] **C2. Podman.** DoD: C0 on rootless and rootful; pods shown as groups. **S–M**
- [ ] **C3. nerdctl / containerd.** DoD: C0 with a namespace switcher that
  lists namespaces and remembers the last one per host. **S–M**
- [ ] **C4. Kubernetes.** DoD: kubeconfig read from the host (or pasted into
  the vault, encrypted); context and namespace switcher; pods, deployments,
  services, nodes, events; pod logs (follow, previous, container choice),
  exec into a pod as a terminal tab, port-forward as a tunnel entry, delete
  pod and scale deployment with confirmation. **L**
- [ ] **C5. WSL containers (WSLC) on Windows.** DoD: lists and manages
  Docker-in-WSL2 per distro through C1; hidden on non-Windows. **S**

## Phase 5: richer monitoring

Extends E7 (`hostmetrics.ts`, `FleetView.svelte`, `health.rs`). Still opt-in
per host and agentless.

- [ ] **M1. Network, processes, ports and interfaces.** **M**
  DoD:
  - [ ] Per-host monitoring adds: network throughput per interface,
        top processes (CPU and memory, sortable), listening ports with the
        owning process, and an interface list with addresses and state.
  - [ ] "Kill process" sends `SIGTERM` after a confirmation; `SIGKILL` is a
        second, separate action.
  - [ ] Works on Linux; BSD/macOS shows what it can and says which panels
        are unavailable (also fixes the CPU/memory gap noted in 0.12.0).
  - [ ] Parsers are unit-tested with recorded outputs from at least Debian,
        Alpine (BusyBox) and one non-Linux system.
  - [ ] History charts keep the last 15 minutes in memory only; nothing is
        written to disk.

## Phase 6: AI assistant

Everything here is opt-in, off by default, and the vault never sends a
secret to a model. Build A0 first.

- [ ] **A0. LLM provider settings.** **M**
  DoD:
  - [ ] Settings → AI: provider type (Anthropic API, OpenAI-compatible),
        base URL, model, API key stored in the vault, test button.
  - [ ] A local endpoint (Ollama, LM Studio) works with no key.
  - [ ] Nothing is sent until the user has switched AI on and accepted a
        one-time notice saying what is sent (the prompt and the terminal
        text they chose to share).
  - [ ] Passwords, private keys and tokens are redacted from anything sent
        (reuses the private-key/token detector from the paste guard); a test
        feeds sample secrets and proves they are masked.

- [ ] **A1. Assistant panel with command suggestions.** **M**
  DoD:
  - [ ] A side panel per terminal pane: describe a goal, get a command, with
        an explanation; "Insert at prompt" never presses Enter.
  - [ ] "Explain this output" and "Why did that fail?" send only the
        selection or the last N lines the user can see in a preview.
  - [ ] The panel shows which host, user and directory it is talking about.

- [ ] **A2. Agent mode: multi-turn command execution.** **L**
  DoD:
  - [ ] The assistant can run commands in a pane over several turns toward a
        goal, with a step list, live output, a Stop button that works at any
        time, and a turn limit.
  - [ ] Execution modes: **confirm every command** (default),
        **confirm only risky commands**, **bypass** (needs a per-host opt-in
        and is refused outright on hosts marked production).
  - [ ] A risk classifier flags `rm -r`, `dd`, `mkfs`, `chmod -R`, `shutdown`,
        pipes to `sh`, `sudo`, and writes to `/etc`; unit-tested with a list
        of at least 50 commands.
  - [ ] Every command the agent ran is recorded in the connection log with
        who approved it (user or mode).

- [ ] **A3. Reusable skills.** **M**
  DoD:
  - [ ] A "skill" is a named, saved workflow (a prompt plus allowed steps and
        variables), stored in the vault, synced, and importable/exportable as
        a plain file.
  - [ ] Skills run from the assistant panel and the command palette, with the
        same execution modes as A2.
  - [ ] Ships with at least five examples (disk usage report, check failed
        services, rotate logs, tail and summarise errors, security updates
        check) that all default to confirm-every-command.

- [ ] **A4. MCP server for external AI agents.** **L**
  DoD:
  - [ ] An MCP server (stdio, and local socket) exposed by the app or CLI,
        off by default, switched on per computer like the scripting CLI.
  - [ ] Tools: list hosts, run a command on a host, read a file, list a
        directory. No tool returns a secret, and nothing can read or write
        the vault itself.
  - [ ] Every call is approved in the app, using the same prompt and
        optional trust window as `sshvault run`; the trust window never
        covers production hosts.
  - [ ] Tested with a real MCP client or the MCP inspector; if not possible,
        the protocol conformance test output is attached under `Not verified:`.

## Phase 7: sync and personalisation

- [ ] **S1. Sync through Git.** The vault is already encrypted, so a repo
  only ever holds ciphertext. **M**
  DoD:
  - [ ] Settings → Sync → "Git repository": URL, branch, and auth by the
        vault's own keys or a token.
  - [ ] Pull on unlock, push after changes, with the existing conflict
        handling in `sync.rs`; a diverged repo shows a clear choice, never
        a force push.
  - [ ] Nothing but vault files is committed; a test checks the repo
        contents contain no plaintext host names.

- [ ] **S2. Sync through WebDAV.** **S–M**
  DoD:
  - [ ] Settings → Sync → "WebDAV": URL and credentials stored in the vault;
        uses ETags to avoid overwriting a newer file.
  - [ ] Two vaults converge after edits on both sides (test with a local
        WebDAV server).
  - [ ] Works alongside folder sync as an alternative, not at the same time.

- [ ] **P1. Localisation (9 languages).** Same as E16. **L**
  DoD:
  - [ ] Every user-visible string goes through `t()`; a lint or test fails on
        a new hard-coded string in a component.
  - [ ] English plus at least eight more languages (suggested: Chinese,
        Spanish, French, German, Portuguese, Russian, Japanese, Persian with
        right-to-left layout), switchable live in Settings.
  - [ ] Missing keys fall back to English and a test lists them.
  - [ ] Dates, numbers and file sizes follow the chosen locale.

---

## Suggested order

1. **F0, F1, F4, F5, F2** (file protocols that reuse the SFTP view).
2. **M1, T2, F6** (small wins on top of what exists).
3. **C0, C1, C2, C3, C4, C5** (containers).
4. **A0, A1, A2, A3, A4** (AI, in this order; A4 last).
5. **D0, then D1, D2, D6, D7, D5, D8, D3, D4** (databases, common ones first).
6. **S1, S2, P1** (sync and languages).
7. **F3, T3, T4, T5** (SMB and remote desktop: the largest and hardest to
   verify here).
8. **T1** (Mosh) after the decision in `ROADMAP.md` is made.

## Out of scope

- A hosted account or sync service (the folder, Git and WebDAV sync designs
  are the point).
- Mobile apps.
- Anything that needs an agent installed on the remote host.
