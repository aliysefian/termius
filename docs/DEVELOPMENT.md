# Developing SSHVault

## Prerequisites

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

## Get the code and run it

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

## Tests and checks

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

## Command specs

The popup's subcommands and options come from `src/lib/completion/specs/*.json`
(one file per command, loaded the first time the command is typed, plus a small
`index.json` of names). They are committed; a normal build never needs Node or
the network for them. To redo them, for a new version of the source package or a
different list of commands:

```bash
# edit COMMANDS (and the pinned VERSION) in scripts/build-completion-specs.mjs, then
node scripts/build-completion-specs.mjs            # downloads the pinned package with npm
node scripts/build-completion-specs.mjs --from DIR # or use an unpacked one (DIR/build/*.js)
pnpm test                                          # src/lib/__tests__/command.test.ts reads them
```

The script prints how many commands it wrote and their size. Licence and source
are in `THIRD_PARTY.md`; `SOURCE.json` next to the specs records the version.

## Build installers

```sh
pnpm tauri build
```

Output goes to `src-tauri/target/release/bundle/`:

- Linux: `deb/`, `rpm/`, `appimage/`
- Windows: `msi/`, `nsis/`

## Code signing

**See `docs/SIGNING.md`** for everything about signing and verifying releases (Windows Authenticode, GPG
signatures and checksums for every installer, how to set the keys up, and how a downloader checks a
release). The rest of this section is the short Windows version.

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

## Updates

SSHVault can update itself. It checks for a new version at start-up (at
most twice a day, switchable in **Settings → Updates**, which also has a
**Check for updates** button). When one exists, an update button appears in
the activity bar and a card offers **Update now**.

There are two ways an update is verified and installed:

- **Signed (preferred).** Builds made with an update-signing key use the
  Tauri updater. It reads `latest.json` from the release and installs only
  files whose signature matches the public key built into the app.
- **GitHub checksum (every other build).** The app asks the GitHub API for
  the latest release of `aliysefian/termius`, picks the installer that
  matches how it was installed, downloads it, and installs it only if it
  matches the SHA-256 GitHub publishes for that file (`src-tauri/src/release.rs`).
  The AppImage is replaced in place; `.deb` and `.rpm` installs run
  `pkexec dpkg -i` / `pkexec rpm -U`, which shows the system password
  prompt; Windows runs the NSIS setup in passive mode and reopens the app.
  A checksum only proves the file is the one on the release page, so it
  protects against a corrupted or tampered download, not against someone
  who can publish releases to the repository. That's what signing adds.

Signed updates switch on in CI once these exist in the repository settings:

| Name | Kind | Value |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | secret | The private key from `pnpm tauri signer generate` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | secret | Its password |
| `TAURI_UPDATER_PUBKEY` | variable | The matching public key (printed by the same command) |
| `UPDATER_ENDPOINT` | variable, optional | Where apps read `latest.json` (default: this repository's latest release) |
| `UPDATER_DOWNLOAD_BASE` | variable, optional | Where `latest.json` points for the files (default: this release's assets) |

```sh
pnpm tauri signer generate -w ~/.tauri/sshvault-updater.key
```

Keep a backup of the private key. Without it, installed copies can never
receive another update, because only updates signed with it are accepted.

Each tagged release then also carries `latest.json` and `.sig` files.
**The feed must be downloadable without logging in.** A private
repository's release assets aren't, so either make the releases public, or
point `UPDATER_ENDPOINT` and `UPDATER_DOWNLOAD_BASE` at a public place that
the release files are copied to.

## Continuous integration

`.github/workflows/build.yml` has three jobs:

- **Checks** run on every push to `main` and every pull request, on Ubuntu
  22.04 and Windows: the frontend type-check and tests (once, on Linux),
  clippy and the Rust tests. Changes that only touch `*.md` or `docs/` skip
  CI.
- **Installers** are built for `v*` tags, and on demand from **Actions →
  Build → Run workflow**, alongside the checks. The optimised release build
  is most of CI's time, so ordinary pushes skip it.
- **Release**, for a tag, once checks and installers pass: a published
  GitHub Release with the `.exe`, `.msi`, `.deb`, `.rpm` and `.AppImage`
  and a table saying which file is for which system. It refuses to publish
  if any of the five is missing.

The end-to-end SSH tests run on Linux. Windows has
no `sshd`, so they skip themselves there. Set `SSHVAULT_SKIP_SSHD_TESTS=1` to
skip them anywhere else.

To release, bump the version in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, then push a tag such as `v1.0.0`. The release is
published automatically when the build passes.

## Project structure

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

## IPC overview

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

## Type-checking the Tauri crate without GTK

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

The full design is in [docs/vault-architecture.md](vault-architecture.md).
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

## Troubleshooting development builds

**`pnpm tauri dev` fails with a `webkit2gtk-4.1` or `gdk-3.0` pkg-config error.**
The Tauri system libraries are missing. Install the packages listed under
[Prerequisites](#prerequisites).
