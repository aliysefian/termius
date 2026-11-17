# Feature inventory used for the website

Compiled in Phase A from the repository, not from memory. Sources: `README.md`, `docs/USAGE.md`, `docs/TMUX.md`, `ROADMAP.md`,
`docs/engineering/FEATURE_INVENTORY.md` (an earlier audit, with its own caveats), `docs/design/*` and the v0.62.0 release assets.
The app is Tauri 2 + Rust + Svelte 5 + xterm.js; its palette and layout (`src/app.css`, `ActivityBar`, `TopBar`, `HostRow`) are the
reference for the simulated UI.

| Status | Capability | Where the website uses it |
|---|---|---|
| Implemented | Encrypted vault (XChaCha20-Poly1305, Argon2id, recovery key), folder sync, backups, conflict handling | Home, Security, docs |
| Implemented | Hosts, groups, tags, favorites, search, bulk edit; imports (ssh config, Ansible, PuTTY, MobaXterm, CSV) | Demo: host management |
| Implemented | SSH connections: password / key / credential / ask; host-key pinning; jump hosts, proxies, agent and X11 forwarding | Demo: create connection |
| Implemented | Terminal tabs, up to six splits, search, recording, themes, shell integration | Demo: terminal workflow |
| Implemented | tmux manager (sessions, windows, panes, live read-only previews, attach) | Demo: tmux |
| Implemented | Dual-pane SFTP / SCP / FTP(S), transfer queue, remote editing | Demo: SFTP |
| Implemented | Local, remote and SOCKS tunnels | Demo: port forwarding |
| Implemented | Key Manager and built-in SSH agent | Demo: keys and agent |
| Implemented | Snippets with variables, run on up to 8 hosts | Demo: run on many hosts |
| Implemented | Databases, containers (Docker/Podman/nerdctl), runbooks, CLI, quick connect, command palette | Features: "More capabilities" |
| Partial | Kubernetes (no deployments/services/events), monitoring and alerts (only while the app is open), Mosh | Labelled Partial |
| Experimental | Hardware keys through the local agent (tested with a stand-in device), serial consoles (not tried on hardware) | Labelled Experimental |
| Planned | Hardware-key vault unlock, team sharing, notarized macOS app, signed installers (built, not yet switched on); plugins and controller are design proposals only | Labelled Planned |

Release assets (v0.62.0, from the GitHub release API): `SSHVault_0.62.0_x64-setup.exe`, `SSHVault_0.62.0_x64_en-US.msi`,
`SSHVault_0.62.0_amd64.deb`, `SSHVault-0.62.0-1.x86_64.rpm`, `SSHVault_0.62.0_amd64.AppImage`, `SSHVault_0.62.0_universal.dmg`.

Not claimed anywhere on the site: anything from the Planned row as available, "end-to-end verified" security properties beyond
what `docs/vault-architecture.md` states, or real-world usage numbers. The README itself says the app is young.
