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
