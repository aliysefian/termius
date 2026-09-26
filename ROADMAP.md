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

## Needs a decision

These are blocked on something only the project owner can provide, or are
large enough to plan separately.

- **Code-signed installers.** Windows needs an Authenticode certificate or an
  Azure Trusted Signing account. macOS needs an Apple Developer ID (paid
  yearly) plus notarization. Both are recurring costs and account setup, and
  once the credentials exist in CI secrets, the workflow change is small.
- **X11 forwarding.** Needs a local X server, which Windows and macOS don't
  ship. Low demand for a terminal manager.
- **Mosh.** There is no maintained Rust Mosh client, so this means
  implementing the SSP protocol from scratch. A project of its own.
- **Intel Mac build.** A second macOS job on an Intel runner, if anyone needs
  it.
