# What SSHVault needs next (feat3)

Written 2026-10-08, updated the same day after release 0.26.0. `feat.md` planned the file protocols,
remote desktops, databases, containers and monitoring; `feat2.md` compared the
app with other clients and planned the terminal, completion, inventory and
automation work. Most of that is built now (see the changelog from 0.9.0 to
0.26.0). This file lists what is **still** missing: what is left open from the
two earlier files, in one place, and the new needs that come from how
developers and operations people use a tool like this every day.

As in `feat2.md`: the "others" column is from each product's public material as
I know it (up to mid-2026), not re-checked for this file. **S** days, **M** a
week or two, **L** a month or more. **P1** do next, **P2** soon, **P3** when
there is room. "Verify" means I think it is missing but did not check every
corner of the code.

---

## 1. Where the app stands after feat2

Built since `feat2.md` was written: command blocks, highlight rules, search
across terminals, per-host looks, one-click shell integration, images and safe
links (0.21.0); smart completion for saved hosts, abbreviations, local tabs,
host history and more specs (0.22.0); VNC and Wake-on-LAN (0.23.0); cloud and
tool inventory import, network scan, command hosts and Ansible export (0.24.0);
runbooks, schedules, wait-and-send and hooks (0.25.0); SQL Server, Oracle,
rqlite, Redis, MongoDB and Elasticsearch in the Databases view (0.26.0). Before that: Ops
(logs, services, alerts), Kubernetes, the tour, languages, accessibility and
big-fleet performance.

What that leaves, against the comparison in `feat2.md` section 2:

| Gap | Who has it | Status |
|---|---|---|
| Team sharing, roles, audit | Termius, Royal TS, Warp (paid) | **not built** |
| Hardware keys, biometric unlock | Termius, Royal TS | **not built** |
| Plugins | Tabby, MobaXterm, Royal TS | not built, by decision |
| Multi-window | most | not built (needs a design) |
| ZMODEM, SMB, WebDAV, S3 | MobaXterm, WindTerm, Royal TS (some) | **not built** |
| SPICE, RDP depth | MobaXterm, Royal TS | not built |
| Built-in X server | MobaXterm | not built, by decision |

And a new column that `feat2.md` did not have: what a **developer or an
operations person** reaches for in a day that the app does not do yet. That is
section 4 below, and it is the reason for this file.

---

## 2. Still open from `feat.md` and `feat2.md`, in one list

Carried forward unchanged unless noted. Details are in the earlier files.

**Release and trust (feat2 4.1).** R3
real-window tests in CI, R4 packaging (winget, Scoop, Chocolatey, Homebrew,
Flatpak, AUR, ARM64), R5 security review and supply chain (`cargo audit`,
`cargo deny`, SBOM, reproducible builds), R6 opt-in crash reports.

**Teams and identity (feat2 4.2).** TS1 team vaults, TS2 audit trail, TS3
policies, TS4 SSO and short-lived certificates, TS5 emergency access.

