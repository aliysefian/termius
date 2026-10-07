# What SSHVault needs next (feat2)

Written 2026-10-07, after release 0.18.0. This is a map of what is missing
compared with the other SSH clients people use, and a suggested order. It does
not repeat the tasks already planned in `feat.md` (file protocols, VNC/SPICE,
containers, databases, AI, languages): those are listed in section 6 so there is
one place to see everything that is open.

**How to read the comparison.** The "others" column comes from each product's
public documentation and marketing as I know them (up to mid-2026). I have not
re-checked them for this file and I have not used every one of them, so treat a
✓ or ✗ for another product as "worth confirming before you quote it". The
SSHVault column is from this repository.

Legend: ✓ has it · ~ partly · ✗ does not · ? not sure. Sizes: **S** days,
**M** a week or two, **L** a month or more. Priorities: **P1** do next, **P2**
soon, **P3** when there is room.

---

## 1. Where SSHVault stands

What it does that most of the others do not, and should keep leading with:

- **A vault you sync yourself.** No account, no server; the sync folder holds
  ciphertext. Termius, Warp and Royal TS sync through their own cloud or a paid
  server; MobaXterm and Tabby have no encrypted team vault at all.
- **A vault-backed SSH agent** with per-key approval, also over agent
  forwarding. Few clients have this.
- **Production guard rails**: destructive-command and paste checks, typed
  confirmation, red banners, a Security review page.
- **Many tools in one app**: terminals, SFTP/SCP/FTP, databases, containers,
  tunnels, RDP, Mosh, fleet monitoring, a CLI, run-on-many-hosts, smart
  completion that never types for you.

What holds it back, in the order a new user feels it: it is Linux and Windows
only; the installers are not signed; there is no sharing between people; there
is no inventory import from clouds; and several things were only ever checked
in a headless browser, not in the real window (see section 5).

---

## 2. Comparison

Rows are things people choose a client for. Columns: **SV** = SSHVault.
**Termius**, **MobaXterm**, **Tabby**, **WindTerm**, **Royal TS / SecureCRT**
(the two commercial multi-protocol managers, grouped), **Warp** (a terminal
first, SSH second).

