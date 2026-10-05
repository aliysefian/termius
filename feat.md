# feat.md: features from uniTerm that SSHVault does not have yet

Source: https://github.com/ys-ll/uniterm (site: https://uniterm.net).
Compared against SSHVault 0.12.0 (`README.md`, `ROADMAP.md`, `task-feat.md`,
`src-tauri/src/*`). Only gaps are listed. Already covered and not repeated
here: SSH, SFTP, tunnels, Telnet, Serial, raw TCP, local shell, X11, split
panes, themes, custom keybindings, folder sync, host monitoring, scripting CLI.

Overlaps with `task-feat.md`: E14 (ZMODEM) and E16 (localisation) are already
listed there. They are kept below under their uniTerm names, and the work
should be done once, in whichever file is picked first.

## Status (2026-10-05)

4 of 36 tasks are done. Everything below is committed on `main` and not
pushed or released (`CHANGELOG.md` has them under "Unreleased").

| Phase | Done | Next |
|---|---|---|
| 1. File transfer | 0 of 7 | F0 |
| 2. Terminal and connection types | 0 of 5 | T2 |
| 3. Databases | 3 of 9 (D0 and D1 `acfb247`, D2 `a72c4dd`) | D6, D7, D5, D8, D3, D4 |
| 4. Containers | 1 of 6 (C0 `45a9743`) | C1 |
| 5. Monitoring | 0 of 1 | M1 |
| 6. AI assistant | 0 of 5 | A0 |
| 7. Sync and languages | 0 of 3 | S1 |

Open items on done tasks (not blockers, but unproven): MySQL and MariaDB
TLS has never been tried against a real TLS server (the CA-plus-leaf recipe
in `db/live_pg_tests.rs` shows how); Podman and nerdctl parsers rest on
fixtures written from their documentation; nothing has run in the real
Tauri window.

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

## Tooling and traps (learned building D0 to C0)

None of this is in the repo; the scratchpad is per session, so recreate it.

**Rust.** The Tauri crate cannot build natively here (no webkit, no sudo).
- *Engine modules* (everything except `commands.rs`, `control.rs`,
  `selfupdate.rs`, `acceptance_tests.rs`) are tested through a scratch crate:
  its `Cargo.toml` is `src-tauri/Cargo.toml`'s `[dependencies]` minus the
  `tauri*` lines, and its `lib.rs` is one `#[path = ".../src/X.rs"] pub mod X;`
  per module (`X/mod.rs` for folders, skip `fixtures`). **Regenerate the
  module list when you add a module**, or its tests silently don't run
  ("0 passed").
- *The whole crate* (commands, `lib.rs`) is type-checked and linted for
  Windows: a venv with `pip install ziglang`, a shim folder on `PATH` with an
  `x86_64-w64-mingw32-gcc` that runs `python -m ziglang cc -target
  x86_64-windows-gnu` (dropping `--target=*` and `-gdwarf-2`) and an
  `x86_64-w64-mingw32-windres` that writes `!<arch>` to its output file, then
  `cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings`
  with a `CARGO_TARGET_DIR` outside the repo. A careless `windres` shim once
  left a file named `src-tauri/--output-format=coff` in the repo: check
  `git status`.
- *Live tests* are `#[ignore]` and gated on an environment variable, with the
  setup in the file's header comment (`db/live_tests.rs`,
  `db/live_pg_tests.rs`, `containers/live_tests.rs`). `ssh::testutil::spawn_sshd`
  gives a real user-level `sshd` for tunnel and SSH-exec paths. Use throwaway
  containers with distinctive names and remove them. Images already on the
  machine are used; **ask before pulling a large one** (`mariadb:11` was
  pulled once, with permission implied by the task).
- Fixtures: record real output where a program exists here, and say plainly in
  the file and the task when one is written from documentation.

**UI.** No webkit means no real window, so the frontend is driven in headless
Chromium (Playwright for Python; the cached browser needs `executable_path`
pointing at `chrome-headless-shell`) against `pnpm exec vite dev --port 1420`,
with an init script that defines `window.__TAURI_INTERNALS__` (`invoke`,
`transformCallback`, `metadata`). A `Channel` receives `{ index, message }`
through `window["_" + channel.id]`. Stop the dev server by port; **never
`pkill -f` from the shell tool**, it matches (and kills) the shell itself.

**Traps found, each fixed once already.**
- Pressing Enter in a field that opens a confirm dialog also presses the
  dialog's focused OK button. `preventDefault()` on that Enter, and make the
  dialog the careful kind so Cancel has the focus.
