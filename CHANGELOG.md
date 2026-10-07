# Changelog

Every notable change, newest first. Dates are when the release was tagged.
This file is also shown in the app itself, under **Settings → Updates →
View changelog**.

## 0.21.0 — 2026-10-07

- **Terminal.** *Command blocks:* with shell integration, a thin bar in the margin beside each command, green
  when it worked and red when it failed; hover for the command, outcome and duration, click to copy its output
  or command, select the output or pin it, and jump between failed commands from the right-click menu.
  *Highlight words:* colour words such as ERROR in the output, in order, with a live preview and one-click
  starting sets; a rule can also raise a notice when new output matches. *Search all terminals:* one box over
  the screen and scrollback of every open terminal (command palette). *A look per host:* a theme, text size
  and scrollback for one host. *One-click shell integration:* a pane's right-click menu shows exactly the lines
  that would be added to the host's startup file and adds or removes them (a copy of the file is kept).
  Also: emoji and wide characters line up (Unicode 11), pictures drawn with Sixel or the iTerm2 protocol,
  clickable `file:line` paths, and links a program marks up are followed only for web and mail addresses
  and confirmed when the address differs from the text shown.
- **Everyday polish.** A guided tour (*Take the tour* in the command palette). The command
  palette also finds keys, credentials, groups, trusted servers and settings. The sidebar can be rearranged:
  names on or off, entries put away into Manage, entries reordered. The sidebar, menus, headings and tour
  are available in Spanish, German, Portuguese, Russian, Chinese, Japanese and Persian. Ten more terminal themes
  and a high-contrast app theme. A starter set of read-only snippets, and snippet packs to import and export.
- **Accessibility.** Every page and dialog now passes axe-core in the dark, light and high-contrast themes:
  text and icon contrast raised (the accent and the muted grey), real landmarks and headings, a properly
  labelled command palette and tab list, and no controls nested inside other controls.
- **Large vaults.** The host list draws only the rows in view once it has more than a few hundred: with 5,000
  hosts, start-up went from 22 s to 3 s, memory from 670 MB to 82 MB and the page from 5,800 elements to 23.
  Search now opens folded groups so matches are never hidden.
- **Release signing, built but not yet switched on.** Releases can now carry a GPG
  signature on every installer and a signed `SHA256SUMS`, and the Windows build
  verifies that its installers are validly signed before publishing. Neither
  happens until a certificate and a key are added to the repository; see
  `docs/SIGNING.md` (how to turn it on, and `scripts/verify-release.sh` for
  checking a download).

## 0.20.0 — 2026-10-07

- **Kubernetes.** A new tab next to Containers lists a cluster's pods through the
  `kubectl` on this computer or on a saved host: contexts, namespaces, coloured
  status, restarts and age, followed logs (per container, previous run), describe,
  a shell in a pod in a terminal tab, delete (typed name on production hosts), and
  port-forwards to this computer. Names are checked and passed as separate
  arguments; every call has a time limit.

## 0.19.0 — 2026-10-07

- **Operations.** A new **Ops** page with three tabs. **Logs** follows the system
  journal or a log file on several hosts at once, with filtering, regular
  expressions, level colours, highlighting and pause. **Services** lists systemd
  units and starts, stops, restarts, reloads, enables or disables them, with a
  typed-name confirmation on production hosts. **Alerts** (off until you turn them
  on) tells you when a monitored host stops answering or its CPU, memory or disk
  stays high, with quiet hours and per-host muting.
- **Monitoring charts can be kept** for a day or a week (Settings → Alerts and
  monitoring history); the host detail view gains 24 h and 7 day ranges. Stored on
  this computer only; off by default.

## 0.18.1 — 2026-10-07

- **Fixed: stray text like `35;2;16M35;6;15M` in the shell after a program that
  uses the mouse (Claude Code, vim, tmux) quit or the connection dropped.** Mouse
  reports are now sent only while a program has actually asked for them. A new
  or restarted session, a dropped connection and a shell prompt each switch the
  mouse (and bracketed paste, application keys and the alternate screen, where
  they apply) back off, and a soft terminal reset (`ESC [ ! p`) now turns the
  mouse off too.
- **Fixed a test that failed on machines with `kubectl` but no cluster**, which
  stopped the 0.17.0 and 0.18.0 release builds. The host lookups for kubectl
  now give up after two seconds.

## 0.18.0 — 2026-10-07

- **A new look for the sidebar and a new logo.** The left rail has seven labelled
  icons in groups (Hosts and Favorites; Snippets, Files and Tunnels; Databases and
  Containers). Groups and proxies, Keys, Credentials, Known hosts and the Vault are
  under **Manage**. The app icon, favicon and unlock screen carry the new
  shield-and-prompt logo.