**Keys and unlocking (feat2 4.3).** K1 hardware keys, K2 biometric unlock, K3
TOTP (needs the owner's decision), K4 password rotation helper.

**Terminal (feat2 4.4).** TB5 multi-window, TB8 ZMODEM/trzsz, TB9 X server;
TB2's per-host rules and auto-reply; TB3's bell and group-level profiles; TB4's
kitty graphics and opening a local editor at `file:line`; TB7's PowerShell and
cmd integration (also AC-f).

**Files (feat.md F3–F6, feat2 4.6).** F3 SMB, F4 WebDAV, F5 S3, F6 ZMODEM; FP1
compare and mirror, FP2 transfer rules, FP3 remote search, FP4 archives, FP5 a
built-in editor.

**Remote desktop (feat2 4.7).** RD1's SPICE and VNC's compressed encodings and
a real-server test; RD2 RDP depth; RD3's scheduled wake.

**Inventory (feat2 4.8).** G1's Proxmox, SSM as a transport, and refresh on a
schedule.

**Automation (feat2 4.9).** A5 plugins (not before the core settles).

**Containers (feat.md).** C2 Podman (needs a real Podman), C3 nerdctl, C4's
kubeconfig paste and more Kubernetes, C5 WSL containers.

**Databases (feat.md Phase 3).** D3–D8 are built (0.26.0) but only partly
proven; see DB1 in section 4.8.

**Monitoring (feat.md M1).** A recording from real BSD.

**AI (feat.md Phase 6).** A0–A4; the owner decided against AI in completion,
and whether an assistant is wanted at all is still an open decision.

**Sync and data (feat2 4.13).** SY1 history and restore, SY2 selective sync,
SY3 relay, SY4 more import and export formats, SY5 mobile (out of scope).

**Everyday (feat2 4.12 notes).** NVDA and Orca testing; translations of
dialogs and settings, native review, RTL mirroring; start-up and memory budgets
in CI; drag to resize the rail.

---

## 3. Suggested order: the next ten

1. **R3 Real-window tests in CI**, with the browser scripts (section 5) run
   there: the cheapest way to stop regressions like the 0.22.0 Windows failure.
2. **DX1 One connection per host** (section 4.1): every tab, file pane,
   monitor and runbook on a host shares one SSH connection, so a 2FA or
   hardware-key prompt is answered once. Removes the most daily friction.
3. **SM1 External secret managers** (4.2): fetch a password or key from
   1Password, Bitwarden, HashiCorp Vault or a cloud secret store at connect
   time, so teams never copy secrets into the vault.
4. **K1 + K2**: hardware keys and biometric unlock.
5. **OP1 Compare command output across hosts** (4.4) and **OP2 Alerts to a
   webhook** (4.4): the two things operations people ask for first after a
   fleet view.
6. **DX3 Connection diagnostics** (4.1): when a connection fails, say which of
   DNS, TCP, banner, host key, auth or the shell went wrong, with the verbose
   SSH log.
7. **TS1 Team vaults**, if the owner decides on teams (feat2 decision 3).
8. **DX5 Open a host's folder in VS Code or JetBrains** (4.3) and **DX6 Deploy
   on save** (4.3): the two developer workflows the app only half covers.
9. **F6 ZMODEM/trzsz** and **F5 S3**: the file protocols people still ask for.
10. **TB5 Multi-window.**

Reasoning: signing and macOS are left out on purpose (not wanted for now). 1 protects
everything else. 2 and 3 remove friction that every other item sits on top of (each
runbook host, each inventory refresh, each file pane opens a connection today).
4 to 6 are the gaps between "it works" and "I'd use it at work". 7 to 10 are
the features that decide whether a team or a power user switches.

---

## 4. New needs, by area

### 4.1 Connections (developers and operations alike)

- **DX1 One SSH connection per host, shared** (P1, M). Today each terminal
  tab, file pane, monitor session, completion lookup and runbook host opens its
  own SSH connection and authenticates again. With a hardware key, 2FA or a
  bastion that rate-limits, that means a prompt or a delay every time. OpenSSH
  has `ControlMaster`; Termius and SecureCRT reuse sessions. *Done:* a
  connection registry keyed by resolved target; a second tab, an SFTP pane and
  a monitor on the same host open channels on the existing connection; the
  connection closes when its last user does (with a short grace period); a
  connection that drops is replaced transparently; the header shows "shared
  with 3 tabs". Tested against the throw-away `sshd` with `MaxSessions` set
  low, which must now *work* instead of refusing the extra channel.
- **DX2 Keep-alive and reconnect that preserve the shell** (P2, M). Mosh does
  this for terminals; for everyone else, offer `tmux` or `screen` on the host
  as the session keeper: "Attach to a persistent session" per host (creates or
  attaches `sshvault-<label>`), so a dropped connection or a closed laptop
  comes back to the same shell. *Done:* a host option, an attach after
  reconnect, and a list of the host's tmux sessions to pick from. Verify the
  interaction with shell integration inside tmux.
- **DX3 Connection diagnostics** (P2, S to M). When a connection fails, the
  error is one line. Add a "Why?" that runs the steps one at a time — DNS,
  TCP reach, SSH banner, host key, each auth method, the shell start — and
  shows which failed with the server's own message and the verbose protocol
  log (`-vvv` equivalent), with a "copy for a bug report" that redacts
  secrets. *Done:* a wrong port, a wrong key, a changed host key and a
  banner-only server each give a different, correct diagnosis.
- **DX4 Environment and locale per host** (P3, S). `SendEnv`/`SetEnv`
  equivalents (the server must allow them), a `TERM` override for odd
  devices, and a locale hint. Verify: nothing of this exists today.
- **DX7 Port forwards from what the host is listening on** (P2, S). The
  monitor already lists listening ports (feat.md M1). Add "Forward to this
  computer" on a listening port, and on a Docker container's published port and
  a Kubernetes service, creating a tunnel rule with one click. *Done:* a
  tunnel appears under Tunnels, named after the service, and can be saved.
- **DX8 Proxy auto-detection** (P3, S). Read the system proxy or a PAC file for
  the HTTP proxy type; today the proxy is typed per host or group.
- **DX9 Paste for slow consoles** (P3, S). A per-host "type pasted text with a
  delay between lines" for serial consoles and network gear that drop
  characters. The paste confirmation already exists; this adds pacing.

### 4.2 Secrets and identity (operations)

- **SM1 External secret managers** (P1, L). Most teams already keep SSH
  passwords and keys in 1Password, Bitwarden, HashiCorp Vault, AWS Secrets
  Manager, GCP Secret Manager or Azure Key Vault, and a policy that forbids a
  second copy. Let a credential say "fetch from …" instead of holding the
  secret: the app calls the manager's own CLI or agent at connect time (as the
  inventory import does), uses the value in memory, and stores nothing. Start
  with 1Password (`op`), Bitwarden (`bw`), HashiCorp Vault (`vault`) and the
  three cloud CLIs. *Done:* a credential of kind "reference", a connection that
  works without the secret ever being written to disk, a clear failure when the
  manager is locked, and no secret in any log or toast. The SSH agent in
  1Password should also be usable as the agent backend.
