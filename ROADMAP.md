# Roadmap

Work items for making SSHVault safer, faster to use, and more polished.
Checked items are done. The "Later" section lists larger items that are not
started.

Also fixed along the way: switching terminal tabs used to unmount the other
tabs' terminals and drop their SSH sessions. All tabs now stay connected in
the background.

## Security

- [x] **Keep identity secrets out of the webview by default.** Lists and live
  sync events carry identities without passwords, keys or passphrases. The
  full secret is fetched only when you open an identity to edit it.
- [x] **Auto-lock after inactivity**, with a per-computer setting.
- [x] **Password strength meter** when creating a vault.
- [x] **Known hosts manager** in Settings: list pinned server keys with
  fingerprints and remove entries.
- [x] **Changed host key flow**: the terminal shows the new fingerprint and a
  deliberate "Trust new key" action instead of a raw error.
- [x] **Purge old tombstones** automatically on unlock (older than 90 days).

## Terminal

- [x] **Copy and paste**: Ctrl+Shift+C / Ctrl+Shift+V, right-click menu, and
  optional copy-on-select, through the system clipboard.
- [x] **Find in terminal** (Ctrl+Shift+F) with next/previous.
- [x] **Appearance settings**: colour theme, font, font size, cursor style,
  cursor blink, scrollback length. Applied live to open terminals.
- [x] **Built-in colour themes**: SSHVault, Dracula, Nord, Solarized Dark,
  Gruvbox Dark, Tokyo Night, One Dark.
- [x] **Tab status**: connection dot per tab, host colour accent, remote
  window title shown in the pane header.
- [x] **Tab actions**: rename (double-click), duplicate, close others.

## Speed and navigation

- [x] **Command palette** (Ctrl+Shift+P, or Ctrl+K outside a terminal): connect to hosts, run snippets, switch
  views, create hosts, lock the vault.
- [x] **Keyboard shortcuts**: new connection, close tab, next/previous tab,
  split right/down, find, palette.
- [x] **Quick connect**: type `user@host:port` and connect without saving a
  host, using a password or ssh-agent.
- [x] **Recent connections** at the top of the host tree and palette.
- [x] **Duplicate host** action.

## Onboarding

- [x] **Import from `~/.ssh/config`**: preview, pick hosts, import. Maps
  HostName, User, Port, IdentityFile and single-hop ProxyJump, and applies
  `Host *` defaults with OpenSSH's first-match-wins rules.
- [x] **Generate SSH keys** (Ed25519) from the identity form.
- [x] **Copy public key** for any key-based identity, ready for
  `authorized_keys`.

## Port forwarding and SFTP

- [x] **Auto-start rules** when the vault is unlocked.
- [x] **Show hidden files** toggle in SFTP panes.

## Delivery

- [x] **Draft GitHub Release on `v*` tags** with the Linux and Windows
  installers attached, published by hand after review.

## Round two

- [x] **Run a snippet on several hosts** in the background, up to eight at a
  time, with per-host output, exit code, duration and cancel.
- [x] **Snippet variables**: `{{host}}`, `{{hostname}}`, `{{port}}`,
  `{{user}}`, `{{date}}`, `{{time}}`, and prompted values for any other name.
- [x] **Flexible split layout**: up to six panes per tab, split any pane right
  or down, drag dividers to resize.
- [x] **Drag to reorder tabs.**
- [x] **Drag hosts between groups** in the tree.
- [x] **Edit remote files** from the SFTP pane in the local default editor,
  uploading on every save.
- [x] **Session logging** to a file, plain text or raw.
- [x] **Light theme**, or follow the system setting, plus two light terminal
  colour themes.
- [x] **Agent forwarding** per host, refused unless enabled for that host.
- [x] **macOS build** (Apple Silicon, unsigned) in CI and draft releases.

## Round three

- [x] **X11 forwarding** per host, with OpenSSH-style fake cookies: the
  server gets a random cookie, each forwarded connection is checked against
  it, and the real local cookie is substituted on this computer.