- **Tidier forms and lists.** The host form's tabs and option cards are spaced
  properly, the host list filters by environment with buttons instead of a
  cut-off drop-down, and **Duplicate** on a host asks before copying it.
- **A little motion.** Dialogs, notices and the Manage menu ease in, and empty
  screens have small illustrations. Turning on reduced motion in your system
  turns it all off.

## 0.17.0 — 2026-10-06

- **Smart completion** (off by default; Settings → Terminal). A faint suggestion
  after the cursor from your own history (→ or End accepts, Ctrl+→ one word,
  Esc dismisses), and Ctrl+Space for a list: subcommands, options and values
  for about 60 commands, file and folder names from the host, git branches,
  containers and systemd units, matching history and your snippets. Each host
  can override it, and production hosts use history only. It never types for you,
  stays out of full-screen programs and password prompts, and never stores
  commands that look like they hold a password or token. Host lookups use an
  extra channel of the same SSH connection and stop quietly if the host refuses.
  Command data is converted from `withfig/autocomplete` (MIT); see
  `THIRD_PARTY.md`.
- **Fixed: snippets with `{{variables}}` did nothing after you filled them in.**
  The values dialog closed, then failed before sending the snippet. It now runs.
- **Shell integration for fish.** The snippet in Settings → Shell integration
  now adds the prompt marks to fish, which does not send them itself in version
  3 (the text used to say it did). With it, the current folder, prompt jumping,
  copy last output and long-command notifications work in fish.

## 0.16.0 — 2026-10-06

- **SCP and FTP/FTPS in the file browser.** When a server has SFTP turned off,
  connecting now says so and offers **Use SCP for this host** (remembered on
  the host). New **FTP / FTPS** hosts browse, upload, download, rename, delete
  and make folders over plain FTP, explicit TLS or implicit TLS, continue
  interrupted transfers, and work with servers behind NAT. A server's TLS
  certificate is shown and pinned like an SSH host key, and plain FTP is
  marked UNENCRYPTED. A name that is already at the destination now asks:
  Replace, Skip or Keep both. Underneath, every file pane is one interface, so
  panes only offer what their server can do. Tested against real servers:
  OpenSSH with SFTP removed, Pure-FTPd (plain, TLS, NAT) and ProFTPD (implicit
  TLS).

## 0.15.0 — 2026-10-06

- **Remote Desktop (RDP).** A host can now be a Windows (or xrdp) desktop:
  choose **Remote Desktop (RDP)** as its protocol, with a domain, screen size
  (or fit the tab), colour depth and sign-in security. It opens in a tab beside
  your terminals with keyboard, mouse, wheel and shared clipboard text, and a
  Ctrl+Alt+Del button. The server's certificate is shown the first time and
  trusted from then on, like an SSH host key; a different one later is refused
  before any password is sent. Tested here against a real xrdp server; sign-in
  with Network Level Authentication, and Windows servers, were not available to
  test. No sound or file redirection, and no jump hosts yet.

- **Mosh.** Tick "Use Mosh" on a host and it connects the way `mosh` does:
  SSH logs in (with your vault keys, jump hosts, proxies and trusted-server
  rules), starts `mosh-server`, and your system's `mosh-client` takes over
  over UDP, so the session survives network changes and sleep. If the host has
  no `mosh-server`, the tab says so and offers plain SSH. Needs Mosh installed
  on this computer; tested here against a real `mosh-server` and `mosh-client`
  but not on Windows or macOS.

- **Choose the shell for a local terminal.** The arrow beside the local
  terminal button lists the shells found on this computer (your login shell
  first; PowerShell, Command Prompt and Git Bash on Windows) and opens one in a
  new tab. The command palette has the same entries, and Settings → Local
  terminal has a default-shell menu. On Windows each WSL distribution is listed
  too and opens in its own home folder; on Linux and macOS there are no WSL
  entries. The Windows part is written and checked here but not yet run on
  Windows.

## 0.14.0 — 2026-10-05

- **A detail view for monitored hosts.** Open it from a host's details card
  or its Fleet tile to see, over one SSH connection kept open while the window
  is: charts of CPU, memory and network throughput for the last 15 minutes
  (kept in memory only), every process with its CPU and memory (sortable,
  searchable), what is listening on which port and which process owns it, and
  each network interface with its addresses and state. Terminate a process
  after a confirmation, or force-kill it as a separate, stronger action; the
  host is asked first whether that process number still belongs to the same
  program. On macOS and FreeBSD it shows what those systems can tell, says what
  it can't, and the summary now includes CPU and memory there (written from the
  manuals, not yet run on real macOS or BSD hardware).

