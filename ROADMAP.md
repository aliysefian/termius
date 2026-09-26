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

## Later

These are larger pieces of work, not started yet.

- Run a snippet on several hosts in the background and collect the output.
- Snippet variables, such as `{{host}}` or prompted values.
- More than two panes per tab, and drag to reorder tabs.
- Drag hosts between groups in the tree.
- Edit remote files from the SFTP pane in a local editor.
- Session logging to a file.
- Light theme.
- macOS build, and code-signed Windows installers.
- X11 forwarding and agent forwarding.
- Mosh support.