- **SM2 Certificate authority workflow** (P2, M; narrows TS4). Many teams sign
  user keys with a CA (`ssh-keygen -s`, step-ca, Vault SSH secrets engine,
  Teleport). The app can hold a certificate today; add "request or renew my
  certificate" per CA profile (a command, like command hosts, or a Vault/step-ca
  call), show expiry on the key, warn before it lapses, and renew on connect
  when it has. *Done:* a key whose certificate expires in an hour is renewed
  before the connection and the old one is never offered.
- **SM3 Expiry and hygiene dashboard** (P3, S). One page: keys older than N,
  certificates about to expire, passwords never rotated, hosts whose host key
  changed, credentials unused for 90 days. The Security review page has some of
  this; make it one view with dates and actions. Verify what it already covers.
- **SM4 Agent for WSL and other clients** (P2, S). The vault agent serves this
  computer's `ssh`. Expose it to WSL distributions (a socket relay) and, on
  Windows, as a named pipe `ssh.exe` and Git use, so `git push` from WSL signs
  with vault keys with the same per-key approval. Verify how far the Pageant
  support goes today.

### 4.3 Developer workflows

- **DX5 Open in your editor** (P2, S). "Open this folder in VS Code" (`code
  --remote ssh-remote+<host> <path>`), JetBrains Gateway, and Zed, from a host,
  a file pane and a terminal's current folder, using the app's SSH config
  export so the editor connects with the same key and jump host. *Done:* a
  button in the file pane and the pane header; a generated `~/.ssh/config`
  include that the editor can use; documented limits (the editor does its own
  SSH, so vault-only credentials need the agent).
- **DX6 Deploy on save** (P2, M). Watch a local folder and mirror changes to a
  remote folder as they happen (with ignore patterns, a pause, and a log), for
  people who edit locally and run remotely. Rsync over SSH where available,
  SFTP otherwise. This is the one-way, continuous half of FP1 and is more used
  than the full compare.
- **DX10 Command history across hosts** (P3, S). A page to search "what did I
  run, where, when, and did it work", from the history the app already keeps
  for completion and the connection log, with re-run on the same host or
  another. Verify how much the palette's history already shows.
- **DX11 Local dev containers and compose** (P3, M). The Containers view can
  manage a Compose project on a host (feat.md C1); add starting a project from a
  local folder, and "attach a terminal to this container" is there already.
  Verify what C3 ("Compose editing") still lacks before starting.
- **DX12 Quick-command bar per host** (P3, S). A row of buttons above a
  terminal (SecureCRT's button bar, MobaXterm's macro bar) bound to snippets,
  per host or group. Snippets and the palette exist; this is the visible
  version for people who click.

### 4.4 Operations