- **Docker images, volumes, networks and Compose projects.** The Containers
  view gains tabs for volumes and networks (each showing which containers use
  it) and can pull, remove and clean up images, volumes and networks. "Remove
  unused…" shows exactly what would go before anything does, then removes only
  what you ticked, rechecking each item first; volumes start unticked because
  their data can't be recovered. Containers started by Compose are grouped by
  project, with start, stop, restart and take-down for the whole project (take
  down keeps the volumes). On production hosts all of it asks you to type the
  host's name.

## 0.13.1 — 2026-10-05

- **The release that was meant to be 0.13.0.** A code-style check failed in the
  build for 0.13.0, so that version was never published. 0.13.1 is the same
  Databases view (MySQL, MariaDB, PostgreSQL) and Containers view (Docker,
  Podman, nerdctl), with that one line fixed.

## 0.13.0 — 2026-10-05

- **Databases.** A new view for MySQL, MariaDB and PostgreSQL: saved connections (the
  password lives in the vault), a tree of databases, tables, columns and
  indexes, a query editor with history, and results that stay fast with
  100,000 rows. Connect directly or through one of your SSH hosts, so the
  database port is never exposed. Cancel a running statement, export as
  CSV, TSV or JSON, and edit a cell in place after seeing the exact
  `UPDATE`. Destructive statements ask first, and on production
  connections you type the connection's name. Query history stays on this
  computer. PostgreSQL connections show a database's schemas, check TLS
  certificates against your system's trust store, and apply edits to any
  column type. A run that opens a transaction without ending it is refused,
  since the next run can't continue it.

- **Containers.** A new view that lists the containers and images of Docker,
  Podman or nerdctl, on this computer or on any saved host, over your
  existing SSH access with nothing installed on the host. Search and filter,
  start, stop, restart and remove (removing always asks; on production hosts
  you type the host's name), follow a container's logs with search and copy,
  open a shell in it as a normal terminal tab, and inspect it as JSON. The
  list refreshes by itself and can be paused.

## 0.12.0 — 2026-10-04

- **Host monitoring and a Fleet view.** Turn on monitoring for a host (its
  details card, or a tile in the new Fleet view) to see CPU, memory, disk,
  load and uptime, refreshed every 30 seconds over the same SSH connection
  mechanism as everything else — nothing installed on the host, nothing
  sent anywhere. Fleet shows a tile per host with reachability
  colour-coding and its own auto-refresh; open it from the activity icon
  next to a group in the host list, or the command palette. Off by
  default, per host; CPU and memory aren't sampled on BSD/macOS yet.
- **A changelog, in the app itself.** Settings → Updates → "View
  changelog", or the command palette. After an update actually takes
  effect, the next unlock shows a one-time "Updated to vX.Y.Z" notice
  with a shortcut straight to it.

## 0.11.0 — 2026-10-04

- **The host form is now four tabs** — Connection, Route, Organise,
  Automation — instead of one long scroll, with a **Test connection**
  button, a "no colour" option alongside the colour swatches, and a list
  of the host's tunnels with an auto-start checkbox.
- **Settings has a category list and a search box** that finds a setting
  by name across every category, instead of one long page.
- **A live preview** next to the terminal theme gallery shows your font,
  colours and cursor settings on sample text as you change them. New
  settings: letter spacing, padding, minimum contrast, cursor colour and
  bold-as-bright.
- **SFTP**: the two panes can be resized by dragging the divider between
  them, columns are sortable, right-click a file for a full menu (open,
  upload/download, copy path, rename, permissions, delete), the path is
  a clickable breadcrumb, and there's a per-folder filter box.
- **A first-run card** after creating a new vault offers to import your
  hosts, add one by hand, or quick connect, with a short tour of the
  activity bar, palette and shortcuts.
- **The unlock screen** now lists recently opened vaults for one-click
  access, and the recovery-key reminder can no longer be dismissed by
  accident — you have to tick the box that you've saved it.
- **Reopen your tabs automatically** after unlocking, picking up where
  you left off (switchable in Settings; production hosts stay closed by
  default so you reconnect them on purpose).
- **Drag a file onto a terminal** to upload it over SFTP to that pane's
  current directory, or drag text to paste it.
- **Ctrl+click a file path** in terminal output to browse to it in SFTP,
  or a `host:port` to quick connect to it.
- **A new Security review page** (under Vault) flags certificates nearing
  expiry, weak or deprecated keys, keys over five years old, hosts using
  a password when a key is available, and passwords that haven't changed
  in a while.
- **A local connection log** (Vault → Activity, and on each host's
  details card) shows when you connected, for how long, and how it
  ended. Never synced.
- **Pasting something that looks like a private key or an API token**
  now asks you to confirm first.
- Smaller terminal additions: a word-separator setting, a screen-reader
  mode toggle, an option to drop one trailing newline from a paste, and
  "Copy entire buffer" in the terminal's right-click menu.

## 0.10.0 — 2026-10-04

- **Escape now closes only the dialog you're looking at**, not the form
  underneath it, and closing a dialog or form with unsaved changes asks
  first.
- **Closing a tab, a pane, or the app** while something is still
  connected now asks first, and says if a command is still running
  (switchable in Settings).
- **A status bar** along the bottom shows the vault lock, SSH agent and
  command-line status, how many tunnels are running, and the active
  pane's directory and terminal size — each one click away from its own
  view.
- **Tabs show an unread-output dot, a spinner for a running command, or a
  bell icon** when something happens in a background tab, and a
  searchable "list all tabs" button appears once there are too many to
  see at once.
- **A pane's header controls** are tidier: the less-common ones (select
  mode, sync typing, record, maximize) live under a single "⋯" menu,
  which also gained Reconnect, Duplicate pane, Open SFTP here, Copy ssh
  command and Swap panes.