- A Svelte `$derived` that returns the *same* array or object after it was
  mutated tells nobody. Return a fresh snapshot object (`ContainerLogs`).
- Don't mutate a prop (`ownership_invalid_mutation`); put the mutation in a
  store method.
- `tokio_postgres::Config::port()` appends rather than replaces; the driver
  sends binary parameters, so text for a typed column needs a text-format
  `ToSql` wrapper (`TextParam`).
- Build an `UPDATE` from pieces, never by replacing text, so a value that
  looks like a placeholder cannot shift the others.
- A `?` inside a closure that returns a plain value won't compile; use
  `and_then` with early returns.
- A run that ends with an explicit transaction open leaves a pooled
  connection poisoned; engines `ROLLBACK` after a failed script, and
  `run_query` refuses a dangling `BEGIN`.

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
- [ ] A change of data (an UPDATE, a stop, a delete) is confirmed in a dialog
      that has Cancel focused, never the confirm button, so a stray Enter
      cannot do it. Production hosts and connections need their name typed.
- [ ] The UI has been driven in a browser with the IPC mocked (see
      "Tooling and traps"), at least one check per DoD line that is about
      behaviour you can see.
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
result grid. Build D0 first. (Done for MySQL, MariaDB and PostgreSQL.)

**Adding an engine (D3, D4, D5).** `db/<engine>.rs` provides connect, query
(streaming, stopping at the limit), cancel, children, table_info,
preview_update, apply_update, close, version; then a variant on `Engine` in
`db/mod.rs` (box it if it is large) and an entry in `ENGINES`; `DB_ENGINES`
(label, default port) in `types.ts`; `quoteIdent` in `dbdata.ts` with a test;
a `NodeKind` if the first level of the tree isn't a database or schema; a
live-test file with its setup in the header; USAGE and CHANGELOG. Reuse
`update_pieces`/`join_with`, `safety.rs`, the SSH tunnel (`tunnel_port`) and
`TlsMode`. `pg.rs` is the closer model (streaming, pool, TLS).

**Decision before D6, D7 and D8.** Redis, MongoDB and Elasticsearch don't
return rows and columns, so they don't fit the SQL editor and `QueryResult`.
Give them their own tab type inside the same view (the connection list, vault
record, redaction, tunnel and production prompts are shared; the tab body is a
key browser, a document viewer or a console). Settle that shape with the user
on D6 and let D7 and D8 follow it. D5 (rqlite) is SQL over HTTP and does fit.

**D3 note.** SQL Server's official image needs `ACCEPT_EULA=Y` and is about
1.5 GB: ask before pulling it, and leave the task `Not verified:` if the user
says no.

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

- [x] **D2. PostgreSQL.** Done 2026-10-05, tested against PostgreSQL 16.
  Same tree, grid, query and edit flow as D1, with schemas and views; TLS
  modes none, require and verify-full. **M**
  Engine: `src-tauri/src/db/pg.rs` (`tokio-postgres`, rustls + ring, the OS
  trust store through `rustls-platform-verifier`); tests in
  `live_pg_tests.rs` (setup and env vars at the top of that file).
  DoD:
  - [x] Tree of schemas, tables, views, materialized views, foreign and
        partitioned tables, columns, and indexes (a connection is to one
        database, so the tree starts at its schemas).
  - [x] Inline edit previews the `UPDATE`; values are sent as text-format
        bound parameters, so numeric, boolean, timestamp and jsonb columns
        take their natural text and a bad value is the server's own error.
        Composite keys, names with quotes and spaces, and a key that matches
        nothing are tested live.
  - [x] Reading stops at the row limit and the rest is cancelled on the
        server: 3,000,000 rows return 1,000 in well under 10 s and the
        connection is clean afterwards. Cancel works, and overlapping
        queries run in parallel.
  - [x] TLS: "none" is really unencrypted and "require" really encrypted
        (checked with `pg_stat_ssl`); verify-full refuses an untrusted
        certificate with the reason, accepts one once its CA is trusted, by
        name or address, and refuses a name the certificate doesn't cover.
        Through an SSH tunnel the name is still checked (`hostaddr`).
  - [x] Works through an SSH tunnel (live test, real `sshd`).
  Also: the single-statement and open-transaction checks are engine-neutral
  (they also fix MySQL, where `BEGIN` in one run silently lost its
  transaction on the next); the SQL reader now understands `$$ ... $$`
  bodies. Not verified: PostgreSQL versions other than 16, SCRAM vs md5
  auth variants beyond the default, and the real Tauri window (UI driven
  headless with mocked IPC, as for D0).

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
existing SSH connection, so nothing is installed on the host. (C0 is done.)

