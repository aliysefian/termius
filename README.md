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

- **Terminal**: tabs, split panes (right or down), WebGL-rendered xterm.js,
  256 colours, clickable links, automatic resize.
- **Authentication**: password (with keyboard-interactive fallback), private
  key in OpenSSH, PEM or PuTTY format with optional passphrase, or your local
  ssh-agent (Unix socket on Linux, OpenSSH agent pipe or Pageant on Windows).
- **Identities separate from hosts**: one key or password can be shared by many
  hosts and rotated in one place.
- **Jump hosts** (like OpenSSH `ProxyJump`), including chains of several hops.
- **SFTP**: dual-pane browser for this computer and a remote host, with drag
  and drop, multi-select, rename, delete, new folder, and recursive,
  cancellable transfers with progress.
- **Port forwarding**: local (`-L`), remote (`-R`) and dynamic SOCKS5 (`-D`)
  rules, saved and synced, started and stopped with one click.
- **Snippets**: saved commands you can run or paste into the active terminal,
  or broadcast to every pane in a split tab.
- **Zero-knowledge sync**: every record is its own encrypted file, so editing
  on several computers does not create "conflicted copy" files.
- **Host tree** with nested groups, search, tags and colours.

## Install

There are no signed releases yet. Every push to `main` builds installers in
GitHub Actions, and you can download them from there:

1. Open the repository's **Actions** tab and select the latest successful
   **Build** run on `main`.
2. Scroll to **Artifacts** and download `sshvault-Linux-…` or
   `sshvault-Windows-…`. Artifacts are kept for 14 days.
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

**On the first computer:**

1. Start SSHVault and click **Browse** to choose a vault folder inside your
   synced directory, for example `~/Dropbox/Apps/SSHVault`.
2. The folder is empty, so SSHVault offers to create a vault. Choose a master
   password of at least 8 characters and confirm it.

**On every other computer:**

1. Wait until your sync client has downloaded the vault folder.
2. Start SSHVault, click **Browse**, and choose the same folder.
3. Enter the same master password.

Changes appear on the other computers a moment after the sync client
delivers the files, with no restart or reload needed.

> [!WARNING]
> The master password is never stored anywhere. If you forget it, the vault
> cannot be recovered, by you or by anyone else.

The vault folder path is remembered per computer, since it differs on each
one. You can change it later under **Settings**.

## Using SSHVault

### Hosts and identities

- Create an identity under **Keychain** (the key icon). It holds a username
  and either a password, a private key, or "use ssh-agent".
- Create a host with the **+** button in the **Hosts** view. Set the hostname,
  port and identity. If a host has no identity, SSHVault asks for a username
  and password each time you connect, and does not save them.
- Put hosts in nested groups by typing a path such as `Production/Databases`
  in the **Group** field.
- **Double-click** a host to open it in a new terminal tab.

### Jump hosts

In the host form, pick another saved host under **Jump host**. The form shows
the full route, such as `bastion → gateway → this host`. Terminals, SFTP and
port forwarding all go through the chain.

Every jump host needs an identity attached, because only the final host can
ask you for one-time credentials. SSHVault refuses to save a chain that loops.

### Terminals

- Split a tab with the buttons in a pane's header. A tab holds up to two
  panes.
- Middle-click a tab to close it.
- If the connection drops, a **Reconnect** button appears at the bottom of the
  pane.

### Snippets

Add snippets under **Snippets**. To use one, either:

- Hover over it in the Snippets panel and click **Run** or **Paste**.
- Click the **`</>`** button at the right of the terminal tab bar, search, then
  run or paste. In a split tab, tick **Send to every pane in this tab** to
  broadcast.

Paste sends the text without pressing Enter, so you can edit it before running.

### SFTP

Open the **SFTP** view. The left pane is this computer and the right pane is
the remote host.

1. Choose a host at the top of the right pane and click **Connect**.
2. Double-click a folder to open it, or type a path and press Enter.
3. Select files with click, **Ctrl**+click or **Shift**+click.
4. Drag files to the other pane, or use **Upload →** and **← Download**.

Folders are copied recursively. Symbolic links are listed, but not followed
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
its own rules. The rule's host needs an identity, because forwarding runs
without prompting you.

### Locking

The lock icon at the bottom of the sidebar closes every terminal, SFTP session
and forwarding rule, and wipes the key from memory.

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

### Continuous integration