- **Split dividers can now be resized from the keyboard**, like the
  sidebar's divider already could, and Alt+Shift+Arrow jumps focus
  between panes in a split.
- **The host list is fully keyboard-operable**: arrow keys move around,
  Left/Right collapses or expands a group, typing jumps to a matching
  name, F2 edits a host and Delete removes it.
- Toasts now stack (instead of replacing each other) and errors stay
  until you dismiss them; colours, button styles and a few other small
  inconsistencies across the app were cleaned up.

## 0.9.9 — 2026-10-03

- **Copying inside tmux, Claude Code and Neovim now reaches your system
  clipboard.** These programs copy by asking the terminal to set the
  clipboard directly (a sequence called OSC 52); it was being silently
  ignored.
- Fixed a related issue where select mode and Shift-drag selection
  stopped working in tmux because the program redrawing the screen kept
  resetting the selection.

## 0.9.8 — 2026-10-01

- **The host form now shows a group's jump host and proxy** when the
  host itself doesn't set one, instead of leaving the field looking
  empty while still using the group's default. You can opt out of either
  with "None, connect directly".
- **Proxies have a Test button** that reaches a real address through the
  proxy and reports success with latency, or the failure reason.
- The copied `ssh` command and the reachability check now account for
  proxies and group-inherited jump hosts correctly.

## 0.9.7 — 2026-09-30

- **Quick connect and the command palette accept an OpenSSH-style
  command line**, e.g. `ssh -J bastion,me@gw:2222 -p 2200 root@db` —
  jump hosts, ports, users and a couple of common options are all
  understood, with the full route shown before you connect.
- Fixed the tab right-click menu (Rename, Duplicate, Close, Close
  others): every action had stopped doing anything.

## 0.9.6 — 2026-09-30

- **Updates now work even without a signed release.** If a build can't
  verify a signed update, it falls back to checking GitHub Releases
  directly, downloading the right installer for how you installed the
  app and verifying its checksum before installing.
- Context menus now flip up or sideways to stay on screen near a window
  edge, instead of running off it.
- New searchable pickers for choosing a host, jump host or group, with
  the option to create a new group by typing its name.
- Dragging the sidebar closed now previews the collapse as you drag,
  only committing when you let go; Escape cancels the drag.

## 0.9.5 — 2026-09-29

- **Command history is off by default.** It used to be saved
  automatically; since typed commands can contain passwords and tokens,
  you now turn it on yourself under Settings if you want it, and turning
  it off deletes what was saved.
- Rewrote the README into a proper product page, and added SECURITY.md
  and CONTRIBUTING.md.

## 0.9.4 — 2026-09-27

- Tagged releases are now published directly (with every installer
  attached) instead of as a draft that needed a manual step.

## 0.9.3 — 2026-09-27

- **The list panel can be resized by dragging its edge**, like a code
  editor's sidebar: drag to the width you want, double-click to reset,
  or drag all the way to the edge to hide it. Works from the keyboard
  too, and the width is remembered.

## 0.9.2 — 2026-09-27

- No user-facing changes — build and CI maintenance only (trimming CI
  disk usage and aligning package versions).