- [x] **Intel Macs**: the macOS build is now a universal app.
- [x] **Code-signing pipeline**: CI signs Windows installers and signs and
  notarizes the macOS app automatically once certificates are added as
  secrets (see "Code signing" in the README). Buying the certificates is
  still the owner's decision.

## Round four: for DevOps and senior developers

- [x] **Local terminal tabs** next to SSH sessions (PTY on Linux and macOS,
  ConPTY on Windows).
- [x] **Open a whole group** in tabs, or tiled in one tab.
- [x] **Synchronized typing** across a tab's panes, like cluster SSH.
- [x] **Environment labels** (production, staging, development) with red
  production markers and confirmation before synchronized typing or
  multi-host runs reach production.
- [x] **Startup command** per host, with snippet variables.
- [x] **Reachability check**: TCP latency and SSH server version for every
  host, no login needed.
- [x] **Ansible INI inventory import**, with ranges, group vars, children and
  ProxyJump.
- [x] **Export to OpenSSH config** for use with `ssh`, `scp`, Ansible and git.

## Round five: the portable vault (0.5.0)

- [x] **Portable encrypted vault**: a random vault key wrapped by the master
  password and an optional recovery key, per-record revisions, field-level
  merge, conflict detection, atomic writes, encrypted backups and restore,
  read-only integrity checks, a crash-safe upgrade from the old format, and
  "remember on this device" through the OS credential store.
- [x] **Key Manager**, group defaults, proxies (SOCKS5, HTTP CONNECT,
  approved ProxyCommand), server keys trusted explicitly and shared through
  the vault, richer `~/.ssh/config` import.
- [x] **Guard rails**: multi-line paste confirmation, destructive-command
  warnings and a red banner on production hosts, clipboard clearing.
- [x] **Hideable list panel**, collapse-all groups, favorites, filters and
  sorting, snippet folders and tags, saved workspaces.
- [x] **tmux and friends**: select text while a program has the mouse, and
  scroll full-screen programs with the wheel.

## Round six: daily-driver polish

Small things that make the app feel finished, ordered by how often they
matter in a working day.