| Capability | SV | Termius | MobaXterm | Tabby | WindTerm | Royal TS / SecureCRT | Warp |
|---|---|---|---|---|---|---|---|
| Linux + Windows | ✓ | ✓ | Windows only | ✓ | ✓ | Windows (Royal TS also macOS) | macOS, Linux, Windows |
| **macOS** | ✗ | ✓ | ✗ | ✓ | ✓ | ✓ | ✓ |
| **Mobile apps** | ✗ (out of scope) | ✓ | ✗ | ✗ | ✗ | ~ (Royal TS) | ✗ |
| Signed installers / auto-update | ✗ / ~ | ✓ | ✓ | ✓ | ~ | ✓ | ✓ |
| Encrypted sync without an account | **✓** | ✗ | ✗ | ~ (config sync) | ~ | ~ | ✗ |
| **Team sharing** (shared vault, roles) | ✗ | ✓ (paid) | ✗ | ✗ | ✗ | ✓ | ✓ (paid) |
| SSO / SAML, audit log | ✗ | ✓ (business) | ✗ | ✗ | ✗ | ~ | ✓ (business) |
| Hardware keys (FIDO2, PKCS#11), biometrics | ✗ | ✓ | ~ | ~ | ~ | ✓ | ? |
| Built-in SSH agent from the vault | **✓** | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| SFTP, with transfer queue | ✓ | ✓ | ✓ (side panel) | ~ (plugin) | ✓ | ✓ | ✗ |
| SMB / WebDAV / S3 browsing | ✗ (planned) | ✗ | ✗ | ✗ | ✗ | ~ | ✗ |
| ZMODEM / trzsz upload from a terminal | ✗ (planned) | ✗ | ✓ | ~ | ✓ | ✓ | ✗ |
| Port forwarding, SOCKS, jump hosts | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ~ |
| Built-in **X11 server** on Windows | ✗ (forwards to yours) | ✗ | **✓** | ✗ | ✗ | ✗ | ✗ |
| RDP | ✓ (text clipboard) | ✗ | ✓ | ✗ | ✗ | ✓ | ✗ |
| VNC / SPICE / XDMCP | ✗ (planned) | ✗ | ✓ | ✗ | ✗ | ✓ | ✗ |
| Serial, Telnet | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ |
| Mosh | ✓ | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| Autocomplete, from history / specs / host | ✓ (off by default) | ✓ | ~ | ~ | ✓ | ~ | ✓ |
| **Command blocks** (collapse, copy, search one command's output) | ~ (marks only) | ✗ | ✗ | ✗ | ✗ | ✗ | **✓** |
| Highlight / trigger rules (colour or react to text) | ✗ | ✗ | ✓ | ~ | ✓ | ✓ | ✗ |
| Scripting / macros / runbooks | ~ (snippets, CLI) | ~ | ✓ (macros) | ✓ (plugins) | ✓ | ✓ | ✓ (workflows) |
| **Cloud inventory import** (AWS, GCP, Azure, Hetzner, Proxmox, Tailscale) | ✗ | ✓ (some) | ✗ | ✗ | ✗ | ✓ (dynamic folders) | ✗ |
| Plugins / extensions | ✗ | ✗ | ✓ | **✓** | ~ | ✓ | ✗ |
| Multi-window / tear-off tabs | ✗ | ✓ | ~ | ✓ | ✓ | ✓ | ✓ |
| Databases, containers, fleet monitor | **✓** | ~ | ✗ | ✗ | ✗ | ✗ | ✗ |
| AI help | ✗ (decided: not in completion) | ✓ | ✗ | ~ (plugins) | ✗ | ✗ | ✓ |
| Languages (UI translations) | ✗ (planned) | ~ | ~ | ✓ | ✓ | ✓ | ✗ |

What the table says: the biggest holes against *everyone* are **macOS**,
**signed installers**, and **team sharing**. Against the "does everything"
tools (MobaXterm, Royal TS) it is **VNC, an X server, ZMODEM and inventory
import**. Against the polished terminals (Warp, Tabby, WindTerm) it is
**command blocks, highlight rules and plugins or scripting**.

---

## 3. Suggested order: the next ten

1. ◐ **R1 Sign the installers** (Windows code signing, Linux package signing). *Base built; waiting for a certificate and a GPG key.*
2. **R2 macOS builds** (universal binary, notarised) and a macOS CI job.
3. **R3 Run the UI tests in CI** and verify the real window (tauri-driver).
4. **G1 Cloud and tool inventory import** (AWS, GCP, Azure, Hetzner, Proxmox,
   Tailscale, `kubectl` contexts) with refresh.
5. **K1 Hardware keys and biometric unlock** (FIDO2, PKCS#11, Windows Hello,
   Touch ID).
6. **TS1 Team vaults** encrypted to several people's keys.
7. ✅ **TB1 Command blocks** on top of the OSC 133 marks the app already reads. *Built in 0.21.0.*
8. ✅ **TB2 Highlight and trigger rules.** *Built in 0.21.0.*
9. **A1 Runbooks**: a saved sequence of steps with parameters, run on one or
   many hosts, with a history.
10. **F-phase from `feat.md`**: S3, SMB and ZMODEM, then VNC.

Reasoning: 1 to 3 are what a stranger hits before they ever see a feature (a
SmartScreen warning, no macOS build, an update that cannot be trusted). 4 and 5
remove daily friction for people with many servers and security keys. 6 is the
feature that decides whether a team adopts it. 7 to 9 are what makes the
terminal itself feel better than the alternatives.

---

## 4. Needs, by area

Each item says what is missing, who has it, why it matters, and what "done"
means in one line. Detailed definitions of done should be written when a task
is started, as `feat.md` does.

### 4.1 Release and trust

- **R1 Code signing** (P1, M). **Base built (see `docs/SIGNING.md`); waiting for a certificate and a GPG key.** Windows installers trigger SmartScreen; Linux
  packages are not signed. Termius, Warp, Tabby and Royal TS all ship signed.
  *Done:* an Authenticode-signed `.msi`/`.exe` (certificate or a signing
  service), signed `.deb`/`.rpm` or an apt/yum repository with a published key,
  and the updater verifying the signature (the repo has no signing secrets
  today, so the in-app updater falls back to a checksum check).
- **R2 macOS** (P1, L). The README says Linux and Windows. macOS is the platform
  most developers use; every competitor above except MobaXterm has it.
  *Done:* a notarised universal `.dmg`, a macOS CI job, Keychain integration
  checked, and the macOS and BSD monitoring recording `feat.md` M1 still lacks.
- **R3 Real-window tests in CI** (P1, M). The browser tests use a mocked
  backend and a headless Chromium, and nothing has run in the real Tauri
  window. *Done:* the three browser scripts (`completion.py`, `shells.py`,
  `perf.py`) run in CI against a built app via `tauri-driver`, on Linux and
  Windows at least.
- **R4 More packaging** (P2, S each). winget, Scoop, Chocolatey, Homebrew cask,
  Flatpak, AUR, ARM64 for Linux and Windows. *Done:* each documented and
  produced by the release workflow.
- **R5 Independent security review** (P2, L). A password manager lives on
  trust. Publish the threat model (`docs/vault-architecture.md` is a start),
  commission an external review of the vault format and the agent, and publish
  the result. Add `cargo audit`/`cargo deny`, an SBOM, and reproducible builds
  to CI.
- **R6 Crash and error reports, opt-in** (P3, S). Local log file plus "copy
  diagnostics" first; anything that leaves the computer only if the person sends
  it.

### 4.2 Teams and identity

- **TS1 Team vaults** (P1, L). Already named in `ROADMAP.md` ("Team sharing").
  A group inside a vault, or a second vault, encrypted to each member's public
  key, so hosts are shared without sharing a master password. Needs: add and
  remove a member (with key rotation on removal), read-only and edit roles,
  a visible "shared with" list. *Done:* two people, two machines, one synced
  folder, one shared group; removing a member makes the data unreadable to them
  from then on.
- **TS2 Audit trail** (P2, M). Who connected to what, from which device, and
  which snippets were run on which hosts; exportable; tamper-evident (hash
  chain). Today there is a local connection log only.
- **TS3 Policies** (P2, M). A team vault can say "no agent forwarding to
  production", "no password auth", "paste confirmation always on".
- **TS4 SSO and short-lived certificates** (P3, L). Log in with OIDC and receive
  an SSH certificate from the company's CA (Teleport, Vault, step-ca). Needed
  for companies, not for individuals.
- **TS5 Emergency access and recovery** (P2, M). The recovery key exists; add
  "break glass" for team vaults and a device list with revoke.

### 4.3 Keys and unlocking

- **K1 Hardware keys** (P1, M). FIDO2 (`sk-ssh-ed25519`) and PKCS#11 through the
  vault's own agent, and unlocking the vault with a hardware-backed key.
  Already in `ROADMAP.md`. *Done:* sign in to a real server with a YubiKey, and
  unlock the vault with it.
- **K2 Biometric unlock** (P1, S to M). Windows Hello and Touch ID so the master
  password is not typed all day. The unwrap key stays in the OS secure store.
  *Done:* a setting, a fallback to the password, and a clear statement of what
  is protected by what.
- **K3 TOTP and one-time codes** (P2, M). Store a TOTP secret next to a
  credential, show the code, and offer to type it at the prompt. This is the
  "identities at a password prompt" idea from `auto.md` section 7, which the
  owner declined for completion; as a separate explicit feature it still needs
  its own decision.
- **K4 Password rotation helper** (P3, M). Change a password over SSH on a
  chosen host set, update the vault, report failures. The Security review page
  already finds the passwords worth rotating.

### 4.4 Terminal (built in 0.21.0; TB5, TB8 and TB9 deliberately not built)

- ✅ **TB1 Command blocks** (P1, M). Warp's best idea, and the app already tracks
  OSC 133 prompt, input and end marks. Group a command with its output; fold,
  copy the output, jump between them, search inside one, show duration and exit
  code, pin one. Must keep working when the shell has no integration (just no
  blocks). *Done:* with integration on, a block gutter in the pane; every action
  works by keyboard.
  *Built (4.4, v0.21.0): margin bars, copy/select/pin, jump to failed. Folding is not possible in xterm.js, so output stays visible.*
- ✅ **TB2 Highlight and trigger rules** (P1, M). MobaXterm, SecureCRT and WindTerm
  colour `ERROR` red, or react to text (notify, send a reply). Per host or
  global, regex, ordered. *Done:* a rules list with live preview, tested
  against large output without slowing it.
  *Built: global ordered rules with live preview and notify triggers; not per host, and no auto-reply (sending text on a match is a risk without a clear need).*
- ✅ **TB3 Profiles per host** (P2, S). Font, theme, bell, scrollback and
  environment per host or group (the red production tint is the one example
  today).
  *Built: theme, text size, scrollback per host. Not built: bell, environment, group-level profiles.*
- ✅ **TB4 Terminal images and links** (P2, M). Inline images (iTerm2 protocol,
  Sixel, kitty graphics), OSC 8 hyperlinks, and clicking `file:line` in
  compiler output to open the local editor.
  *Built: Sixel/iTerm2 images, OSC 8, file:line. Not built: kitty graphics, opening a local editor.*
- ⏸ **TB5 Multi-window and tear-off tabs** (P2, M). Drag a tab out into its own
  window; reopen windows after a restart.
  *Not built: needs a second webview window sharing live SSH sessions and the unlocked vault, a design of its own.*
- ✅ **TB6 Search across all open terminals** (P2, S to M). One search box over
  every pane's scrollback and the session logs.
  *Built for open terminals; session logs are not searched.*
- ✅ **TB7 Shell integration, one click** (P2, S). Offer to install the integration
  snippet on a host (with a diff and consent), instead of the person pasting it
  into `.bashrc`. Also PowerShell and cmd integration (none today), which is
  also what smart completion needs on Windows.
  *Built for bash, zsh and fish. PowerShell and cmd integration remain open.*
- ⏸ **TB8 ZMODEM and trzsz** (P2, M). Planned as F6; listed here because it is a
  terminal feature MobaXterm and WindTerm have and people on serial and jump
  hosts use daily.
  *Not built: the protocol needs `lrzsz` on the host and a careful transfer UI; SFTP covers the same need. Revisit on demand.*
- ⏸ **TB9 A built-in X server for Windows** (P3, L). MobaXterm's best-known
  feature. Today X11 forwarding needs VcXsrv or X410. Large; consider
  documenting the setup first and bundling later.
  *Not built: bundling an X server is a large, separate project; the setup (VcXsrv/X410) is documented.*
- ✅ **TB10 Ligatures, font fallback, emoji width** (P3, S). Check the terminal
  renders programmer fonts and wide characters the way people expect.
  *Emoji and wide characters fixed with Unicode 11; ligatures not enabled.*

### 4.5 Smart completion follow-ups (built in 0.22.0; AC-f partly)

The first version shipped in 0.17.0 (`auto.md`). What it still lacks:

- ✅ **AC-a Local tabs** (P2, S). File names and generators for local shells; today
  only SSH panes get host lookups.
  *Built: file and folder names from this computer on Unix; not the named lists, and not on Windows.*
- ✅ **AC-b More lookups and specs** (P2, S each). `kubectl --context`, `journalctl
  -u`, `apt-get`, `dnf`, `ip`, `ss`, `awk` have no spec in the source package;
  write them. Automate refreshing the specs, and tell the person which version
  they have.
  *Built: hand-written specs in `scripts/completion-extra.mjs`, `--check` for a newer package, version shown in Settings. Refresh is a command, not a scheduled job.*
- ✅ **AC-c Seed from the host's own history** (P3, S). Optionally read
  `~/.bash_history` over the lookup channel, with consent, so a new machine is
  not empty.
  *Built: bash, zsh and fish history files, once per host per run, filtered like typed commands.*
- ✅ **AC-d `ssh ` completes your vault hosts** (P2, S). The app knows every host
  name; `ssh we` and `scp` targets should offer them.
  *Built for ssh, scp, sftp and mosh.*
- ✅ **AC-e Abbreviations** (P3, S). Type `gco` and expand to a saved snippet.
  *Built: a snippet abbreviation, offered first when typed in full and accepted from the list.*
- ◐ **AC-f Old-style `tar xzf`, multi-line editing, and PowerShell** (P3, M).
  *Built: old-style tar. Not built: multi-line editing and PowerShell (no integration to read the line from).*

### 4.6 Files and protocols

Already planned in `feat.md` (F3 SMB, F4 WebDAV, F5 S3, F6 ZMODEM). New needs:

- **FP1 Compare and mirror** (P2, L). Compare two folders (local or remote) and
  sync one way or both, with a preview, in the style of WinSCP or Beyond
  Compare. Rsync-over-SSH as an engine where available.
- **FP2 Transfer rules** (P2, M). Bandwidth limit, checksum verification after
  upload, schedule a transfer, retry with back-off, a log you can export.
- **FP3 Remote search** (P3, S). Find a file by name or content on the host
  (`find`/`grep` over the lookup channel with caps).
- **FP4 Archive preview and extract** (P3, M).
- **FP5 A built-in text editor** (P3, M). Today it opens the local default
  editor; a small built-in editor with syntax colours helps on locked-down
  machines.

### 4.7 Remote desktop (VNC and Wake-on-LAN built in 0.23.0)

- ◐ **RD1 VNC** (P2, L; `feat.md` T4) and **SPICE** (P3, L; T5), through an SSH
  tunnel by default.
  *VNC built: SSH tunnel by default, password prompt, Raw and CopyRect only, US layout, tested against a simulator, not a real server. SPICE not built.*
- ⏸ **RD2 RDP depth** (P2, L). Drive and printer redirection, audio, multi-monitor,
  image clipboard, dynamic resize, NLA with smart cards.
  *Not built: each needs a real Windows or xrdp server to test against (xrdp is the only one used so far).*
- ✅ **RD3 Wake-on-LAN** (P3, S). A button and a scheduled wake.
  *Built: MAC and broadcast per host, a test button, and "Wake it up" on a failed connection. A scheduled wake waits for A2.*

### 4.8 Inventory and discovery

- **G1 Cloud and tool import** (P1, L). Pull hosts from AWS EC2 (and SSM
  Session Manager as a transport), GCP, Azure, DigitalOcean, Hetzner, Proxmox,
  Tailscale, and Kubernetes nodes and pods. Group by tag or region, refresh on a
  schedule or on demand, mark which came from where, never overwrite what the
  person edited. Royal TS calls this dynamic folders; Termius imports some
  clouds. *Done:* an import wizard per source, credentials kept in the vault,
  and a "refresh" that shows what was added and removed.
- **G2 Network scan** (P3, M). Find SSH hosts on a subnet and add them.
- **G3 Ansible and Terraform round trip** (P2, M). Import exists for Ansible
  inventories; add export, and read hosts from a Terraform state file.
- **G4 Teleport, Boundary, SSM, `kubectl exec`, `gcloud compute ssh` as host
  types** (P2, M). A host whose "connect" is a command the app runs for you.

### 4.9 Automation

- **A1 Runbooks** (P1, L). A saved sequence of steps: run a command, wait for
  text, upload a file, ask for a value, branch on the exit code. Parameters,
  dry run, run on one host or a group with the existing concurrency limit, a
  history with per-host output, and approvals on production. Snippets are one
  step of this. SecureCRT, MobaXterm macros and Warp workflows are the
  comparison. Keep it declarative (a file you can read and diff), not a new
  programming language.
- **A2 Scheduled runs** (P2, M). Run a runbook or snippet on a schedule while the
  app is open (and say clearly that it does not run when the app is closed, or
  add a small background service later).
- **A3 Expect-style send and wait** (P2, S). Per host: "after connecting, wait
  for `password:` and send ...". Never for secrets typed into prompts unless
  K3 is decided.
- **A4 Hooks** (P3, S). Run something before or after a connection (start a VPN,
  open a tunnel, update a status page).
- **A5 Plugins** (P3, L). Tabby is the model: themes, extra host types, panels.
  This needs a stable, sandboxed API first; do not start until the core
  features above settle.

### 4.10 Monitoring and operations (built in 0.19.0)

`feat.md` M1 covers per-host charts and processes. Needs beyond it:

- **O1 Alerts** ✓ (P2, M). Notify when a host goes down or a threshold is crossed,
  while the app runs; quiet hours; per host or group.
- **O2 Log viewer** ✓ (P2, M). `journalctl -f` and `tail -f` over several hosts at
  once, with filter, highlight (TB2) and pause. The containers view has the
  single-container version.
- **O3 Service manager** ✓ (P2, S). List systemd units, start, stop, restart,
  with the destructive-action guard on production.
- **O4 History that survives** ✓ (P3, S). Keep the 15-minute charts for a day or a
  week on disk, optionally.

### 4.11 Databases and containers (DC1 built in 0.20.0; the rest stays in `feat.md`)

Already planned: D3 to D8 (SQLite, MSSQL, Redis, MongoDB, key browsers), C2 to C5
(Podman verified, Compose editing, Kubernetes). One need not in `feat.md`:

- **DC1 Kubernetes** ✓ (P2, L). Pods, logs, exec and port-forward for a context,
  next to containers. kubectl is the common denominator; use it over the same
  transport.

### 4.12 Everyday experience (built in 0.21.0; see the notes under each item)

- **UX1 First run** ✓ _tour, palette and sidebar steps, from the welcome dialog or the palette; 6 steps; keyboard, focus, 3 themes checked_. (P1, S). A guided start: create or open a vault, import from
  `~/.ssh/config`, connect to the first host, and a two-minute tour of the
  palette, the list and Manage. The new logo and empty-state art are a start.
- **UX2 Accessibility pass** ✓ _axe-core clean in all three themes on 14 pages and dialogs; high-contrast theme added; **not** tested with NVDA or Orca_. (P1, M). Test with NVDA and Orca; focus order in the
  sidebar and Manage menu; contrast in both themes; every animation respects
  reduced motion (done globally); a high-contrast theme.
- **UX3 Languages** ✓ _infrastructure plus the sidebar, menus, headings and tour in 7 languages (**dialogs and most settings still English; translations unreviewed by natives; no RTL layout mirroring**)_. (P2, M; `feat.md` S1 and S2). Start with strings extracted
  and Spanish, German, Chinese, Russian, Portuguese, Japanese, Persian.
- **UX4 Search everything** ✓ _keys, credentials, groups, trusted servers and settings in the palette_. (P2, S). The palette already finds hosts and
  actions; add snippets, keys, credentials, groups, settings, and known hosts.
- **UX5 Customisable layout** ✓ _labels on/off, hide into Manage, reorder; no drag to resize the rail_. (P3, M). Resize and hide the rail labels, pin
  items, reorder the sections.
- **UX6 Snippet library** ✓ _25 read-only starters, import and export of packs with validation_. (P3, S). Built-in starter snippets (disk, memory,
  logs, docker) and import and export of snippet packs.
- **UX7 Themes** ✓ _10 more themes (AA contrast tested), two high-contrast_. (P3, S). More built-in themes and a gallery; imports from VS
  Code, Windows Terminal and iTerm2 already exist.
- **UX8 Speed with big fleets** ✓ _windowed host list; measured 22 s → 3 s, 670 → 82 MB at 5,000 hosts; start-up and memory budgets **not** added to CI_. (P2, S). Virtualise the host list for 5,000+
  hosts; measure start-up time and memory per tab and set budgets in CI.

### 4.13 Sync and data

- **SY1 History and restore** (P2, M). A timeline of vault changes per host,
  restore one record, not just the whole backup.
- **SY2 Selective sync** (P3, S). Keep some groups on this computer only.
- **SY3 More sync paths** (P3, M). An optional relay for people who do not want
  a shared folder: end-to-end encrypted, self-hostable, no account required for
  a single user. Only if there is demand; the folder model is a feature.
- **SY4 Import and export breadth** (P2, S each). Termius export (CSV done),
  1Password and Bitwarden SSH entries, SecureCRT and Royal TS files, `known_hosts`
  both ways.
- **SY5 Mobile companion** (P3, L). Declared out of scope in `feat.md`. If it
  is ever reconsidered: read-only host list and a terminal, using the same
  encrypted folder, nothing else.

---

## 5. Quality debts to clear first

Not features, but each makes everything above more expensive if left:

1. **Nothing has run in the real Tauri window here.** All UI checks used headless
   Chromium with a mocked backend (R3).
2. **Unproven corners** from `feat.md`: MySQL TLS against a real server, Podman
   and nerdctl parsers (fixtures from the docs), macOS and BSD monitoring,
   Docker volumes and Compose on versions other than Docker 29 and Compose v5.
3. **Smart completion** was verified on Linux with bash 5.2, zsh 5.9 and fish
   3.7, with WebGL rendering, Linux `sshd`, and dash as `sh`. Not on macOS or
   Windows; not with zsh-autosuggestions; not with a host whose `exec` is
   disabled; not with the DOM renderer.
4. **Split panes and synchronized input** were not re-tested with smart
   completion on.
5. **The browser test scripts are not in CI** and need a dev server and
   Playwright set up by hand.
6. **No fuzzing** on the parsers that take untrusted text: OSC handling, ssh
   config and PuTTY/MobaXterm imports, the completion line parser, spec loading.
7. **Release builds are not observed**: there is no way here to see whether the
   GitHub build for a tag succeeded; check the Actions tab after each release.

---

## 6. Still open from `feat.md`, for one list

- **Files:** F3 SMB, F4 WebDAV, F5 S3, F6 ZMODEM.
- **Terminals:** T4 VNC, T5 SPICE.
- **Containers:** C2 Podman (needs a real Podman), C3 to C5.
- **Databases:** D3 to D8 (key-browser design first).
- **Monitoring:** M1's last line, a recording from real macOS or BSD.
- **AI assistant:** A0 to A4. Note the owner decided against AI in completion;
  whether an assistant is wanted at all is its own decision.
- **Sync and languages:** S1, S2, P1.
- **Decisions waiting on the owner:** Podman for C2, the key-browser design, the
  never-published `v0.13.0` tag.

---

## 7. Decisions for the owner

1. **macOS.** It is the biggest single audience gap and needs a Mac (or a CI
   runner) and an Apple developer account. Do it, or say plainly that the app is
   Linux and Windows only?
2. **Signing.** Which certificate or service for Windows, and who holds the
   secrets? Without it, R1 and the in-app updater stay as they are.
3. **Teams or individuals.** TS1 to TS5 turn a personal tool into a team tool,
   with support and security expectations to match. Is that the direction?
4. **Plugins.** A public API is a long commitment. Not before the core is stable
   (A5).
5. **AI.** Keep it out entirely, or an optional assistant with strict limits
   (`feat.md` Phase 6)? Smart completion stays AI-free either way.
6. **Cloud sync relay** (SY3). Keep the folder-only model as the identity of the
   product, or add an optional relay?
7. **Mobile.** Stay out of scope, or a read-only companion later?

