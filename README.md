# SSHVault

A cross-platform SSH terminal manager and SFTP client (Tauri v2 + SvelteKit + Rust)
with zero-knowledge, client-side encryption. Sync between machines happens purely by
writing encrypted files into a folder you already sync (Dropbox, Nextcloud, Syncthing).
There is no server.

## Vault layout

```
<sync folder>/SSHVault/
  vault.json              plaintext manifest: Argon2id salt, KDF params, key verifier
  hosts/<uuid>.enc        one XChaCha20-Poly1305 envelope per host
  identities/<uuid>.enc   one per identity (username / password / private key)
  snippets/<uuid>.enc     one per snippet
  forwards/<uuid>.enc     one per port-forwarding rule
```

* One file per record, so machines editing different records never conflict.
* Records carry `updated_at`; the newest write wins per record.
* Deletes write tombstones so offline peers learn about them.
* Writes are temp-file + rename, so the sync client never uploads a partial file.
* The record path is bound into the AEAD as associated data, so files cannot be
  renamed or moved between collections undetected.

## Status

- [x] Phase 1: scaffolding, `src-tauri/src/crypto.rs`, `src-tauri/src/vault.rs`
- [x] Phase 2: UI shell (activity bar, host tree, keychain, snippets, tabs/split panes) and Tauri IPC + live file watcher
- [x] Phase 3: xterm.js (WebGL) + russh terminal engine, password / key / agent auth, trust-on-first-use host keys
- [x] Phase 4: dual-pane SFTP with recursive transfers, local / remote / SOCKS5 port forwarding, snippet run and broadcast

## Architecture (Rust side)

| Module | Role |
|---|---|
| `crypto.rs` | Argon2id KDF, XChaCha20-Poly1305 envelopes |
| `vault.rs` | UUID-per-file encrypted store, tombstones, atomic writes |
| `models.rs` | `Host`, `Identity`, `Snippet` payloads |
| `config.rs` | Per-machine `config.json` (vault path) in the OS config dir |
| `sync.rs` | `notify` watcher, debounced, decrypts changed files |
| `session.rs` | Unlock / lock lifecycle, holds the key, owns the watcher |
| `ssh.rs` | russh connect/auth (`open_client`), PTY sessions per pane, host-key TOFU, agent auth |
| `sftp.rs` | SFTP browsing, local file ops, cancellable recursive transfers |
| `forward.rs` | `-L`, `-R` and SOCKS5 `-D` forwarding, one SSH connection per rule |
| `commands.rs` | Tauri IPC commands, `vault:changed`, `ssh:status`, `forward:status` events |

## Jump hosts

Any host can name another saved host as its jump host, like OpenSSH
`ProxyJump`, and jump hosts can chain. Terminals, SFTP and port forwarding all
tunnel through the chain. Each hop logs in with its own identity and has its
host key pinned separately. Loops are rejected when saving, and deleting a host
makes anything that jumped through it connect directly.

## Host keys

Server keys are pinned on first use in `<app config dir>/known_hosts` (not the
vault, since it is per-machine). A changed key refuses the connection and names
the file to edit. The fingerprint of a newly learned key is shown in the pane.

## Verifying without the GUI libraries

The Tauri crate needs GTK/WebKit headers to compile on Linux. Without them you
can still type-check everything for Windows using Zig as the C compiler; see
`cargo check --target x86_64-pc-windows-gnu` with a `x86_64-w64-mingw32-gcc`
wrapper around `zig cc -target x86_64-windows-gnu` and a stub `windres`.

## Prerequisites

Rust (via rustup), Node 22+, pnpm, and the Tauri Linux system libraries:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

On Windows: Visual Studio Build Tools (C++ workload) and WebView2 (preinstalled on Win 10/11).

## Develop

```sh
pnpm install
pnpm tauri dev                     # desktop app
cd src-tauri && cargo test         # Rust tests; the SSH test spawns a throw-away sshd if one is installed
pnpm check                         # svelte-check
```