`.github/workflows/build.yml` runs on pushes to `main`, pull requests, `v*`
tags, and manual dispatch. On Ubuntu 22.04 and Windows it type-checks the
frontend, runs clippy and the Rust tests, builds the installers, and uploads
them as artifacts.

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
│   │   ├── stores/             Svelte 5 rune stores (vault data, UI state)
│   │   └── components/         UI components
│   └── routes/+page.svelte     App shell
├── src-tauri/                  Rust backend
│   ├── src/
│   │   ├── crypto.rs           Argon2id key derivation, XChaCha20-Poly1305
│   │   ├── vault.rs            One-encrypted-file-per-record store
│   │   ├── models.rs           Host, Identity, Snippet, ForwardRule
│   │   ├── config.rs           Per-machine settings (vault path)
│   │   ├── sync.rs             File watcher for changes from other computers
│   │   ├── session.rs          Unlock, lock, key lifetime
│   │   ├── ssh.rs              Connections, jump chains, terminal sessions
│   │   ├── sftp.rs             SFTP browsing and transfers
│   │   ├── forward.rs          Local, remote and SOCKS5 forwarding
│   │   ├── commands.rs         Tauri IPC commands and events
│   │   └── lib.rs              App entry point, command registration
│   ├── capabilities/           Tauri permission sets
│   └── tauri.conf.json         Window, bundle and build settings
└── .github/workflows/build.yml CI
```

The Rust engine modules (`ssh.rs`, `sftp.rs`, `forward.rs`, `vault.rs` and the
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
forwarding connections never send credentials through the webview. The
Keychain screen is the exception: it loads identities, secrets included, into
the webview so you can edit them.

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

### Vault layout

```text
<your synced folder>/
├── vault.json              Plaintext manifest: salt, KDF settings, key check
├── hosts/<uuid>.enc        One encrypted file per host
├── identities/<uuid>.enc   One per identity
├── snippets/<uuid>.enc     One per snippet
└── forwards/<uuid>.enc     One per port-forwarding rule
```

A single database file would be rewritten on every change, so two computers
editing at once would produce "conflicted copy" files. With one file per
record, computers that edit different records never touch the same file.

- **Newest write wins per record.** Every record carries an `updated_at`
  timestamp. If two computers edit the same record while offline, the sync
  service keeps one file, and every computer converges on it.
- **Deletes are tombstones.** Deleting writes a small "deleted" record instead
  of removing the file, so a computer that was offline learns about the
  deletion instead of re-uploading its old copy.
- **Writes are atomic.** Each record is written to a temporary file and then
  renamed into place, so the sync client never uploads half a file.
- **Stray files are ignored.** Conflicted copies, temp files and anything else
  that isn't `<uuid>.enc` are skipped.

### Live sync

While the vault is unlocked, SSHVault watches the vault folder. When the sync
client writes a record, the watcher waits briefly for the write to settle,
decrypts the file, and pushes the change to the interface.

### Per-computer data

These files stay outside the vault, in the OS config directory:

| OS | Location |
|---|---|
| Linux | `~/.config/com.aliyousefian.sshvault/` |
| Windows | `%APPDATA%\com.aliyousefian.sshvault\` |

- `config.json` holds the path of the vault folder.
- `known_hosts` holds pinned server keys. It's per-computer, like OpenSSH's.

## Security model

- **Key derivation.** The master password and a random 16-byte salt go through
  Argon2id with 64 MiB of memory, 3 passes and 4 lanes. The salt is stored in
  `vault.json` so every computer derives the same key.
- **Encryption.** Every record is encrypted with XChaCha20-Poly1305, using a
  fresh random 24-byte nonce per write. Random nonces are safe to use this way
  across computers with no coordination.
- **Tamper resistance.** Each record's collection and UUID are bound in as
  authenticated data. A file that is edited, renamed, or moved to another
  folder fails to decrypt and is ignored.
- **Password check.** `vault.json` holds a small encrypted check value, so a
  wrong password is rejected immediately. It contains no secrets.
- **What the sync service sees.** It sees file names, which are random UUIDs,
  the number of records of each type, file sizes, and modification times. It
  cannot read hostnames, usernames, keys, passwords, snippets or forwarding
  rules.
- **In memory.** The master key is zeroed when the vault is locked. While
  the vault is unlocked, decrypted records are held in memory so the interface
  can show them.
- **Host keys.** Server keys are trusted on first use and pinned. A changed key
  blocks the connection and tells you which file to edit.
- **Changing the master password.** **Settings** re-encrypts every record
  with a fresh salt. Other computers must unlock again with the new password.

## Troubleshooting

**`pnpm tauri dev` fails with a `webkit2gtk-4.1` or `gdk-3.0` pkg-config error.**
The Tauri system libraries are missing. Install the packages listed under
[Prerequisites](#prerequisites).

**The terminal is blank or garbled on Linux.**
Some GPU and driver combinations misbehave with WebGL in WebKitGTK. Try
starting with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. xterm.js falls back to its
slower DOM renderer when WebGL is unavailable.

**"Host key for … has CHANGED".**
The server presented a different key from the one pinned earlier. If you
expected this, for example because the server was reinstalled, delete that
host's line from the `known_hosts` file under
[Per-computer data](#per-computer-data) and connect again. If you didn't
expect it, don't connect.

**"master password is incorrect" on a second computer.**
Check that the sync client has finished downloading `vault.json`, and that
you chose the same folder as on the first computer.

**Changes from another computer don't appear.**
Check that the sync client is running and has finished syncing. You can force
a re-read from disk with **Settings → Reload from disk**.

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

- There are no signed releases yet. Installers come from CI artifacts.
- macOS is not built or tested.
- A tab holds at most two panes.
- Snippets are sent to open terminals. They are not run on several hosts in
  the background.
- Old tombstones are never purged automatically, so deleted records leave
  small files behind.
- The SFTP pane can reach any path your user account can, since the app is a
  full file manager for this computer.

## License

MIT, as declared in `package.json`.