**Providers (C1 to C3).** A runtime is a `Runtime` variant, an argument list
(`list_containers_args`, `list_images_args`, `action_args`, `logs_args`), and a
reader in `containers/parse.rs`. New actions follow `act()`: a checked
reference, the argument list in `mod.rs` with a test, a case in the store with
the right confirmation, a button. Streaming work (image pull progress, `events`)
should copy `start_logs` (a stop channel, a sink, an `End` event, remote PTY
so closing the channel ends the process). Keep references checked, not escaped.
- **C1.** Compose groups already have their label (`composeProject`); acting on
  a project is `docker compose -p NAME start|stop|restart|down`, which works
  by name without the compose file (verify on a real project). Prune needs a
  preview first: list what would go (dangling images, stopped containers) and
  ask.
- **C2.** First step: run `podman ps`/`images --format json` for real and
  replace the hand-written fixtures with recorded ones; fix the reader where
  they differ. Then pods as groups.
- **C3.** `nerdctl --namespace NAME` is a global flag that goes before the
  verb; add it to the argument builders, to `Source`, and to the per-host
  memory.
- **C4.** `kubectl -o json` is not a runtime variant: a sibling reader module
  and its own list shapes, reusing `Transport`, the checked references, the
  log stream and the production prompt.

- [x] **C0. Container provider interface and view.** Done 2026-10-05. **M**
  Engine: `src-tauri/src/containers/` (`mod.rs` manager and argument lists,
  `parse.rs` the JSON readers, `transport.rs` local and SSH, recorded and
  hand-written `fixtures/`, `live_tests.rs`). UI: `ContainersView`,
  `ContainerLogs`, `ContainerInspect`, `stores/containers.svelte.ts`,
  `lib/containerdata.ts`.
  DoD:
  - [x] A host (or this computer) opens in the Containers view; the runtime is
        detected and can be set by hand. Several sources can stay open.
  - [x] Containers and images are listed with state, ports, age and size;
        search and a state filter; auto-refresh (2/5/10/30 s) that can be
        paused, skips a hidden window, and keeps the old list with the error
        when a refresh fails.
  - [x] Start, stop, restart, remove (always asks; force only for a running
        container, and says so), logs (follow, tail, timestamps, search with
        highlight and next/previous, only-matches, copy), a shell in a normal
        terminal tab (local or host), and inspect as JSON.
  - [x] Output is parsed from the runtime's JSON (`--format json` /
        `{{json .}}`), never from column text. Tests use output recorded from
        a real Docker 29 (ps, ps with sizes, images). The remote command is
        one single-quoted `sh -c` script, container references are checked
        (letters, digits, `_.-:/@` only, never a leading dash) rather than
        escaped, and the log follow takes a terminal on the remote side.
  - [x] Nothing is installed on a host; one SSH connection per source is
        reused (a channel per command), and reopened if it drops.
  Verified live: local Docker and Docker through a real `sshd` (list,
  start/stop/restart/remove, errors, inspect, bounded and followed logs with
  non-ASCII text, and that stopping a followed log ends the remote
  `docker logs` process), plus the UI in headless Chromium with a mocked
  runtime (production typed-name prompts, force-remove dialog with Cancel
  focused, pause, error recovery, shell command typed into a local terminal).
  Not verified: Podman and nerdctl against the real programs (their parsers
  are tested on fixtures written from their documentation, marked as such);
  Docker on Windows or macOS hosts; hosts whose login shell is not POSIX
  (Windows OpenSSH); the real Tauri window. Podman's JSON `Ports`, `Created`
  and `RepoTags` shapes in particular should be checked on a real host.
  Left for C1: image pull/remove/prune, volumes, networks, Compose actions.

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


## Suggested order

1. **F0, F1, F4, F5, F2** (file protocols that reuse the SFTP view).
2. **M1, T2, F6** (small wins on top of what exists).
3. **C0 (done), then C1, C2, C3, C4, C5** (containers).
4. **A0, A1, A2, A3, A4** (AI, in this order; A4 last).
5. **D0, D1, D2 (done), then D6, D7, D5, D8, D3, D4** (databases, common ones first).
6. **S1, S2, P1** (sync and languages).
7. **F3, T3, T4, T5** (SMB and remote desktop: the largest and hardest to
   verify here).
8. **T1** (Mosh) after the decision in `ROADMAP.md` is made.

## Out of scope

- Mobile apps.
- Anything that needs an agent installed on the remote host.