- **OP1 Compare output across hosts** (P2, M). After "Run on hosts" or a
  runbook, show the hosts' outputs side by side with the differences marked
  (which hosts have a different kernel, package version, config line), group
  identical outputs, and export a table. This is what fleet tools are used for
  most. *Done:* 20 hosts, `uname -r`, one host different, visible in a second.
- **OP2 Alerts to a webhook** (P2, S). Alerts exist in the app only. Add a
  webhook (Slack, Teams, Discord, a generic JSON POST) and an email through a
  command, with a test button and a per-rule choice. Only while the app runs,
  said plainly, as with schedules.
- **OP3 Maintenance windows and change freezes** (P2, S). A calendar of windows
  per group: outside a window, destructive actions on production (the guard
  already exists) require a reason that goes into the connection log; a
  "freeze" flag blocks them. Small, but it is what change management asks for.
- **OP4 Session recordings you can play back** (P2, M). The session log is a
  raw text file. Record in the asciicast format and add a player with a
  timeline, speed, search and "jump to the next command" (the command blocks
  give the marks), and an export for incident reviews. Keep the raw option.
- **OP5 Run an Ansible playbook as a step** (P3, M). Runbooks are the app's
  own; many teams have playbooks already. A runbook step `ansible: { playbook,
  limit }` that runs the local `ansible-playbook` with the app's exported
  inventory and shows its output per host. Also Terraform plan/apply as a
  command host is already possible; a step with the plan shown first is the
  useful version.
- **OP6 Kubernetes, next steps** (P2, M; carries C4). Events, a YAML view with
  edit and apply (shown as a diff first), Helm releases, deployments with
  restart and scale, and a context switcher that remembers per host. Pods, logs,
  shell and port-forward exist.
- **OP7 Host notes that are a wiki** (P3, S). Notes are a text field. Render
  Markdown (the renderer exists), link to other hosts and snippets, and show
  the note on connect when it has a "read me first" marker. Verify whether
  notes already render Markdown.

### 4.5 Remote desktop and protocols

- **RD4 RDP and VNC through a jump host** (P2, S). The local-tunnel helper
  already exists for forwards; RDP connects directly today. Route RDP (and
  VNC when not using its own SSH tunnel) through the host's jump chain and
  proxy like SSH does, so bastion-only networks work. *Done:* an RDP host
  behind a jump host opens; the tunnel closes with the tab.
- **RD5 VNC compression and a real server** (P2, M). Tight or ZRLE encoding
  (today only Raw and CopyRect, which costs bandwidth) and a test against a
  real TigerVNC or x11vnc in CI (Xvfb exists on the CI image). Also UTF-8
  clipboard through the extended clipboard extension.
- **RD2** carries: drives, audio, multi-monitor, dynamic resize for RDP.

### 4.6 CLI and integration (automation from outside the app)

- **CL1 A fuller CLI** (P2, M). The control socket and CLI exist for
  connecting and listing. Add `run` (a command or a runbook on hosts, with JSON
  output and exit codes), `inventory refresh`, `tunnel up/down`, `export`, so
  CI jobs and scripts can use the vault without the window, with the same
  approval prompts shown in the app. *Done:* a shell script that runs a runbook
  on a group and fails the job when a host fails.
- **CL2 URL handlers** (P3, S). `ssh://user@host:port` and `sshvault://host/<label>`
  open the app from a browser, a wiki or a ticket. Register the schemes on
  install.
- **CL3 Import from Termius, SecureCRT, Royal TS** (P2, S each; SY4). Termius
  exports CSV (done) and JSON; SecureCRT and Royal TS have session files.
  People switch when the import is painless.

### 4.7 Platform and reach