## 0.9.1 — 2026-09-27

- **Confirmation dialogs actually show up now.** The app had been relying
  on the browser's own confirm/prompt, which the packaged app doesn't
  reliably display — so deletes, production warnings, backup restores
  and several other confirmations were silently approved instead of
  asking. All of them now use the app's own dialogs.

## 0.9.0 — 2026-09-27

- **Tunnels gained a search box and host/status filters**, and show up
  in the command palette by name, host or port.
- Running a snippet on a production host is now checked for destructive
  commands the same way typing one is.
- Fixed `{{user}}` in snippets not picking up a credential inherited from
  a host's group.
- Ctrl (Cmd on macOS) + scroll now zooms the terminal's font size.

## 0.8.0 — 2026-09-27

- **A scripting command line**: `sshvault status`, `list`, `connect
  <host>`, and `run … -- <command>`, talking to the running app so your
  saved credentials are used without ever passing through the CLI
  itself. Off until you turn it on, and every `run` is approved inside
  the app.
- **Automatic updates**: a start-up check, a progress banner, a signature
  check against the key built into the app, then install and restart.

## 0.7.0 — 2026-09-27

- **A built-in SSH agent backed by the vault**: `ssh`, `git` and your
  editor can use your vault keys with nothing written to disk, approved
  key by key, including over agent forwarding.
- **Telnet and serial consoles** for network gear, alongside SSH.
- **SFTP transfer queue**: two transfers run at a time, the rest wait,
  and each can be paused, resumed, cancelled or retried without starting
  over.
- MobaXterm bookmark import.

## 0.6.0 — 2026-09-26

- **Shell integration**: the pane header shows the remote directory
  (with "browse in SFTP" and "new tab here"), you can jump between
  prompts and copy a command's output, and you're notified when a long
  command finishes in a background tab.
- **Per-host command history** on this computer, offered in the command
  palette.
- **Bulk edit** several hosts at once; a host details card with
  Markdown notes.
- Import PuTTY sessions and CSV host lists (password columns are never
  imported).
- SFTP gained chmod, quick look, a permissions column, and dragging
  files in from your file manager to upload.

## 0.5.0 — 2026-09-26

- **A portable, encrypted vault you sync yourself**: a folder of
  per-record encrypted files that Dropbox, OneDrive, Nextcloud or
  Syncthing can carry without ever seeing your data. A random vault key
  is protected by your master password and an optional recovery key.
- Per-record revisions, automatic conflict merging (with a manual
  resolution screen for anything that can't merge itself), encrypted
  backups and restore, and a safe upgrade from the previous format.
- **Key Manager**, group default credentials and proxies, and server
  keys trusted explicitly and shared through the vault.
- The list panel can be hidden, host groups can be collapsed and
  remembered, and selecting text works again inside tmux and other
  mouse-hungry programs (hold Shift while dragging).

## 0.4.0 — 2026-09-27

- **Local terminal tabs** alongside SSH sessions, with the same splits,
  recording and snippets.
- **Open a whole host group** in its own tabs, or tiled into one tab
  with synchronized typing — like running the same command across a
  cluster.
- **Environment labels** (production/staging/development), with red
  markers and a confirmation before synchronized typing or a multi-host
  run reaches a production host.
- A reachability check (latency and SSH version, no login needed), and
  importing an Ansible inventory or exporting your hosts to an OpenSSH
  config file.

## 0.3.0 — 2026-09-27

- **X11 forwarding** per host, so a remote program's window can show up
  on your screen.
- macOS builds became a single universal app (Apple Silicon and Intel),
  and the release pipeline signs and notarizes builds automatically once
  the certificates are configured.

## 0.2.0 — 2026-09-26

- **Run a command on several hosts at once**, with per-host output,
  exit code and duration.
- **Snippet variables** like `{{host}}` and `{{user}}`, filled in
  automatically or prompted for.
- Split a tab into up to six panes, dragging to resize; splitting a pane
  no longer disconnects the others.
- Show and copy a saved password or key after re-entering your master
  password.
- Edit a remote file in your own editor, uploading it back on save.
- A light theme (or follow your system setting).

## 0.1.1 — 2026-09-26

- Credentials can now be entered directly in the host form.
- Fixed terminal prompts that couldn't be clicked on.

## 0.1.0 — 2026-09-26

- First tagged release: an encrypted, folder-synced vault for hosts,
  identities, snippets and port-forwarding rules; an SSH terminal with
  tabs and splits; a dual-pane SFTP browser; jump-host chains; and a
  command palette.
