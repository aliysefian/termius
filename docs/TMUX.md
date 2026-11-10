# tmux in SSHVault

Tmux sessions belong to the **remote tmux server**, not to SSHVault's terminal tabs. A session can exist with no tab attached,
closing a tab never touches a session, and the app reads tmux over the connection it already keeps for monitoring, without
opening a terminal.

```
SSH host -> SSH transport -> tmux server -> session -> window -> pane          (remote state: src-tauri/src/tmux)
                |
                +-> PTY / shell -> terminal tab -> split                       (terminal state: the tabs in ui.svelte.ts)
```

## What was wrong with the earlier implementation (the audit)

The first tmux support (`multiplexer.rs`, `src/lib/tmux/discovery.ts`, the bar under the tab strip) read tmux once per terminal
pane. Each of these was reproduced against a real tmux 3.4 before it was changed:

| Problem | Cause | Fix |
| --- | --- | --- |
| "Attach" lands in the wrong session | `tmux attach -t NAME` matches by **prefix** when no session has that name: with only `db2` left, `-t db` is `db2` (`has-session -t db` succeeds, `-t =db` does not) | the bar uses `=name`; the monitor attaches by session id and checks the server first |
| A list that is wrong or forged | the probe was parsed line by line with `|`. tmux escapes tabs and newlines in session names but **not in window names or pane titles**, so a window named `x\n@@tmux-sessions\n$9|forged|…` injects rows | every run is read through a per-run random nonce (`@@<32 hex>`) that a name cannot contain; wrong shapes are errors, never shorter lists |
| Sessions missing | only the default socket; a server on `-L name` or `-S path` was invisible. Output was cut at 64 KB with the truncation flag ignored | sockets are first-class (`Socket`), other sockets in the default folder are offered, a run without its end marker is an error |
| Stale or empty list | a refresh requested while one was running was **dropped**; a failed refresh **cleared** the list | the request is repeated once the running one ends; the last good list stays, marked stale, with a back-off |
| Ids that point at something else | ids (`$0`, `@0`, `%0`) are only valid for one server process; a restarted server reuses them | every snapshot carries the server's pid and start time, and every change and attach checks it first |
| Switching typed into whatever was in front | `switch-client` was typed into the pane's shell; with another program in the foreground the text went to that program | a tab's own client is found by a marker (`SSHVAULT_TAB`) in the client's environment and moved with `switch-client -c`; nothing is typed |
| Names run commands | tmux expands `#{...}` in names and **runs `#(cmd)`** on the host; an argument ending in `;` ends the command | names have `#` doubled and a trailing `;` escaped (tested with a marker file that must not appear) |

## Pieces

* **`tmux::model/parse/script`**: snapshots by tmux id, strict parsing, the shell that talks to tmux.
* **`tmux::ops`**: every command that changes a server, one place. Targets are ids validated against tmux's own forms and quoted;
  after each change the host is read again and the change counts only if that reading shows it.
* **`tmux::manager`**: one snapshot per host and socket, shared by every view; ordered by `generation`; bounded concurrency; one
  poll loop per host and socket that ends with its last watcher; a locked vault stops it and forgets everything.
* **`tmux::search`, `tmux::resources`**: search over captured text (the `regex` crate: linear time); CPU and memory by process
  tree from `/proc`.
* **Frontend**: `src/lib/tmux/monitor.ts` (pure logic), `ansi.ts` (pane text as data), `actions.ts` (menus as data),
  `stores/tmuxmonitor.svelte.ts`, `components/tmux/*`.

## Behaviour worth knowing

* **Previews are snapshots** from `capture-pane`, not terminals: no input. Only what is on screen is captured (selected pane about
  every 3 s, other tiles every 8 s, at most 12 tiles), and nothing while the page is hidden. Pane text is held in memory only.
* **Search** covers the screen plus the scrollback tmux still holds (up to 5000 lines per pane); output tmux has discarded cannot be
  found. A search can be cancelled; a new search replaces the old.
* **Attach** types one `sh -c '…'` into a new tab (or split) of the host, so bash, zsh and fish read it the same way. Inside tmux on
  the same server it switches; inside another tmux it refuses instead of nesting.
* **Auto tmux** (per host, off by default): attach to a named session, create it if missing, or always start a new one. A shell
  already inside tmux and a host without tmux are left alone. Mouse support sets the `mouse` option **of that session only**;
  key bindings and the tmux config file are never modified.
* **Resources** are estimates: processes under each pane's shell, resident memory summed per process (shared pages are counted
  in each), CPU from two readings. A pane whose process cannot be seen is reported as not measured, never as zero.

## Limits

* tmux older than 2.0 is reported as unsupported (the sessions are still reachable through the older bar).
* Session tags are stored on this computer by session **name** (`host|socket|name`), because tmux ids do not survive a server
  restart. Renaming in SSHVault moves the tags; renaming elsewhere does not.
* Names cannot contain `.`, `:` or a backslash (tmux rewrites them), and `{{` is refused in socket paths (the app's own placeholder).
* Resource figures need Linux hosts (`/proc`).
* A terminal's tab is limited to six panes, the app's existing limit.
* Interface text is English only.
* Dragging a session into a split is not implemented; the context menu has the same actions.

## Tests

* `src-tauri/src/tmux/tests.rs`: 38 tests, most against a real tmux server on a private socket in a temp folder (`TMUX` is removed
  from every child, so the user's tmux is never touched). They skip when tmux (or `script`, for the pty ones) is missing.
* `src/lib/__tests__/tmuxmonitor*.test.ts`, `multiplexer*.test.ts`: tree, search, previews, store and attach logic.
* `src/lib/__tests__/e2e/tmuxmonitor.py`: the page in a headless browser with the backend simulated.