- [x] **Undo delete.** Deleting a host, credential, snippet or rule shows a
  toast with **Undo** for eight seconds. The record (including its secrets,
  and a host's own credential) is held in memory on the Rust side, never
  written anywhere, and put back exactly.
- [x] **Maximize a pane** in a split tab (double-click its header or
  Ctrl+Shift+Enter) and restore it; the other panes stay connected.
- [x] **Auto-reconnect** when a connection drops (not after `exit`), up to
  three attempts, switchable in Settings.
- [x] **Copy as `ssh` command** from a host's row, with `-p`, `-J` for the
  jump chain, `-A` and `-X`. Never includes secrets.
- [x] **Last used**: each host remembers when it was last connected to from
  this computer and how often; sort the tree by it.
- [x] **Open in browser** for an active local forward (`http://127.0.0.1:port`).
- [x] **Keyboard cheat sheet** (Ctrl+Shift+/).
- [x] **Remember the window size and position** between launches.
- [x] **Shell integration** (OSC 7 and OSC 133, the VS Code / WezTerm /
  Kitty convention; snippets for bash and zsh under Settings): the pane
  header shows the remote directory with **Browse in SFTP** and **New tab
  here**, Ctrl+Shift+↑/↓ jumps between prompts, and **Copy last command
  output** is in the terminal menu.
- [x] **Command history per host**, kept on this computer only (never in the
  vault), with exit codes when shell integration is on. Recent commands
  appear in the palette and re-run in the right host; clearable in Settings.
- [x] **Notify when a background tab finishes**: a command that ran 8 s or
  longer, or the terminal bell, raises a system notification if you're not
  looking at that pane.
- [x] **Bulk edit**: Ctrl/Shift+click hosts, then change group, environment,
  credential, jump host, proxy or tags for all of them, or open them all.
- [x] **Host details card** (Space on a host, or its info button): route,
  credential, proxy, custom fields, notes rendered as Markdown (HTML is
  escaped), reachability, last connected, recent commands, and the `ssh`
  command.
- [x] **Local terminal profiles**: choose the shell (bash, zsh, fish,
  PowerShell, `wsl`, …) and start folder.
- [x] **Custom terminal themes**: import VS Code, Windows Terminal or iTerm2
  colour schemes; optional red tint on production terminals.
- [x] **Custom keyboard shortcuts**: rebind or unbind any action from the
  cheat sheet (Ctrl+Shift+/).
- [x] **SFTP**: chmod, quick look at text and images (Space), a
  permissions column, drag files in from the OS file manager to upload, and
  "show in file manager" for local files.
- [x] **SFTP transfer queue**: two transfers run at a time and the rest
  wait; each can be paused, resumed or cancelled, and **Retry** continues
  partial files from where they stopped instead of starting over.
- [x] **Migrate from other tools**: PuTTY saved sessions (Windows registry,
  or `~/.putty/sessions`) and any CSV (Termius exports, spreadsheets) with
  column mapping. Password columns are detected and never imported.
- [x] **MobaXterm** import: SSH bookmarks from `MobaXterm.ini` or an
  exported `.mxtsessions` file, with their folders, ports, users and key
  paths. Other bookmark types are listed as skipped.
- [x] **Focus mode** (Ctrl+Shift+U hides everything but the terminal) and a
  **compact density** option.
- [x] **Accessibility pass**: visible keyboard focus rings everywhere,
  reduced-motion support, and every icon button has an accessible name.

## Later: bigger developer features

- [x] **Vault-backed SSH agent.** Serves Key Manager keys on
  `SSH_AUTH_SOCK` (an owner-only Unix socket, or a named pipe on Windows)
  while the vault is unlocked, so `git`, `ssh`, `scp` and IDEs use vault
  keys with no file on disk. Keys are off until switched on, one by one,
  as "ask every time" or "allow". Read-only: it lists and signs, and
  refuses to add, remove or lock keys. Honours RSA SHA-2 requests. Tested
  with real `ssh-add` and `ssh`.
- [x] **Agent forwarding from the vault agent**: while the vault agent is
  on, sessions with "forward ssh-agent" offer vault keys instead of the
  system agent, with the same per-key approval; the prompt names the server
  asking. Tested with a second hop signed through the forwarded agent.
- [ ] **Auto-update** through the Tauri updater. Needs the update-signing key
  pair and a release channel decision.
- [ ] **Hardware keys**: FIDO2 (`sk-ssh-ed25519`) and PKCS#11 keys through
  the local agent; and unlocking the vault with a hardware-backed key.
- [ ] **Team sharing**: a second vault (or a group inside one) encrypted to
  several people's public keys, so a team can share hosts without sharing a
  master password.
- [x] **Serial and Telnet** for network gear: Telnet hosts (saved, or
  `telnet://host:port` in Quick connect) with proper option negotiation,
  window size and terminal type, marked UNENCRYPTED; serial consoles with
  port discovery, baud, data bits, parity, stop bits and flow control.
  Serial is unit-tested but has not been tried against real hardware yet.
- [ ] **Scripting**: a small CLI (`sshvault connect web-01`, `sshvault run
  --group prod "uptime"`) that talks to the running app.

## Needs a decision

- **macOS builds were dropped from CI** at the owner's request, along with
  the macOS signing step. The universal-build and Apple signing setup can be
  recovered from git history (commit d391c53) if macOS is wanted again.
- **Certificates for signing.** The Windows signing pipeline is ready. It
  needs a code-signing certificate, a recurring cost.
- **Mosh.** There is no maintained Rust Mosh client. The options are
  implementing the SSP protocol (UDP, AES-OCB, state sync and local echo),
  which is a project of its own, or adding a local terminal that runs the
  system's `mosh` command, which Windows doesn't have.
