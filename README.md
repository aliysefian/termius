# SSHVault

A cross-platform SSH terminal manager and SFTP client for Linux and Windows,
inspired by Termius. Hosts, keys, snippets and port-forwarding rules are
encrypted on your machine and synced between computers through a folder you
already sync, such as Dropbox, Nextcloud or Syncthing. There is no server and no
account: the sync service only ever sees ciphertext.

Built with [Tauri v2](https://tauri.app), Rust, SvelteKit, Tailwind CSS and
[xterm.js](https://xtermjs.org).

- [Features](#features)
- [Install](#install)
- [First run and syncing between computers](#first-run-and-syncing-between-computers)
- [Using SSHVault](#using-sshvault)
- [Development](#development)
- [How it works](#how-it-works)
- [Security model](#security-model)
- [Troubleshooting](#troubleshooting)
- [Project status and limitations](#project-status-and-limitations)

## Features

- **Terminal**: tabs that stay connected in the background, up to six
  resizable split panes per tab, WebGL rendering, copy and paste, find,
  clickable links, nine colour themes, session recording to a file, and
  adjustable font, cursor and scrollback.
- **Command palette** (Ctrl+Shift+P) and keyboard shortcuts for connecting,
  running snippets, splitting, and switching tabs.
- **Built for fleets**: open a whole group tiled with synchronized typing,
  label hosts as production, staging or development with guard rails, run a
  startup command on connect, check which hosts are reachable, and keep
  local shells in tabs next to your SSH sessions.
- **Plays well with your tooling**: import Ansible INI inventories, and
  export every host as an OpenSSH config for `ssh`, `scp`, Ansible and git.
- **Quick connect**: type `user@host:port` to connect without saving a host.
- **Import from `~/.ssh/config`**, including `Host *` defaults, ProxyJump,
  ProxyCommand (off until you approve it), forwards and keep-alive. Key files
  are referenced by path, or copied into the vault if you choose.
- **Authentication**: password (with keyboard-interactive fallback), private
  key in OpenSSH, PEM or PuTTY format with optional passphrase, or your local
  ssh-agent (Unix socket on Linux, OpenSSH agent pipe or Pageant on Windows).
- **Credentials separate from hosts**: one key or password can be shared by
  many hosts, or by a whole group, and rotated in one place.
- **Key Manager**: generate Ed25519, ECDSA or RSA keys, import private and
  public keys, OpenSSH certificates, passphrase changes, "used by", and a
  deliberately guarded private-key export.
- **Jump hosts** (like OpenSSH `ProxyJump`), including chains of several
  hops, plus optional **ssh-agent forwarding** and **X11 forwarding** per
  host.
- **SFTP**: dual-pane browser for this computer and a remote host, with drag
  and drop, multi-select, rename, delete, new folder, recursive, cancellable
  transfers with progress, and editing remote files in your local editor.
- **Port forwarding**: local (`-L`), remote (`-R`) and dynamic SOCKS5 (`-D`)
  rules, saved and synced, started and stopped with one click.
- **Snippets**: saved commands with `{{variables}}`. Run or paste them into
  the active terminal, broadcast to every pane in a tab, or run them on many
  hosts at once in the background and compare each host's output.
- **Portable encrypted vault**: a folder of per-record encrypted files under a
  random vault key that your master password (or recovery key) unlocks.
  Several computers can edit at once: different changes merge, clashing ones
  are flagged and never silently overwritten. Encrypted automatic backups,
  restore, integrity checks, and "remember on this device" through the OS
  keychain.
- **Host tree** with nested groups (with default credential, jump host, proxy
  and environment), favorites, drag and drop between groups, recent
  connections, search, filters, sorting, tags, custom fields and colours.
- **Proxies**: SOCKS5, HTTP CONNECT or an approved ProxyCommand, per host or
  per group.
- **Light and dark themes**, or follow the system setting.
- **Security extras**: server keys trusted explicitly and shared through the
  vault, with old and new fingerprints when one changes; multi-line paste and
  destructive-command confirmation on production hosts; clipboard clearing;
  auto-lock.

## Install

There are no signed releases yet. Every push to `main` builds installers in
GitHub Actions, and you can download them from there:

1. Open the repository's **Actions** tab and select the latest successful
   **Build** run on `main`.
2. Scroll to **Artifacts** and download `sshvault-Linux-…`,
   or `sshvault-Windows-…`. Artifacts are kept for 14 days.
3. Unzip the download and install the package for your system, as below.

### Linux

Pick one of the three formats.

```sh
# Debian, Ubuntu, Mint, Pop!_OS
sudo apt install ./SSHVault_*_amd64.deb

# Fedora, RHEL, openSUSE
sudo dnf install ./SSHVault-*.x86_64.rpm

# Any distribution, no install needed
chmod +x SSHVault_*_amd64.AppImage
./SSHVault_*_amd64.AppImage
```

The packages are built on Ubuntu 22.04, so they need glibc 2.35 or newer.

### Windows

Run either the `.msi` installer or the `SSHVault_*_x64-setup.exe` installer.
The installers are not code-signed yet, so Windows SmartScreen may warn you.
Choose **More info**, then **Run anyway**.

SSHVault uses the Microsoft Edge WebView2 runtime, which is already installed
on Windows 10 and 11. The installer fetches it if it is missing.

## First run and syncing between computers

**On the first computer:** start SSHVault and choose **Create New Vault**.
Give the vault a name, choose where to put it (a folder your sync service
syncs, such as `~/Dropbox`), and pick a master password. Keep the **recovery
key** it shows you somewhere safe and offline: it's the only way back in if
you forget the password. **Import SSH Config** does the same and then imports
your `~/.ssh/config`.

**On every other computer:** wait until the sync client has downloaded the
folder, choose **Open Existing Vault**, pick the same folder and enter the
same master password. Tick **Remember on this device** to unlock without
typing it next time (the vault key goes into the OS credential store, never
the password).

Changes appear on the other computers a moment after the sync client
delivers the files, with no restart or reload needed.

> [!WARNING]
> The master password is never stored anywhere. Without it or the recovery
> key, the vault cannot be opened, by you or by anyone else.

See the [vault user guide](docs/vault-user-guide.md) for backups, conflicts,
moving the vault and more, and the [architecture and threat
model](docs/vault-architecture.md) for how it works.

## Using SSHVault

### Hosts and identities

- Create a host with the **+** button in the **Hosts** view. Enter the
  hostname and port, then choose how to log in under **Credentials**:
  - **Password**: a username and password.
  - **SSH key**: a username and a key. You can generate a new Ed25519 key,
    load a key file, paste a key, or use ssh-agent. A generated key's public
    half is shown after saving, ready for `~/.ssh/authorized_keys`.
  - **Credential**: reuse a credential that several hosts share.
  - **Ask**: save nothing and prompt at every connection. The prompt has a
    "Remember for this host" option.
- Credentials entered in a host's form belong to that host. They are listed
  under **Credentials → Saved with hosts** and are removed with the host. Tick
  "Also save to Credentials" in the form to make them shareable instead.
- When editing a host, leave the password or key empty to keep the saved one.
- To share one login across many hosts, create it under **Credentials**
  and pick it in each host's form, or set it as the default of their group
  under **Groups and proxies**.

### Viewing and copying saved passwords

Saved passwords, private keys and passphrases stay hidden until you confirm
your master password:

- In **Credentials**, hover an entry and press the eye icon to show its
  secret, or the copy icon to copy the password directly. Credentials saved
  with a host are under **Saved with hosts**.
- In the host and identity forms, press **Reveal saved** next to a stored
  password or key.

After a correct master password, you can reveal more secrets for 2 minutes
without typing it again. Locking the vault ends that window. Five wrong
attempts block reveals for 30 seconds, and the wait doubles with each further
round of failures. Shown secrets hide themselves after a minute. Copied
secrets are cleared from the clipboard after 30 seconds, if nothing else has
been copied since.
- Put hosts in nested groups by typing a path such as `Production/Databases`
  in the **Group** field.
- **Double-click** a host to open it in a new terminal tab. Hosts you
  connected to recently appear at the top of the tree.
- Hover a host to duplicate, edit or delete it.
- Drag a host onto a group to move it there, or onto empty space to move it
  to the top level.
- **Forward ssh-agent** in the host form lets that server use the keys in
  this computer's agent, for example to reach another server or a git
  remote. Anyone with root on the server can use your agent while you're
  connected, so only enable it for servers you trust.
- **Forward X11** shows the server's graphical programs on this computer.
  You need an X server here: most Linux desktops have one, macOS needs
  XQuartz, and Windows needs VcXsrv or X410. The server needs `xauth` and
  `X11Forwarding yes`. As with `ssh -X`, the server only ever gets a random
  stand-in cookie. SSHVault checks it on every connection and swaps in your
  real X cookie locally, so the real one never leaves your computer.

### Working with many servers

- **Open a whole group.** Hover a group in the host tree. The list icon opens
  every host in its own tab. The grid icon tiles up to six of them in one tab
  with **synchronized typing** turned on, so each keystroke goes to every
  pane, like `tmux synchronize-panes` or cluster SSH.
- **Synchronized typing** can be switched on for any split tab with the
  keyboard icon in a pane header, or **Ctrl+Shift+B**. Synced tabs show a
  yellow **SYNC** badge and a yellow outline.
- **Environments.** Set a host's environment to Production, Staging or
  Development in its form. Production hosts get a red **PROD** badge in the
  tree, the tab and the pane header. SSHVault asks before synchronized typing
  or "Run on hosts" touches a production host.
- **Run after connecting.** A host's startup command, such as `sudo -i`,
  `cd /srv/app` or `tmux attach -t main`, is typed for you once the shell is
  ready. Snippet variables like `{{user}}` work there too.
- **Reachability.** The pulse icon at the top of the host list opens a TCP
  connection to every host's SSH port, 32 at a time, without logging in. Each
  host shows its latency or "down". Hover it to see the server's version
  line, such as `SSH-2.0-OpenSSH_9.6p1`, handy for spotting outdated servers.
  Hosts behind a jump host are marked "jump" rather than probed.
- **Local terminals.** **Ctrl+Shift+`**, or the terminal icon next to the tabs,
  opens your own shell in a tab: your login shell on Linux and macOS, the
  default shell on Windows. Local tabs work with splits, recording, snippets
  and synchronized typing like SSH tabs.

### Importing from an Ansible inventory

In the import dialog, choose **Ansible inventory…** and pick an INI
inventory. SSHVault reads groups, `[group:vars]`, `[group:children]`,
`[all:vars]`, host ranges like `web[01:03]`, and the variables
`ansible_host`, `ansible_port`, `ansible_user` and
`ansible_ssh_private_key_file`, including their older `ansible_ssh_*` names.
A `ProxyJump` or `-J` in `ansible_ssh_common_args` becomes the jump host.
Hosts land in folders that mirror the group tree, such as `prod/web`.

### Exporting to `~/.ssh/config`

**Settings → Use your hosts from the command line** saves every host as an
OpenSSH config file, or copies it. Add `Include /path/to/sshvault.config` to
`~/.ssh/config`, and `ssh web-1` works from any shell with the same names,
users, ports and jump hosts. Private keys stay in the vault, so the file has
no `IdentityFile` lines, and `ssh` uses your agent or default keys.

### Importing from `~/.ssh/config`

Click the import icon at the top of the host list, or run "Import hosts from
~/.ssh/config" from the command palette. SSHVault shows a preview of every
host it found. Pick the ones you want and choose a group.

- `HostName`, `User`, `Port`, `IdentityFile`, single-hop `ProxyJump`,
  `ProxyCommand`, `ForwardAgent`, `ForwardX11`, `LocalForward`,
  `RemoteForward`, `DynamicForward` and `ServerAliveInterval` are imported,
  with `Host *` defaults applied the same way `ssh` applies them.
- Key files are **referenced by path** by default. Choose **Copy into the
  vault** to store them encrypted in the Key Manager so they sync.
- ProxyCommands are imported switched off; they never run until you approve
  them under **Groups and proxies**. Forwards become tunnels that don't
  start automatically.
- Hosts that already exist are skipped. `Match`, `Include` and multi-hop
  `ProxyJump` are reported as warnings instead of imported.

### Keys

The **Keys** screen is the Key Manager: generate Ed25519, ECDSA or RSA keys,
import private keys (OpenSSH, PEM, PKCS#8, PuTTY) or colleagues' public
keys, copy a public key for `~/.ssh/authorized_keys`, attach an OpenSSH
certificate, change a passphrase, and see which credentials and hosts use a
key. Credentials refer to keys, so replacing a key applies everywhere.
Exporting a private key asks for the master password every time and writes an
owner-only file.

### SSH agent

**Keys → SSH agent → Turn on** starts an SSH agent backed by the vault. Copy
the `SSH_AUTH_SOCK` line it shows into your shell profile, and `ssh`, `git`,
`scp` and editors use your vault keys directly. No private key file is ever
written.

- Keys are offered only after you switch them on, one by one: **Ask every
  time** (the app asks before each signature) or **Allow while unlocked**.
- The agent stops when the vault locks, and starts again at the next unlock
  if it's turned on for that computer.
- It only lists keys and signs. `ssh-add` can't add, remove or lock keys in
  it.
- Encrypted keys need their passphrase saved in the vault to be offered,
  because the agent can't ask for it.

### Telnet and serial consoles

For switches, routers and devices without SSH:

- **Telnet**: set a host's protocol to Telnet, or type `telnet://host:port`
  in Quick connect. You log in inside the terminal and nothing is saved for
  it. Telnet is unencrypted, and its panes say so.
- **Serial**: Quick connect → **Serial console…** (or the command palette).
  Pick the port, baud rate and line settings (9600 8N1 by default). On
  Linux your user may need to be in the `dialout` group.

### Jump hosts

In the host form, pick another saved host under **Jump host**. The form shows
the full route, such as `bastion → gateway → this host`. Terminals, SFTP and
port forwarding all go through the chain.

Every jump host needs an identity attached, because only the final host can
ask you for one-time credentials. SSHVault refuses to save a chain that loops.

### Terminals

- Split any pane right or down with the buttons in its header, up to six
  panes per tab. Drag the line between panes to resize them.
- Drag tabs to reorder them.
- The record button in a pane's header saves everything the session prints
  to a file you choose, until you press it again. Logs are plain text by
  default. Under **Settings** you can keep colours and control codes instead,
  so `cat` replays the session.
- Double-click a tab to rename it. Right-click it to duplicate it or close
  other tabs. Middle-click to close it.
- The dot on each tab shows its connection: green connected, yellow
  connecting, red failed, grey closed.
- Right-click inside a terminal for copy, paste, select all, find and clear.
- If the connection drops, a **Reconnect** button appears at the bottom of the
  pane.
- Change the colour theme, font, cursor and scrollback under **Settings**.
  These settings are per computer and are not synced.

### Quick connect and the command palette

Press **Ctrl+Shift+T**, or the **+** next to the tabs, and type
`user@host` or `user@host:port`. Leave the password empty to use ssh-agent.
Nothing is saved.

Press **Ctrl+Shift+P** to open the command palette. It searches hosts, recent
connections, snippets and actions. Typing `user@host` there offers a quick
connection too. Outside a terminal, **Ctrl+K** also opens it.

### Keyboard shortcuts

| Keys | Action |
|---|---|
| Ctrl+Shift+P | Command palette |
| Ctrl+Shift+T | Quick connect |
| Ctrl+Shift+W | Close tab |
| Ctrl+Tab, Ctrl+Shift+Tab | Next or previous tab |
| Ctrl+PageDown, Ctrl+PageUp | Next or previous tab |
| Ctrl+1 … Ctrl+9 | Go to tab 1 to 9 |
| Ctrl+Shift+D, Ctrl+Shift+E | Split right, split down |
| Ctrl+Shift+F | Find in terminal |
| Ctrl+Shift+C, Ctrl+Shift+V | Copy, paste |
| Shift+drag | Select text while a program (tmux, vim…) is using the mouse |
| Ctrl+=, Ctrl+-, Ctrl+0 | Zoom in, out, reset |
| Ctrl+Shift+L | Lock vault |
| Ctrl+Shift+B | Type into all panes of the tab (toggle) |
| Ctrl+Shift+H | Hide or show the list panel |
| Ctrl+Shift+Enter | Maximize or restore the active pane |
| Ctrl+Shift+U | Focus mode: only the terminal |
| Ctrl+Shift+/ | Keyboard shortcuts (and rebinding them) |
| Ctrl+Shift+↑ / ↓ | Previous / next prompt (with shell integration) |
| Ctrl+Shift+` | New local terminal |

Plain Ctrl shortcuts such as Ctrl+W, Ctrl+T and Ctrl+K go to the remote
shell, where editors and readline use them.

### Snippets

Add snippets under **Snippets**. To use one, either:

- Hover over it in the Snippets panel and click **Run** or **Paste**.
- Click the **`</>`** button at the right of the terminal tab bar, search, then
  run or paste. In a split tab, tick **Send to every pane in this tab** to
  broadcast.

Paste sends the text without pressing Enter, so you can edit it before running.
Snippets also appear in the command palette.

#### Variables

Snippets can contain placeholders in double braces:

| Placeholder | Filled with |
|---|---|
| `{{host}}` | The host's label |
| `{{hostname}}` | Its address |
| `{{port}}` | Its port |
| `{{user}}` | The username it logs in with |
| `{{date}}`, `{{time}}` | Today's date and the current time |
| Any other name, like `{{lines}}` | A value you're asked for when the snippet runs |

When a snippet runs on several panes or hosts, each gets its own `{{host}}`,
`{{user}}` and so on. Write `{{{{` for a literal `{{`. Typed values are
remembered until you quit, and never written to disk.

#### Running on several hosts

Choose **Run on several hosts** on a snippet, or "Run a command on several
hosts" in the command palette. Pick hosts, adjust the command if needed, and
run it. Up to eight hosts run at a time, each with a timeout. Every host's
exit code, duration and output appear in the results, and you can copy any of
them. Hosts need saved credentials to run unattended.

### SFTP

Open the **SFTP** view. The left pane is this computer and the right pane is
the remote host.

1. Choose a host at the top of the right pane and click **Connect**.
2. Double-click a folder to open it, or type a path and press Enter.
3. Select files with click, **Ctrl**+click or **Shift**+click.
4. Drag files to the other pane, or use **Upload →** and **← Download**.

The eye icon in each pane's toolbar shows or hides dotfiles. Folders are copied recursively.

To edit a remote file, double-click it, or select it and press the edit
button. It opens in this computer's default app for that file type. Each time
you save, it's uploaded back, and the list at the bottom of the SFTP view
shows the last upload. Press **Stop** when you're done. The local copy lives
in a private temporary folder and is deleted when you stop, disconnect, lock
the vault or quit. Symbolic links are listed, but not followed
during recursive copies. Transfers appear at the bottom with progress and a
cancel button.

### Port forwarding

Open the **Port Forwarding** view, add a rule, and press the play button.

| Type | OpenSSH equivalent | What it does |
|---|---|---|
| Local | `ssh -L 127.0.0.1:8080:db:5432 host` | Listens on this computer. Connections are carried to the destination from the server. |
| Remote | `ssh -R 127.0.0.1:9000:localhost:3000 host` | The server listens. Connections come back and are dialled from this computer. |
| Dynamic | `ssh -D 127.0.0.1:1080 host` | A SOCKS5 proxy on this computer that exits through the server. |

Set the port to `0` to have a free port chosen for you. The chosen port is
shown under the rule. A green dot means the rule is running. Errors, such as a
port already in use, are shown under the rule.

Rules are synced like hosts, but running state is not: each computer starts
its own rules. Tick **Start automatically** to start a rule every time the
vault is unlocked, on every computer. The rule's host needs an identity,
because forwarding runs without prompting you.

### Locking

The lock icon at the bottom of the sidebar, or **Ctrl+Shift+L**, closes every
terminal, SFTP session and forwarding rule, removes temporary copies of
remote files, and wipes the key from memory. Under **Settings → Auto-lock**
you can lock automatically after 5, 15, 30 or 60 minutes without keyboard
or mouse activity.

### Known hosts

The first connection to a server shows its fingerprint and asks whether to
trust it. Trusted keys are stored in the vault, so your other computers
don't ask again. If a key changes, the connection stops and you see the old
and new fingerprints; replacing the key needs you to type `replace`. The
**Known hosts** screen lists every trusted key with its history, and can
import your existing `~/.ssh/known_hosts`.

## Development

### Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | stable | Install with [rustup](https://rustup.rs). |
| Node.js | 22 or newer | |
| pnpm | 12.4.1 | Pinned in `package.json`. Run `corepack enable` and the right version is used automatically. |

Plus the Tauri system dependencies for your OS.

**Debian and Ubuntu:**

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**Fedora:**

```sh
sudo dnf check-update
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel
sudo dnf group install "c-development"
```

**Arch Linux:**

```sh
sudo pacman -Syu
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg xdotool
```

**Windows:**

1. Install [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   and select **Desktop development with C++**.
2. Install Rust with rustup and keep the default MSVC toolchain.
3. WebView2 is already present on Windows 10 and 11.

For other distributions, see the
[Tauri prerequisites guide](https://tauri.app/start/prerequisites/).

### Get the code and run it

```sh
git clone git@github.com:aliysefian/termius.git sshvault
cd sshvault
corepack enable
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` starts the Vite dev server on port 1420, compiles the Rust
backend, and opens the app window. Frontend edits reload instantly. Rust
edits trigger a rebuild and restart. The first Rust build takes a few
minutes.

> [!TIP]
> Use a throw-away vault folder while developing, for example
> `/tmp/sshvault-dev`, so you don't touch your real vault.

### Tests and checks

```sh
pnpm check                                 # Svelte and TypeScript type-check
pnpm test                                  # Frontend unit tests (Vitest)
cd src-tauri
cargo test                                 # Rust unit and end-to-end tests
cargo clippy --all-targets -- -D warnings  # Rust lints, warnings are errors
```

The SSH, SFTP and port-forwarding tests are end-to-end. They start a
temporary, unprivileged `sshd` on a random local port with throw-away keys,
and remove it afterwards. They need OpenSSH server installed
(`sudo apt install openssh-server`), though the service does not need to be
running. Without `sshd`, those tests print `skipping` and pass.

### Build installers

```sh
pnpm tauri build
```

Output goes to `src-tauri/target/release/bundle/`:

- Linux: `deb/`, `rpm/`, `appimage/`
- Windows: `msi/`, `nsis/`

### Code signing

Builds are unsigned until you add signing secrets to the GitHub repository
(**Settings → Secrets and variables → Actions**). The workflow picks them up
by itself. Without them, it keeps building unsigned installers.

Windows signing needs a code-signing certificate as a `.pfx` file:

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | The `.pfx`, base64-encoded |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |

To base64-encode it, run
`[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))` in
PowerShell, or `base64 -w0 cert.pfx` on Linux.

### Continuous integration

`.github/workflows/build.yml` runs on pushes to `main`, pull requests, `v*`
tags, and manual dispatch. On Ubuntu 22.04 and Windows it type-checks and
tests the frontend, runs clippy and the Rust tests, builds the installers, and
uploads them as artifacts. The end-to-end SSH tests run on Linux. Windows has
no `sshd`, so they skip themselves there. Set `SSHVAULT_SKIP_SSHD_TESTS=1` to
skip them anywhere else.

Pushing a tag such as `v0.2.0` also creates a **draft** GitHub Release with
both platforms' installers attached. Review the draft on GitHub and publish it
when ready.

### Project structure

```text
.
├── src/                        SvelteKit frontend
│   ├── lib/
│   │   ├── api.ts              Typed IPC wrappers for vault records
│   │   ├── ssh.ts              Terminal session IPC
│   │   ├── sftp.ts             SFTP, local files and transfer IPC
│   │   ├── types.ts            Types mirroring the Rust models
│   │   ├── tree.ts             Builds the host tree from group paths
│   │   ├── guard.ts            Paste and destructive-command checks
│   │   ├── stores/             Svelte 5 rune stores (vault data, UI state)
│   │   └── components/         UI components
│   └── routes/+page.svelte     App shell
├── src-tauri/                  Rust backend
│   ├── src/
│   │   ├── crypto.rs           Argon2id, XChaCha20-Poly1305, key wrapping, recovery keys
│   │   ├── vault/              Portable vault: format and key slots, records and
│   │   │                       revisions, merge, conflicts, atomic writes, backups,
│   │   │                       integrity, devices and locks, v1 migration
│   │   ├── keychain.rs         "Remember on this device" (OS credential store)
│   │   ├── models.rs           Host, Identity, SshKey, HostGroup, Proxy, KnownHost, …
│   │   ├── keymanager.rs       Key Manager, group defaults, connection targets
│   │   ├── hostkeys.rs         Trusted server keys in the vault
│   │   ├── dial.rs             TCP, SOCKS5, HTTP CONNECT and ProxyCommand
│   │   ├── config.rs           Per-machine settings (vault path, device)
│   │   ├── sync.rs             File watcher for changes from other computers
│   │   ├── session.rs          Unlock, lock, key lifetime
│   │   ├── ssh.rs              Connections, jump chains, terminal sessions
│   │   ├── sftp.rs             SFTP browsing and transfers
│   │   ├── forward.rs          Local, remote and SOCKS5 forwarding
│   │   ├── runner.rs           Run a command on many hosts
│   │   ├── remoteedit.rs       Edit remote files in a local editor
│   │   ├── sessionlog.rs       Session recording and escape stripping
│   │   ├── hostcreds.rs        Credentials entered in the host form
│   │   ├── reveal.rs           Master-password gate for showing secrets
│   │   ├── x11.rs              X11 forwarding with cookie substitution
│   │   ├── localpty.rs         Local shell tabs (PTY / ConPTY)
│   │   ├── health.rs           Reachability and SSH banner checks
│   │   ├── ansible.rs          Ansible INI inventory import
│   │   ├── sshconfig.rs        ~/.ssh/config import
│   │   ├── keys.rs             Key generation, import, passphrases, certificates
│   │   ├── knownhosts.rs       OpenSSH known_hosts files (import, tests)
│   │   ├── commands.rs         Tauri IPC commands and events
│   │   └── lib.rs              App entry point, command registration
│   ├── capabilities/           Tauri permission sets
│   └── tauri.conf.json         Window, bundle and build settings
└── .github/workflows/build.yml CI
```

The Rust engine modules (`ssh.rs`, `sftp.rs`, `forward.rs`, `vault/` and the
rest) do not depend on Tauri. `commands.rs` is the only layer that adapts them
to IPC, which keeps the engine testable without a window.

### IPC overview

The frontend calls Rust with `invoke`. Rust pushes updates back in two ways:

| Channel | Payload | Used for |
|---|---|---|
| Tauri channel per terminal pane | raw bytes | Terminal output |
| Tauri channel per transfer | progress JSON | SFTP transfer progress |
| `vault:changed` event | changed record | Records synced in from other computers |
| `ssh:status` event | pane id and status | Connecting, connected, new host key, errors |
| `forward:status` event | rule id and status | Forwarding rule state |

To connect, the frontend sends only a host id. The backend decrypts that
host's identity and any jump-host identities itself, so terminal, SFTP and
forwarding connections never send credentials through the webview.
Identity lists and live sync events are redacted too: they carry the label,
username and auth type, but no password, key or passphrase. The only way a
secret reaches the webview is a reveal, which the Rust side allows only after
the master password is re-entered (see `reveal.rs`).

### Type-checking the Tauri crate without GTK

If you can't install the WebKit and GTK development packages, you can still
type-check the whole crate, including `commands.rs`, by targeting Windows with
Zig as the C compiler:

1. `rustup target add x86_64-pc-windows-gnu`, and install Zig
   (`pip install ziglang` works without root).
2. Put an `x86_64-w64-mingw32-gcc` script on your `PATH` that runs
   `zig cc -target x86_64-windows-gnu "$@"` after dropping any `--target=…` and
   `-gdwarf-2` arguments.
3. Put an `x86_64-w64-mingw32-windres` stub on your `PATH` that writes
   `!<arch>` plus a newline to the file named by `--output` or `-o`.
4. Run `cargo clippy --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
   in `src-tauri`.

This only checks the code. It does not produce a working binary.

## How it works

The full design is in [docs/vault-architecture.md](docs/vault-architecture.md).
In short:

```text
<your synced folder>/MySSHVault/
├── vault.json            Format version, vault ID and wrapped keys. No secrets, no names.
├── hosts/<uuid>.enc      One encrypted file per record (also identities/, keys/,
├── …                     groups/, known_hosts/, proxies/, snippets/, forwards/, …)
└── backups/*.enc         Encrypted backups
```

- **Keys.** A random 256-bit vault key encrypts every record. `vault.json`
  holds it wrapped under a key derived from your master password with
  Argon2id (64 MiB, 3 passes, 4 lanes), and optionally under your recovery
  key. Changing the password rewraps the vault key; records don't change.
- **Encryption.** XChaCha20-Poly1305 with a random 192-bit nonce per write.
  Each record is bound to its vault, collection and ID, so edited, renamed
  or transplanted files fail to decrypt.
- **One file per record, with revisions.** Computers editing different
  records never touch the same file. Edits to the same record carry the
  revision they started from: different fields merge, clashing fields are
  refused with a conflict instead of being overwritten, and sync-service
  "conflicted copies" are merged or shown for you to resolve.
- **Atomic writes.** Temp file, fsync, read-back check, rename, directory
  fsync. A crash or full disk leaves the old version intact.
- **Live sync.** A file watcher decrypts records as the sync client delivers
  them and updates the interface.

Per-computer settings (the vault path, this device's name and ID, and
whether the vault is remembered) live in `config.json` in the OS config
directory: `~/.config/com.aliyousefian.sshvault/` on Linux,
`%APPDATA%\com.aliyousefian.sshvault\` on Windows.

## Security model

- **What the sync service sees.** File names (random UUIDs), how many
  records of each type exist, their sizes and modification times. It cannot
  read host names, users, addresses, groups, keys, passwords, snippets or
  rules, and it never holds anything that unlocks the vault.
- **Not protected:** a computer that is compromised while the vault is
  unlocked. Malware running as you can see what you see.
- **In memory.** The vault key is wiped when the vault locks. Credentials,
  keys and proxy passwords stay on the Rust side; the interface only gets
  redacted copies unless you reveal a secret with the master password.
- **Remember on this device** stores the vault key (never the password) in
  the OS credential store. If that store is unavailable, the app asks for
  the password; it never falls back to storing the key elsewhere.
- **Backups** are encrypted with the vault key and checked completely before
  a restore, which itself takes a safety backup first.
- **Old versions** of SSHVault refuse newer vault formats instead of
  damaging them. Upgrading keeps an untouched copy of the old files.

## Troubleshooting

**`pnpm tauri dev` fails with a `webkit2gtk-4.1` or `gdk-3.0` pkg-config error.**
The Tauri system libraries are missing. Install the packages listed under
[Prerequisites](#prerequisites).

**The terminal is blank or garbled on Linux.**
Some GPU and driver combinations misbehave with WebGL in WebKitGTK. Try
starting with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. xterm.js falls back to its
slower DOM renderer when WebGL is unavailable.

**The terminal says the host key changed.**
The server presented a different key from the one trusted before. If you
expected this, for example because the server was reinstalled, reconnect
and replace the key after comparing fingerprints. If you didn't expect it,
don't connect.

**"master password is incorrect" on a second computer.**
Check that the sync client has finished downloading `vault.json`, and that
you chose the same folder as on the first computer.

**Changes from another computer don't appear.**
Check that the sync client is running and has finished syncing. Comparing
the **Revision** value on each computer's **Vault** screen tells you whether
they hold the same data.

**"Another device changed the same item" when saving.**
Someone (or you, on another computer) edited the same field before the
change synced. The current version is now shown; make your change again.

**A forwarding rule shows "cannot listen on …".**
Another program is using that port. Pick another port, or use `0` for a free
one.

**ssh-agent authentication fails with "no ssh-agent available".**
On Linux, `SSH_AUTH_SOCK` must be set in the environment SSHVault was started
from. On Windows, start the **OpenSSH Authentication Agent** service or
Pageant.

## Project status and limitations

SSHVault is young software. The engine is covered by end-to-end tests against
a real OpenSSH server, but the desktop app has had little real-world use.

Known limitations:

- Installers are unsigned until signing secrets are added (see
  [Code signing](#code-signing)), so Windows warns on first launch.
- macOS is not built or tested. The code is portable, but there is no CI job
  or installer for it.
- Mosh is not supported.

See [ROADMAP.md](ROADMAP.md) for planned work.
- The SFTP pane can reach any path your user account can, since the app is a
  full file manager for this computer.

## License

Released under the [MIT License](LICENSE).
