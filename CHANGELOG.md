# Changelog

Every notable change, newest first. Dates are when the release was tagged.
This file is also shown in the app itself, under **Settings → Updates →
View changelog**.

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