- **PL1 Windows polish** (P2, S each). ConPTY edge cases with PowerShell 7
  and cmd, WSL paths in file panes, the Windows credential store for the unwrap
  key (K2's half), and the named-pipe agent (SM4).
- **PL2 Linux packaging** (P2, S). Flatpak and AUR first (R4), because that is
  where Linux users look.
- **PL3 Portable mode** (P3, S). Run from a USB stick with the vault and
  settings beside the executable; verify what the portable-vault work already
  gives.

### 4.8 Databases, after D3–D8

- **DB1 Prove the new engines on real servers** (P2, S each). SQL Server and
  Oracle were never run against a real server, Redis TLS and MongoDB's SCRAM
  and TLS never against a real one, and rqlite, MongoDB and Elasticsearch only
  against stand-in servers. Run each in CI as a service container (CI has the
  disk the owner's machine lacks) and keep the live tests behind an
  environment variable like `SSHVAULT_REDIS_SERVER`. *Done:* every engine has a
  live test that passes in CI; the "Not verified" notes in `feat.md` D3–D8 are
  removed or narrowed.
- **DB2 A key browser for Redis, a filter bar for MongoDB** (P3, M). They use
  the console and grid for now. A Redis key detail pane (type, TTL, size, edit
  in place) and a MongoDB filter bar (the original D7 wording) would suit
  people who do not type commands.
- **DB3 Larger results from Elasticsearch and Redis** (P3, S). Scroll or
  point-in-time paging past a search's size; cursor-style paging of huge
  keys; Redis cluster and Sentinel; Redis 7 functions and streams groups.
- **DB4 Export, import and schema tools for all engines** (P3, M). Export a
  result as CSV, JSON or INSERTs, import a CSV, and a schema diff, now that
  there are eight engines to cover. Verify what D1/D2 already export.
- **DB5 SQL Server named instances and Azure SQL; Oracle wallets** (P3, S).
  Named instances are reached only by port, and `oracle-rs` is a young crate;
  decide whether to keep it or add an OCI-backed option if it falls short in
  real use.

---

## 5. Quality debts, updated

Carried from `feat2.md` section 5, with what changed:

1. **Nothing has run in the real Tauri window here** (still true; R3).
2. **Browser test scripts are not in CI.** There are now thirteen of them
   (`completion`, `shells`, `perf`, `ops`, `kube`, `a11y`, `bigfleet`,
   `everyday`, `terminal`, `remote`, `inventory`, `automation`, `databases`). Putting them in
   CI is a day's work and would have caught the 0.22.0 Windows failure's class
   of bug earlier.
3. **Platform assumptions.** 0.22.0's Windows build failed on a Unix-only path
   check. Add a Windows job that runs the Rust tests (it exists for clippy;
   verify it runs `cargo test`) and grep the code for `starts_with('/')` and
   friends.
4. **Tests written but not run here:** the two real-`sshd` runbook tests
   (`runbookrun/tests.rs`) compile but could not be run on this machine;
   confirm them in CI.
5. **VNC was tested against a simulator only**; the inventory parsers against
   documented output shapes only; Wake-on-LAN against a local listener only.
6. **No fuzzing** on untrusted-text parsers: OSC and VNC framing, ssh config
   and PuTTY/MobaXterm/CSV imports, the completion line parser, runbook
   documents, inventory JSON.
7. **Unproven corners** from before and from 0.26.0: SQL Server, Oracle, Redis
   TLS, MongoDB SCRAM/TLS (DB1), MySQL TLS, Podman and nerdctl parsers,
   BSD monitoring, Compose on other versions, smart completion on
   Windows and with zsh-autosuggestions.
8. **Release builds are not observed** from here; check the Actions tab after
   each tag.
9. **Translations** cover the sidebar, menus, headings and tour only, are
   unreviewed by native speakers, and there is no RTL layout.

---

## 6. Decisions for the owner

The ones from `feat2.md` about teams, plugins, AI, a sync relay and mobile are
still open. Signing and macOS were dropped by the owner and are not planned. New ones:

8. **Secret managers (SM1).** Integrating with 1Password, Bitwarden, Vault and
   the clouds makes the app a client of those systems; is that wanted, or
   should the vault stay the only store?
9. **Shared connections (DX1).** Sharing one connection changes failure
   behaviour (one drop closes several tabs) and is a change to the SSH core.
   Worth it for the 2FA and bastion cases?
10. **Webhooks and email (OP2).** The first feature that sends data to a
    third-party service from the app. Opt-in per rule, but it is a line to
    cross knowingly.
11. **Ansible and Terraform as steps (OP5).** Running other tools' playbooks
    from runbooks makes the app responsible for their output and failures. A
    command host already covers the simple case.
12. **Session recordings (OP4).** Playable recordings are also a surveillance
    tool. If built, decide whether they are per-person only, and never synced.
13. **Service containers in CI (DB1).** Running real SQL Server, Oracle,
    MongoDB and the rest in CI makes the database work trustworthy, at the
    cost of slower, larger CI runs. Worth it, or keep them manual?
