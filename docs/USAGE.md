<!-- The full guide to SSHVault's features. The overview is in the README. -->
# Using SSHVault

## Hosts and identities

- Create a host with the **+** button in the **Hosts** view. The form has
  four tabs: **Connection** (address and credentials), **Route** (jump host,
  proxy, agent/X11 forwarding, keep-alive), **Organise** (group, environment,
  tags, colour, notes) and **Automation** (a startup command and which of
  this host's tunnels start automatically). **Test connection** checks
  reachability for an already-saved host. Enter the
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

## Viewing and copying saved passwords

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

## Working with many servers

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
- **Fleet view.** The activity icon next to a group in the host tree, or
  "Go to Fleet" in the command palette, opens a tile per host with
  reachability colour-coding and a 30-second auto-refresh. Turn on
  **monitoring** for a host (on its details card, or the small activity icon
  on its tile) to also sample CPU, memory, disk, load and uptime every 30
  seconds, over the same SSH connection mechanism as everything else — no
  agent, nothing installed on the host. Off by default, per host, and never
  synced; needs a saved credential, since it runs unattended. On macOS and
  FreeBSD, CPU comes from `top` and memory from `vm_stat` or `sysctl`; on
  another BSD only disk, load and uptime are shown. (The macOS and FreeBSD
  readings are written from the manuals and have not yet been run on a real
  Mac or BSD machine.)
- **Detail view.** **Open detail view…** on a host's details card, or the gauge
  icon on its Fleet tile, opens a window for that one host. It keeps a single
  SSH connection open while the window is open (not a new login per
  refresh) and reads the host every 5 seconds (3, 10 or 30 if you prefer),
  only while the window is visible. **Pause** stops it. Nothing is installed
  and nothing is changed: the script only reads. Needs a saved credential.
  - **Overview** charts CPU, memory and network throughput (received and sent,
    everything but loopback) for the last 15 minutes. Hover, or focus a chart
    and use the arrow keys, to read the exact values; **Show as a table** lists
    every reading. The history is kept **in memory only**: never written to
    disk, never synced, and locking the vault clears it.
  - **Processes** lists what is running with its user, CPU (a share of one
    core over the last second, so it can pass 100) and memory. Sort by any
    column, and search by name, user or pid. **Terminate** asks a process to
    stop (SIGTERM) after a confirmation; **Force kill** (SIGKILL) is a
    separate button with its own, stronger confirmation, because the process
    can't clean up. Just before signalling, the host is asked whether that
    process number still belongs to the same program, so a number that was
    reused in the meantime is left alone and you are told. The first process
    (pid 1) can't be signalled here. A process of another user may be
    refused by the host; its answer is shown. On a production host, both ask
    you to type the host's name.
  - **Ports** lists what is listening, with the process that owns each. That
    needs `ss` or `netstat` on the host, and the host only tells you about
    your own processes unless you are root: the others are marked "owner not
    shown". Without either tool the listeners still appear, without owners.
  - **Interfaces** lists each network interface with its state, addresses,
    MAC, MTU and current throughput. Without `ip` or `ifconfig` on the host,
    the addresses aren't shown, and the window says so.
  - Linux needs nothing beyond a shell and `awk` (processes are read straight
    from `/proc`, so even a host without `ps` works). On macOS and FreeBSD the
    system's own tools are used, CPU per process is the system's figure, and
    the CPU and memory charts need monitoring switched on (the 30-second
    summary supplies them). The window says which panels a host can't fill.
- **Local terminals.** **Ctrl+Shift+`**, or the terminal icon next to the tabs,
  opens your own shell in a tab: your login shell on Linux and macOS, the
  default shell on Windows. The arrow beside the icon lists every shell
  found on the computer (and, on Windows, each WSL distribution, which opens
  in its own home folder); **Settings → Local terminal** sets which one the
  plain button uses. Local tabs work with splits, recording, snippets
  and synchronized typing like SSH tabs.

- **Remote Desktop (RDP).** In a host's form choose **Remote Desktop (RDP)**
  as the protocol (the port becomes 3389). Set a domain if the account needs
  one (or write `DOMAIN\user` as the user name), a screen size (or **Fit the
  tab**), the colour depth and the sign-in security, and attach a credential
  with a user name and password. Double-click the host to open it in a tab.
  - The first time, the server's certificate is shown (subject, issuer, dates
    and SHA-256 fingerprint). Trust it and it is remembered for that host;
    nothing, including your password, is sent before you do. If a different
    certificate shows up later the connection stops and shows both fingerprints:
    that is normal after a server is reinstalled or its certificate renewed, and
    also what someone intercepting the connection looks like. **Forget it** in
    the host's form to be asked again.
  - Keys you type go to the remote computer while its screen has the focus;
    the app's own shortcuts (Ctrl+Shift combinations and the fixed tab keys)
    still work first. **Ctrl+Alt+Del** is a button in the tab's toolbar. The
    clipboard button shares text both ways (text only; turn it off to keep the
    two clipboards apart). The last button shows the screen at its real size,
    with scrolling, instead of shrinking it to the tab.
  - **Sign-in security:** *Automatic* uses Network Level Authentication when
    the server offers it and TLS otherwise; *Require NLA* refuses a server that
    doesn't offer it instead of falling back; *TLS only* leaves signing in to
    the server's own screen.
  - Limits: no sound, drive or printer redirection; the host is reached
    directly, so jump hosts and proxies aren't used; the screen size is fixed
    when you connect (close and reopen the tab to change it); Kerberos isn't
    supported (NTLM only); a key-based credential can't be used.
- **VNC.** In a host's form choose **VNC** (the port becomes 5900: 5900 is
  display `:0`, 5901 is `:1`). By default the connection goes **through SSH**:
  the app signs in over SSH to the same machine (with the host's credential,
  jump host and proxy, and trusted-server rules, on the SSH port in the VNC
  settings) and reaches the VNC port from there, because VNC's own password
  scheme is weak and its picture is not encrypted. Untick *Connect through SSH*
  only on a network you trust. The **VNC password** is asked for when the
  server wants one, is not saved, and only its first eight characters count.
  The tab works like an RDP tab (keys, mouse, wheel, Ctrl+Alt+Del, shared
  clipboard text, actual size). Limits: US keyboard layout (keys are sent as
  symbols), clipboard text is Latin-1, only the Raw and CopyRect picture
  encodings (a server falls back to them, using more bandwidth than a
  compressed one would), no TLS or VeNCrypt or Apple logins, no SPICE.
  Tested against a protocol simulator, not a real VNC server.
- **Wake-on-LAN.** In a host's form, under **Automation**, enter the machine's
  MAC address (and, for another subnet, its broadcast address such as
  `192.168.1.255`). **Send a wake-up now** tests it; when a connection to that
  host fails, **Wake it up** appears next to Reconnect. It only reaches a
  machine on the same network as this computer, which must be set up to wake
  (BIOS and network card); nothing here can tell whether it did. There is no
  scheduled wake yet.
- **Importing from a cloud or tool.** The cloud button above the host list (or
  the command palette: *Import hosts from a cloud, Tailscale, Kubernetes or
  Terraform…*) lists machines from AWS EC2, Google Cloud, Azure, DigitalOcean,
  Hetzner, Tailscale, Kubernetes nodes, or Terraform (a folder, or a state
  file). It runs that product's own command-line program on this computer, so
  you sign in with the program as you already do and this app never sees the
  credentials; the exact command is shown before it runs, and what it prints is
  read as data. The optional field is the region, project, subscription or
  context. You see what would be **added**, what **changed** since the last
  import, and what is **no longer listed**, and tick what to apply. A refresh
  updates an address or name only while you haven't edited it, keeps hosts that
  disappeared unless you tick them, and never touches your notes, groups, tags or
  ports. Imported hosts remember where they came from. Windows machines
  become RDP hosts. Proxmox is not covered (it needs an API token, not a
  program).
- **Scanning a network.** *Scan the network for SSH servers…* (palette)
  connects once to each address of a private network (up to 4,096 addresses;
  only 10.x, 172.16–31.x, 192.168.x, 100.64–127.x, 169.254.x) and lists those
  that answer with an SSH banner, to add as hosts.
- **Hosts that connect with a command.** Choose **A command** as a host's
  protocol, for AWS Session Manager (`aws ssm start-session --target {id}`),
  `gcloud compute ssh`, Teleport, Boundary, `kubectl exec`, Tailscale SSH. Opening
  the host shows the exact command and asks you to approve it, then runs it in a
  local terminal. `{host}`, `{user}` and `{id}` are replaced by the host's
  values, and only when they have nothing a shell could misread; anything else
  in braces is refused. An imported host's `{id}` is the id the provider gave it.
- **Ansible.** *Export hosts as an Ansible inventory…* (palette) writes a
  `.json` inventory (Ansible reads JSON in its YAML inventory format): groups
  follow your folders, each host has `ansible_host`, `ansible_port` and
  `ansible_user`. Import from an inventory already exists.
- **Mosh.** In a host's form, tick **Use Mosh**. Connecting then logs in with
  SSH as usual (vault keys, jump hosts, proxies and trusted-server rules all
  apply), starts `mosh-server` on the host, and runs your computer's
  `mosh-client` in the tab. The session keeps going across Wi-Fi changes and
  sleep; `mosh-client` shows its own "last contact" bar while the link is down.
  If the host has no `mosh-server`, the error offers **Use plain SSH**.
  Limits: Mosh must be installed on this computer (`apt install mosh`,
  `brew install mosh`; on Windows only if a `mosh-client.exe` is on the PATH),
  the host must be reachable directly over UDP ports 60000–61000 (a jump host
  carries only the login, not the session), there is no scrollback from the
  server (use `tmux` or `screen` there), and port, agent and X11 forwarding
  don't work over Mosh.

## Importing from an Ansible inventory

In the import dialog, choose **Ansible inventory…** and pick an INI
inventory. SSHVault reads groups, `[group:vars]`, `[group:children]`,
`[all:vars]`, host ranges like `web[01:03]`, and the variables
`ansible_host`, `ansible_port`, `ansible_user` and
`ansible_ssh_private_key_file`, including their older `ansible_ssh_*` names.
A `ProxyJump` or `-J` in `ansible_ssh_common_args` becomes the jump host.
Hosts land in folders that mirror the group tree, such as `prod/web`.

## Exporting to `~/.ssh/config`

**Settings → Use your hosts from the command line** saves every host as an
OpenSSH config file, or copies it. Add `Include /path/to/sshvault.config` to
`~/.ssh/config`, and `ssh web-1` works from any shell with the same names,
users, ports and jump hosts. Private keys stay in the vault, so the file has
no `IdentityFile` lines, and `ssh` uses your agent or default keys.

## Importing from `~/.ssh/config`

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

## Keys

The **Keys** screen is the Key Manager: generate Ed25519, ECDSA or RSA keys,
import private keys (OpenSSH, PEM, PKCS#8, PuTTY) or colleagues' public
keys, copy a public key for `~/.ssh/authorized_keys`, attach an OpenSSH
certificate, change a passphrase, and see which credentials and hosts use a
key. Credentials refer to keys, so replacing a key applies everywhere.
Exporting a private key asks for the master password every time and writes an
owner-only file.

## SSH agent

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

## Command line

Turn on **Settings → Command line**, then script against your hosts while
the vault is unlocked:

```sh
sshvault status
sshvault list --group Production
sshvault connect web-01                         # opens a tab in the app
sshvault run --tag frontend -- uptime           # asks you in the app first
sshvault run web-01 db-01 --json -- df -h /     # JSON lines for scripts
```

The CLI is the SSHVault program itself; Settings shows the alias to add if
it isn't on your PATH. It talks to the running app over a socket only you
can open, so it never sees passwords or keys. Every `run` is approved in
the app. A 10-minute "don't ask again" never covers production hosts. Exit
codes: 0 success, 1 a host failed, 2 usage error, 3 the app isn't reachable.

## Telnet and serial consoles

For switches, routers and devices without SSH:

- **Telnet**: set a host's protocol to Telnet, or type `telnet://host:port`
  in Quick connect. You log in inside the terminal and nothing is saved for
  it. Telnet is unencrypted, and its panes say so.
- **Serial**: Quick connect → **Serial console…** (or the command palette).
  Pick the port, baud rate and line settings (9600 8N1 by default). On
  Linux your user may need to be in the `dialout` group.

## Jump hosts

In the host form, pick another saved host under **Jump host**. The form shows
the full route, such as `bastion → gateway → this host`. Terminals, SFTP and
port forwarding all go through the chain.

Every jump host needs an identity attached, because only the final host can
ask you for one-time credentials. SSHVault refuses to save a chain that loops.

For a one-off connection, type an `ssh -J` command in Quick connect instead
(see below). **Copy as ssh command** on a host gives you the same syntax.

## Terminals

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
- Right-click inside a terminal for copy, paste, select all, find, clear and
  copying the entire scroll buffer.
- Ctrl+click an absolute or `./relative` path in the output to browse to it
  in SFTP, or a `host:port` to quick connect to it.
- Drop a file from your file manager onto a terminal to upload it over SFTP
  to that pane's current directory (needs shell integration to know it;
  otherwise it uses the home directory), then the remote path is typed at
  the cursor. Drop plain text to paste it.
- Pasting something that looks like a private key or an API token asks you
  to confirm first; turn this off under **Settings → Connections**, where
  you can also have one trailing newline dropped from every paste.
- When a program such as tmux, vim or Claude Code takes the mouse, hold
  **Shift** while dragging to select text, or click the **Program has the
  mouse** pill to make plain drags select until you switch back.
- Programs that copy through the terminal (tmux's mouse selection with
  `set-clipboard on`, Claude Code, Neovim's OSC 52 provider) put the text on
  your clipboard. Turn this off under **Settings → Terminal** if you would
  rather remote programs could not write to it.
- If the connection drops, a **Reconnect** button appears at the bottom of the
  pane.
- **Command blocks.** On a host with shell integration, each command gets a thin bar in the left margin: green
  if it succeeded, red if it failed, grey if the shell didn't say. Hover for the command, outcome and how long
  it took. Click it to copy the output or the command, select the output, or pin the command (pinned bars are
  purple). Right-click → *Previous / Next failed command* jumps between failures. Turn the bars off under
  **Settings → Terminal**.
- **Highlight words.** Under **Settings → Terminal**, add rules that colour words in the output (plain text or a
  pattern, ordered; earlier rules win where two overlap). Start from Errors, Warnings, Success or IP addresses.
  *Notify me* raises a desktop notice when new output matches while the app is in the background. Only the lines
  on screen are coloured, so a large output stays fast.
- **Search all terminals.** Command palette → *Search all open terminals…* looks through the screen and
  scrollback of every open terminal; choosing a result switches to that tab and selects the line.
- **A look for one host.** In the host's form, *Terminal look on this host* sets a theme, text size and
  scrollback just for it (handy for telling production apart). Empty fields follow Settings, and zooming still
  works on top of the host's size.
- **Shell integration in one click.** Right-click in a pane → *Install shell integration on this host…* shows the
  exact lines for the host's login shell (bash, zsh or fish), then adds or removes them. A copy of the file
  is kept as `<file>.sshvault.bak`, and on production hosts you type the host's name first.
- **Links and pictures.** Ctrl/Cmd+click a link a program printed. Only web and mail addresses are followed,
  and you are asked first when the address differs from the text shown. A path with `:line:col` browses to the
  file. Sixel and iTerm2 pictures are drawn in the terminal (Settings can turn this off).
- Change the colour theme, font, cursor, scrollback, letter spacing, padding,
  minimum contrast and cursor colour under **Settings**, with a live preview
  next to the controls. These settings are per computer and are not synced.
  Settings has a category list on the left and a search box that finds a
  setting by name across every category.

## Smart completion

Suggests the rest of a command while you type at a shell prompt. **It is off by
default**: turn it on under **Settings → Terminal → Smart completion**. Nothing is
typed for you and nothing is sent anywhere except, if you allow it, a question
to the host you are already connected to. The switch applies to open tabs
immediately; off means no suggestion, no key handled, no lookup and nothing
recorded.

**What it does**

- **As you type**, a faint suggestion appears after the cursor, taken from
  commands you ran before (this host first, then the same folder, then what
  followed the last command). **→** or **End** accepts it, **Ctrl+→** takes one
  word, **Esc** hides it. Accepting only types the missing characters; **Enter
  is always yours**, and on a production host it still goes through the
  destructive-command check.
- **Ctrl+Space** opens a list for the line as typed, and typing narrows it.
  Up and Down move, **Enter** (or Tab) puts the choice on the line in place of
  what you typed, **Esc** closes it. It lists, under their own headings:
  subcommands, options and values for about 60 common commands (git, docker,
  kubectl, systemctl, tar, ssh and more, with descriptions, also behind `sudo`
  and `env`); file and folder names from the host (`cat /etc/ho`, `~/`, relative
  paths); names such as git branches, containers and systemd units where a
  command takes one; matching history, fuzzy; and your snippets (single-line
  ones; a snippet with `{{variables}}` asks for them first). Tab is left to the
  shell's own completion; you can bind it to the list under **Keyboard
  shortcuts** (Open the suggestion list (Tab)).
- Each part has its own switch in Settings, and each host can override them:
  **Use default / Always on / History only / Off** (host form → Automation).
  **Production hosts default to History only**: the faint suggestion and the
  list work from your own history, but no snippets, command specs or lookups on
  the host.

**Where history comes from.** From the commands typed in this session. They are
kept for next time only if **Remember the commands I run on each host** is on.
Commands that look like they contain a password, token or key
(`--password …`, `TOKEN=…`, `user:pass@host`, long key-like strings), commands you
start with a space, and commands that failed to start are never suggested or
stored. This is a best-effort filter, not a guarantee.

**Looking things up on the host.** File names and the lists above come from a
second channel on the same SSH connection, never from the shell you are typing
in. The folder and what you typed reach the host as arguments, never as part of
a command, so a file named `$(rm -rf ~)` is just a name. A lookup is capped at
500 names and three seconds, and a listing that was cut says so. If the host
won't open another channel (`MaxSessions 1`, exec turned off) the app stops
asking for that session and nothing else is affected. Relative paths need the
shell to report its folder (see shell integration below). Local tabs list file
and folder names on this computer (nothing is sent anywhere, and the named
lists such as branches and units are not offered there). Mosh, Telnet and
serial consoles have no connection to ask over, so they get history, snippets
and command options but not file names.

**Your saved hosts.** After `ssh`, `scp`, `sftp` or `mosh`, the list offers the
hosts in your vault whose name matches, and puts `user@host` on the line (with
`-p 2222` in front when the port isn't 22; for `scp`, `user@host:`). Only SSH
hosts, and only names that are safe to type, are offered. Nothing is asked of
any host for this.

**Abbreviations.** Give a snippet an *Abbreviation* (one word, such as `gco`).
Typing it in full and opening the list puts that snippet first. Nothing is
expanded until you accept it.

**Learning from the host's own history.** Under Settings → Smart completion,
*Learn from the host's own shell history* (off by default, and never on
production hosts) reads the last few hundred lines of `.bash_history`,
`.zsh_history` or fish's history once per host per run, over the same extra
channel, so a new computer isn't empty. The lines go through the same filter as
commands you type: anything that looks like a password or token is skipped.

**Which commands it knows.** The bundled specs come from the MIT-licensed
`@withfig/autocomplete` package; Settings shows the version. A few commands the
package lacks (`journalctl`, `apt-get`, `dnf`, `ip`, `ss`, `awk`, and kubectl's
`--context`) were written for this app in `scripts/completion-extra.mjs`.
`node scripts/build-completion-specs.mjs --check` says whether a newer package
exists. Old-style `tar xzf archive.tgz` is understood too.

**When nothing appears.** In full-screen programs (vim, tmux, top, less), while
a command runs, at password prompts (including `sudo`'s), while the mouse is
tracked, in a command you are editing over several lines (a multi-line paste),
when text already follows the cursor (a right-hand prompt, or the grey
suggestion of fish or zsh-autosuggestions, so you never get two), after the
window was resized until the next prompt, when scrolled back, and with a screen
reader on (the list is announced instead of faint text).

**Shell integration** makes the line exact: it lets the app read what you typed
from the screen. Settings → Shell integration has the lines to add for bash,
zsh and fish (the fish one is needed even for fish 3.7, which sends no marks).
Without it the app follows your keystrokes and checks them against the screen,
and shows nothing when they disagree. PowerShell and cmd have no integration
yet, so they get no suggestions.

## Operations: logs, services and alerts

**Ops** in the sidebar (or *Go to Operations* in the command palette) has three tabs. All of
them use a second connection to each host, made with its saved credentials, so they never
ask for a password and nothing is installed on the host.

- **Logs.** Tick one or more hosts, choose the system journal (a unit, a priority and
  above, or the kernel) or a log file, and press **Follow**. Lines from all the hosts arrive
  in one list, tagged by host. Filter by text (or a regular expression), by level (error,
  warning, info, debug: guessed from the words in the line) or by host; matches are
  highlighted; **Pause** freezes the view while lines keep arriving; copy what is shown. Only the
  newest 5,000 lines are kept. The unit and the file path reach the host as one quoted
  word, never as part of a command. The journal needs a user who may read it.
- **Services.** Pick a host to list its systemd services with state and whether they
  start at boot. Start, stop, restart, reload, enable or disable one; read its status; jump
  to its log. Anything that needs root can use **sudo**, which works only where sudo
  asks no password (`sudo -n`), and fails at once where it does. On a production host
  stopping, restarting or disabling asks you to type the host's name first. Hosts
  without systemd say so.
- **Alerts.** Off until you turn them on in **Settings → Alerts and monitoring
  history**. They watch the hosts you turned monitoring on for (Fleet) while the app is
  open: a host that stops answering (two checks in a row, a minute apart), and CPU, memory
  or disk over a limit for several readings in a row, each with a message when it is back
  to normal. They show a notice, an operating-system notification when the window isn't in
  front, and a list on the Alerts tab. Quiet hours keep alerts off the screen (they still
  reach the list); any host can be muted. Alerts are not saved and nothing leaves this
  computer, and they do not run when the app is closed.

**Longer charts.** The monitoring charts show the last 15 minutes. Under **Settings →
Alerts and monitoring history** you can keep one-minute averages for a day or a week
instead; the host's detail view then offers 24 h and 7 days. They are stored on this
computer, not synced, and not encrypted; choosing 15 minutes again deletes them.

## Runbooks and automation

**Runbooks** (the checklist button on the left rail) are saved sequences of
steps with parameters, run on one host or many, with a record of every run.
A runbook is a JSON document you can read, diff and share (export and open
files from the page), not a program:

```json
{
  "name": "Restart a service",
  "params": [{ "name": "service", "label": "Service", "default": "nginx" }],
  "steps": [
    { "name": "Is it running?", "id": "before", "run": "systemctl is-active {{service|q}}", "on_error": "continue" },
    { "name": "Restart", "run": "sudo systemctl restart {{service|q}}", "when": { "step": "before", "exit_not": 0 } },
    { "name": "Wait until it is up", "wait": { "run": "systemctl is-active {{service|q}}", "every_secs": 2, "timeout_secs": 60 } }
  ]
}
```

- **Steps** are `run` (a command), `wait` (repeat a command every few seconds
  until it exits with `until_exit`, default 0, and prints `contains` if given,
  or give up after `timeout_secs`), or `upload` (see below). A failed step
  stops that host unless the step has `"on_error": "continue"`; other hosts go
  on. Each step may have a `timeout_secs`.
- **Parameters** are asked for when you run it. `{{name}}` puts the value in as
  typed; `{{name|q}}` quotes it for the shell, which you should use for
  anything a person types; `{{name|raw}}` says outright that you want it as
  typed. `{{host}}` and `{{label}}` are the host's own address and name. They
  come from host records, which an import or a synced vault can fill, so they
  are quoted for you (a plain name looks the same); use `{{host|raw}}` only if
  you mean shell syntax. A parameter can have `choices`, `optional`, or `"kind":
  "file"`.
- **Conditions.** A step runs `when` an earlier step (by its `id`) exited with
  `exit` or not (`exit_not`), or a text parameter `equals` / `not_equals`
  something. Nothing else: there are no loops and no scripting.
- **Upload** puts a file on the host. Its source must be exactly `{{name}}` of
  a `"kind": "file"` parameter: you choose the file when you run it. A
  runbook can't name a file on this computer by itself, so a runbook someone
  shared can't send your private keys anywhere. It is written next to its
  target and moved into place, and may set a `mode` such as `"0644"`.
- The page checks the document as you type and shows the problem against its
  step; nothing can run until it is clean. A **dry run** shows every step
  filled in for the first chosen host and runs nothing.
- **Running.** Choose hosts (or add a whole group) and press Run. Each host shows
  its steps as they happen, with what they printed, and **Stop** ends the run
  (what happened so far is kept). Several hosts go at once, eight at a time,
  each over one SSH connection that needs saved credentials. If any chosen host
  is a **production** host, you must type the runbook's name first.
- **History** keeps the last 100 runs on this computer only (never synced or
  backed up, because outputs can hold secrets), each with its values, per-host
  steps and output. Each output is cut at 16 KB.
- **Schedules** run a runbook by themselves (every N minutes, daily, or on
  chosen days), **only while SSHVault is open** and the vault is unlocked. A
  time that passes while the app is closed or the computer sleeps is skipped,
  not caught up. Schedules are kept on this computer. A schedule that includes
  a production host is skipped unless you allowed that, with a typed
  confirmation, when you made it; runbooks that need a file chosen each time
  can't be scheduled. Failures are reported and recorded.

**Wait for this, send that.** Under a host's **Automation** tab, add steps that
wait for some text in the output after connecting and send an answer (for a
banner, a menu or a jump box). Each gives up after its time (30 s by default)
and drops the rest. It can't wait for or send a password, code or token: those
are refused, because the text is stored as plain text in the vault.

**Commands before and after.** A host can run a command on this computer
before it opens (start a VPN, open a tunnel) and after its tab closes. The exact
command is shown and you approve it first (tick *Don't ask again* to remember
that exact command); if the before-command fails, you are asked whether to
connect anyway. `{host}`, `{user}` and `{id}` are filled in only when they are
safe on a command line. The after-command runs only if it was approved when the
host was opened.

Plugins (an extension API) are not built: a stable, sandboxed API should come
after the rest has settled.

## Finding your way: the tour, the sidebar, languages and themes

- **The tour.** *Show me around* in the welcome dialog, or *Take the tour* in the command palette, walks
  through the sidebar, the host list, Manage, the palette and quick connect in six short steps, pointing at the real
  thing. Arrow keys or Enter move on, Esc ends it.
- **The command palette finds more.** Besides hosts, snippets, tunnels, workspaces and actions it now finds
  **keys**, **credentials**, **groups** (it shows that group's hosts), **trusted servers** and **settings** (*Setting:
  Font and font size* opens Settings already searched for it).
- **The sidebar is yours to arrange.** **Settings → Appearance → Sidebar** turns the names under the icons on or off,
  puts entries away (they move into the **Manage** menu, one click away) and moves them up and down within their
  group. Hosts always stays. Reset puts everything back.
- **Languages.** **Settings → Appearance → Language** changes the sidebar, menus, headings and the tour to Spanish,
  German, Portuguese, Russian, Chinese, Japanese or Persian, or follows the system. Dialogs and most of Settings are
  still in English. The translations were written for this app and have not been reviewed by native speakers;
  corrections are welcome. Persian text is shown, but the layout is not mirrored.
- **Themes.** Ten more terminal themes (Catppuccin Mocha and Latte, Monokai, Rosé Pine, Everforest, Night Owl,
  Palenight and two high-contrast ones) join the gallery. **App theme → High contrast** gives black and white with
  visible edges: text is 7:1 or better.
- **Snippet packs.** The **library** button on the Snippets list adds a **starter set** (disk, memory, processes,
  services, logs, network, Docker and Git: read-only commands) or the snippets in a pack file; **export** saves
  yours as a `.json` pack. Snippets already in your list are not offered again, and nothing from a pack runs.
- **Thousands of hosts.** Past a few hundred rows the host list draws only what is on screen, so a vault of 5,000 hosts
  starts in seconds and scrolls smoothly. Search opens every group.

**Accessibility.** The pages and dialogs are checked with axe-core in the dark, light and high-contrast themes (no
serious findings). Every animation stops under *reduce motion*. A screen reader has labelled landmarks, a page heading
and a labelled command palette. Not tested with a real screen reader yet; the terminal itself (a canvas) is the
terminal's own accessibility mode, under **Settings → Terminal appearance → Screen reader mode**.

## Quick connect and the command palette

Press **Ctrl+Shift+T**, or the **+** next to the tabs, and type
`user@host` or `user@host:port`. Leave the password empty to use ssh-agent.
Nothing is saved.

To go through jump hosts, type it the way you would for OpenSSH:

```
ssh -J bastion root@10.0.1.5
ssh -J bastion,me@gw.internal:2222 -p 2200 root@db
```

A jump host written without a user (`bastion`) must be a saved host, found by
label or address; it connects with its saved credentials, proxy and jump
chain. One written with a user (`me@gw.internal`) uses the saved host with
that address and login if there is one, and otherwise connects with
ssh-agent. The dialog shows the route before you connect. The password field
is for the last host only. `-p`, `-l` and `-o ProxyJump=/Port=/User=` are
understood; other options are refused rather than silently ignored.

Press **Ctrl+Shift+P** to open the command palette. It searches hosts, recent
connections, snippets and actions. Typing `user@host` there offers a quick
connection too. Outside a terminal, **Ctrl+K** also opens it.

## Keyboard shortcuts

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
| Ctrl+=, Ctrl+-, Ctrl+0, Ctrl+scroll | Zoom in, out, reset |
| Ctrl+Shift+L | Lock vault |
| Ctrl+Shift+B | Type into all panes of the tab (toggle) |
| Ctrl+Shift+H | Hide or show the list panel |
| Ctrl+Shift+Enter | Maximize or restore the active pane |
| Ctrl+Shift+U | Focus mode: only the terminal |
| Ctrl+Shift+/ | Keyboard shortcuts (and rebinding them) |
| Ctrl+Shift+↑ / ↓ | Previous / next prompt (with shell integration) |
| Ctrl+Shift+` | New local terminal |
| Alt+Shift+arrows | Focus the pane to the left/right/up/down in a split |
| Shift+End | Jump a terminal to the bottom |
| →, End, Ctrl+→, Esc | Accept, accept one word, dismiss a suggestion (smart completion, when one is showing) |
| Ctrl+Space | Open the suggestion list (smart completion) |

Plain Ctrl shortcuts such as Ctrl+W, Ctrl+T and Ctrl+K go to the remote
shell, where editors and readline use them. Pane navigation uses Alt+Shift,
not plain Alt, so it doesn't take over Alt+Left/Right, which readline uses
for back/forward-a-word.

A split's divider can be dragged, or focused (Tab) and resized with the
arrow keys; double-click or Home/Enter centres it.

The host list is a full keyboard tree: arrow keys move between hosts and
groups, Left/Right collapses or expands a group, Home/End jump to the top
or bottom, typing jumps to the next matching name, F2 edits the focused
host and Delete removes it (after confirming).

## Snippets

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

## SFTP

Open the **SFTP** view. The left pane is this computer and the right pane is
the remote host.

1. Choose a host at the top of the right pane and click **Connect**.
2. Click a folder in the breadcrumb to jump to it, or the pencil icon next
   to it to type a path.
3. Select files with click, **Ctrl**+click or **Shift**+click.
4. Drag files to the other pane, or use **Upload →** and **← Download**.
5. Right-click a file for open, download/upload, copy path, rename,
   permissions and delete.

Click a column heading to sort by it; click again to reverse. The filter box
narrows the current folder by name. The eye icon in each pane's toolbar shows
or hides dotfiles, and folders are copied recursively. Drag the divider
between the two panes to resize them.

**When a name is already there**, a transfer asks what to do: **Replace**,
**Skip** that file, or **Keep both** (the new one becomes `name (1).ext`). A
folder you keep both of is copied beside the old one, not into it.

**Other kinds of server.** The same two panes browse whatever a host speaks:
- **SCP.** If a server has SFTP turned off, connecting says so and offers
  **Use SCP for this host**; the choice is saved on the host (or set
  **Files** to SCP in its form). SCP browses, uploads, downloads, renames,
  deletes and changes permissions, but can't continue a partly copied file or
  open a file for editing, and a file name can't contain a line break.
- **FTP and FTPS.** Choose **FTP / FTPS** as a host's protocol (port 21, or 990
  for implicit TLS), pick the encryption, and tick **Anonymous** if the server
  takes no login; otherwise attach a user name and password. Double-click the
  host, or choose it in the SFTP view. Explicit TLS (the default) and implicit
  TLS show the server's certificate the first time and trust it from then on,
  like an SSH host key; a different one later is refused before your password
  is sent. **None** works, but the tab says **UNENCRYPTED** and the password and
  files cross the network readable. FTP downloads and uploads continue where
  they stopped when you retry. If a server behind NAT announces a private
  address for its data connections, the address you connected to is used
  instead. Jump hosts and proxies aren't used for FTP.
- Each kind only offers what it can do: a server that can't rename or delete
  doesn't show those buttons or menu items.

To edit a remote file, double-click it, or select it and press the edit
button. It opens in this computer's default app for that file type. Each time
you save, it's uploaded back, and the list at the bottom of the SFTP view
shows the last upload. Press **Stop** when you're done. The local copy lives
in a private temporary folder and is deleted when you stop, disconnect, lock
the vault or quit. Symbolic links are listed, but not followed
during recursive copies. Transfers appear at the bottom with progress and a
cancel button.

## Port forwarding

Open the **Tunnels** view, add a rule, and press the play button.

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

## Databases

Open **Databases** in the activity bar (or **Ctrl+Shift+P** → "Go to
Databases"). It browses and queries MySQL, MariaDB, PostgreSQL, SQL Server,
Oracle, rqlite, Redis, MongoDB and Elasticsearch / OpenSearch servers, all in
the same view: one list of connections, one tree, one console, one grid. The
sections below say what each engine adds or changes.

**Add a connection** with the **+** button: name, server, port, user,
password, and optionally a database to start in. **Test connection** tries
the values without saving. The password is stored in the vault, encrypted
like every other secret, and synced with it; it is never sent to the
window except as you type it.

**Connect through an SSH host.** Pick one of your saved hosts and the
database is reached through it, so the database port never has to be open
to your computer. The server address is then as seen from that host, often
`127.0.0.1`. A loopback port is opened for the length of the session only
and closes when you disconnect or lock the vault. The host needs an
identity, because connecting runs without prompting you.

**Encryption.** The default is TLS with the certificate checked. "Don't
verify" still encrypts but accepts any certificate. "None" is marked
unencrypted in the form and in the list. For PostgreSQL the certificate is
checked against your operating system's trust store, so a company CA that is
already installed works. Through an SSH host, the certificate is checked
against the server name you entered (as the SSH host sees it), not against
the loopback address.

**PostgreSQL.** A connection is to one database (the one named in the form,
`postgres` if left empty), so its tree starts at the **schemas** of that
database; add a connection for each database you want to browse. Tables,
views, materialized views, foreign and partitioned tables are listed, with
their columns and indexes below. A statement that can't be described ahead of
time, such as a script of several statements, shows its values as text and
only its first result set.

### The other engines

Each engine's form shows only what it has. What it opens, and how it differs:

- **SQL Server.** A connection is to one database (the form's, or the
  login's default), so the tree starts at its **schemas**, then tables and
  views, with columns and indexes below. SQL logins work everywhere;
  `DOMAIN\user` Windows logins work in the Windows version only (the operating
  system signs in). A named instance is reached by its port. The TLS
  certificate is checked against the trust the driver carries; for a private
  CA, put its PEM file in *Certificate authority file*. Names are quoted as
  `[name]`; an edit is shown as the `UPDATE` with the values written in and
  runs with bound parameters. *Not yet tried against a real server.*
- **Oracle.** No Oracle client library is needed: SSHVault speaks Oracle's own
  protocol (12c or later). Give the **service name** (or a SID under the
  connection's settings). The tree starts at **schemas** (users): yours first,
  Oracle's own last. A data change you make (an `INSERT`, `UPDATE`, `DELETE`,
  `MERGE`, or an inline edit) is committed as soon as it succeeds; a PL/SQL
  block (`BEGIN … END;`) runs as it is. Dates in an inline edit are read in the
  session's date format. TLS (TCPS) takes a CA file or an Oracle wallet.
  *Not yet tried against a real server.*
- **rqlite.** SQL over the node's HTTP API (SQLite's dialect). The tree has one
  schema, `main`. *Read consistency* is how sure a read is to see the latest
  write: Weak (rqlite's default), None (may be stale), Linearizable or Strong.
  Reads go to the query endpoint and writes to the execute endpoint; the first
  word of a statement decides which. A user and password are sent as basic
  authentication. Most nodes run without TLS, so set Encryption to None (or use
  an SSH host).
- **Redis.** Keys have no rows, so the tree shows the server's databases, and
  inside one its keys, found with `SCAN` (never `KEYS *`) and grouped into
  folders by the part before a colon, with each key's type and time to live.
  Double-click a key to see it as a grid whatever its type: a string (key and
  value), a hash (fields and values), a list (index and value), a set
  (members), a sorted set (members and scores) or a stream (ids and fields).
  The console takes Redis commands, one per line, with `redis-cli`'s quoting
  (`SET "my key" "a value"`). Start a line with **`@3`** to run it in database 3,
  or use the form's database number for the rest. `VIEW key` is the console's own
  command that does what double-clicking a key does. Inline edits: a string's
  value (the time to live is kept), a hash field, a list item, a sorted set's
  score; a set or a stream is changed with commands. Each command runs on a
  connection of its own, so **Cancel** works on a blocking command such as
  `BLPOP`. `SUBSCRIBE`, `MONITOR`, `MULTI` and `SELECT` aren't available in the
  console (it says why). A user name is for Redis 6 ACL users.
- **MongoDB.** The tree is the server's databases, their collections and
  views, and a collection's indexes. Double-click a collection to run its
  `find`. The console takes a database command as **JSON**, the way
  `db.runCommand` does: `{"find": "users", "filter": {"age": {"$gt": 30}}}`, or
  `{"aggregate": "users", "pipeline": [...]}`, `{"count": "users"}`,
  `{"insert": ...}`. A key `"$db"` picks another database. Documents become rows,
  one column per top-level field (nested values are their JSON text), with
  ObjectIds, dates and big numbers shown as plain values. Inline edit changes one
  top-level field of the document with that `_id` (`updateOne` with `$set`); what
  you type is read as JSON when it is JSON (`36`, `true`, `["a"]`), else as text.
  *Authentication database* is where the user is defined; *Connection string*
  takes an Atlas (`mongodb+srv://…`) or replica-set string, with the form's user
  and password added to it (leave the password out of the string). Through an
  SSH host the driver talks to one server and doesn't check the TLS name (the SSH
  connection identifies the server). The sign-in (SCRAM) has not been tried
  against a real server; the commands were tried against a stand-in that speaks
  the wire protocol.
- **Elasticsearch / OpenSearch.** The tree is the cluster, its indices (with
  document count, size and health; system indices last) and an index's fields.
  Double-click an index to search it. The console takes a request the way
  Kibana's does: a first line with the method and path, then the JSON body:
  `GET /books/_search` and `{"query": {"match": {"title": "dune"}}}` on the lines
  after. Search hits become rows (`_id`, `_score` and the `_source` fields as
  columns), `_cat` calls are asked for JSON and shown as rows, anything else as one
  row of its top-level fields. A search is limited to the row limit (and says when
  more matched). *Sign in with* is basic (user and password), an API key or a
  bearer token (put it in the password field). Inline edit sends a partial
  `_update` for the document with that `_id`.

For the engines that aren't SQL, the **Safety** check works on the engine's
own commands: `FLUSHALL`, `DEL`, `CONFIG SET` and the like in Redis; `drop`,
`delete`, an update that touches every match, or an aggregation that writes
with `$out` in MongoDB; `DELETE`, `_delete_by_query`, `_close` and settings
changes in Elasticsearch. They ask first and say why, and production
connections ask for the connection's name.

**Browse.** Click a connection to connect. Expand a database (a schema, for
PostgreSQL) to see its tables and views, and a table to see its columns and
indexes. Double-click a
table to open its rows. The tree and queries use the connection's own
account, so you see what that user is allowed to see.

**Query.** Press **Ctrl+Enter** to run the editor, or the selected text if
there is a selection. The row limit (1,000 by default, up to 100,000) stops
reading once reached and says so; **Cancel** stops a running statement on
the server. Results are drawn as you scroll, so large ones stay fast. Click
a column header to sort the rows already loaded (the server's order comes
back on a third click). Very long values are cut at 64 KB and marked; they
can't be edited.

**Sessions.** Statements run one at a time on the same server session, so
`SET`, `USE` and temporary tables carry over from one run to the next. Runs
that overlap in time (two tabs at once) use separate sessions. Because of
that, a transaction can't be continued from one run to the next: a run that
starts one without ending it (`BEGIN` with no `COMMIT` or `ROLLBACK`) is
refused, so nothing is lost silently. Put `BEGIN … COMMIT` in the same run.

**Row limit and side effects.** The limit stops reading and tells the
server to stop; a `SELECT` that calls a function with side effects only runs
that function for the rows that were read.

**Edit a row.** In a table opened from the tree, double-click a cell (or
press Enter or F2 on it). You are shown the exact `UPDATE` before it runs.
Only tables with a primary key can be edited, and only one cell at a time;
the row is found by its key and the statement changes at most one row. The
new value is sent as a bound parameter and read by the column's own type, so
`12.30` into a numeric column, `false` into a boolean, or JSON text into a
`jsonb` column all work, and a value the column can't hold is the server's
own error.
**NULL** next to the editor sets the value to NULL. Binary and cut values
are read-only.

**Safety.** A statement that drops, truncates, deletes or updates without a
`WHERE`, or alters a table to drop something, asks first and says why.
Mark a connection's **environment** as production and these questions, and
every row edit, ask you to type the connection's name. The check reads past
comments and quoted text but is a safety net, not a guarantee.

**Export.** **Export** copies the result as CSV, TSV or JSON, or saves CSV
or JSON to a file. Text that a spreadsheet could read as a formula is
prefixed with `'`.

**History.** **History** lists recent statements. It is kept on this
computer only and is never synced, because queries often hold names and
identifiers.

## Containers

Open **Containers** in the activity bar (or **Ctrl+Shift+P** → "Go to
Containers"). It lists the containers and images of Docker, Podman or nerdctl
on this computer or on any saved host, and nothing is installed on the host:
it runs the runtime's own command line over your existing SSH access.

**Open a source.** Use the box at the top right: **This computer**, or any
host (type to search). The host needs saved credentials, because the
connection is made without asking. Each source you open stays as a chip at
the top, and its connection stays open so refreshing costs almost nothing;
close the chip with its **×**, or lock the vault, to hang up.

**Runtime.** What is installed is detected when the source opens, and the
first one found is used. Pick another from **Runtime** at any time, including
one marked "not found" if it lives somewhere unusual. On a host, the usual
install folders (`/usr/local/bin`, `/opt/homebrew/bin`, `/snap/bin`) are
searched as well as the default PATH.

**The list.** Containers show name, ID, image, state and its status text,
published ports, and age. A port published on both IPv4 and IPv6 shows once;
a port that is exposed but not published is greyed. Compose projects and
Podman pods appear as badges. Tick **Sizes** to ask for container sizes
(slower on a host with many large containers). The **Images** tab lists
images with size, age and the number of containers using each. **Search**
matches name, image, ID, status, port, label and pod; the buttons next to it
narrow by running or stopped.

**Refreshing.** The list refreshes every 5 seconds (2, 10 or 30 if you
prefer), only while the window is visible. **Pause** stops the timer, and
the refresh button still works. If a refresh fails, the old list stays on
screen with the reason above it, and clears itself when the host answers
again.

**Actions** are on each row: logs, a shell, inspect, start or stop, restart,
and remove. Removing always asks, and says if the container is running (it is
then stopped first; volumes are kept). On a host marked production, every
stop, restart, removal, take-down and prune asks you to type the host's name
first.

**Logs.** Opens the container's log in a window. **Following** keeps adding
new lines (click it to take a snapshot instead), **Lines** picks how many
from the end, and **Timestamps** asks the runtime to prefix each line. Search
highlights every match, **Enter** and **Shift+Enter** jump between them, and
**Only matches** hides the other lines. **Copy** copies what is shown
(or only the matches). Colour codes are removed, a progress bar that rewrites
its line reads as its final state, and only the newest 20,000 lines are kept,
which the footer says. Closing the window ends the follow, on the host too.

**Shell.** Opens a normal terminal tab and types
`docker exec -it <id> sh -c '…bash, or sh if there is none'` into it: on a
host it is that host's usual terminal, here it is a local terminal. The
container must be running.

**Inspect.** The runtime's full JSON for the container, with find and copy.

**Compose projects.** Containers started by Docker Compose are grouped under
their project, with how many are running. The buttons on a project's header
start, stop, restart or take down the whole project (Docker only), found by
its name, so the compose file doesn't need to be on this computer. Hover the
name to see where Compose was run and which files it used, on the host.
"Take down" removes the project's containers and its networks and **keeps its
volumes**, so its data survives; it always asks. Untick **Group by Compose
project** for the flat list.

**Images.** The **Images** tab lists images with size, age and how many
containers use each. **Pull image…** pulls a reference such as `nginx:1.27`
or `ghcr.io/org/app@sha256:…` and shows the runtime's progress as it comes
(Stop ends it, on the host too). The bin on a row removes that tag; if another
tag still names the same image, only this tag goes. Removing an image a
container uses is refused by the runtime, with its reason.

**Volumes and networks** (Docker) have their own tabs, with who uses each: a
container that is stopped still uses its volume and network. The bin is off
for something in use, and for the built-in `bridge`, `host` and `none`
networks. Volume sizes appear with **Sizes**.

**Removing what is unused.** **Remove unused…** on Images, Volumes and
Networks first *shows* what would go, and removes nothing until you press the
button, and then only what is ticked. It never runs the runtime's own `prune`:
the list is worked out from what is on the host, and exactly those items are
removed one at a time. Each is checked again just before, and one that has
become used since you looked is left alone and reported. Images start with
only the untagged ones; tick "Also tagged images that no container uses" for
the rest. **Volumes start with nothing ticked**, because the data in a removed
volume is gone for good. Items that belong to a Compose project are marked,
with a note that bringing the project up again would have to create them anew.

**When it doesn't work.** The runtime's own error is shown in full, with a
hint for the usual causes: a user who isn't in the `docker` group, a daemon
that isn't running, a Podman socket that isn't available. Docker is used as
the signed-in user; there is no `sudo`.

### Kubernetes

The **Kubernetes** tab next to Containers shows the pods of a cluster through the `kubectl`
that is already on this computer or on one of your saved hosts, with that machine's own
kubeconfig. Nothing is installed anywhere and SSHVault never reads the kubeconfig itself.

- Open **This computer** or a host (it needs saved credentials), pick a **context** and a
  **namespace** (or all of them). Pods refresh every ten seconds and can be searched; the
  status is coloured and shows what kubectl shows (CrashLoopBackOff, Terminating,
  ImagePullBackOff ...), with ready containers, restarts, age, node and owner.
- **Logs** follows a pod's log (pick the container; the previous run after a crash; filter and
  pause as in Operations). **Describe** shows `kubectl describe`. **Shell** opens a terminal
  tab running `kubectl exec -it` with bash or sh. **Delete** asks first; on a production host
  you type the pod's name.
- **Port-forward** (this computer only) forwards a pod or service port to `127.0.0.1` until you
  stop it. On a host the port would open on that host, so the button isn't offered there.
- Names are checked against what Kubernetes allows and passed to kubectl as separate
  arguments, never as part of a command line. Every call has a 15-second limit so a cluster
  that isn't answering shows an error with a hint instead of hanging. The connection stays open
  when you go to another page and closes when the vault locks.

## Locking

The lock icon at the bottom of the sidebar, or **Ctrl+Shift+L**, closes every
terminal, SFTP session and forwarding rule, removes temporary copies of
remote files, and wipes the key from memory. Under **Settings → Auto-lock**
you can lock automatically after 5, 15, 30 or 60 minutes without keyboard
or mouse activity.

Unlocking again (or restarting the app) reopens the tabs that were open last
time, each reconnecting on its own, unless you turn this off under
**Settings → Connections**. Production hosts are left closed by default, to
reconnect by hand.

## Security review

**Vault → Security review** lists certificates nearing or past expiry, weak
(RSA under 3072 bits) or deprecated (DSA) keys, keys older than five years,
hosts still using a saved password when a key is available, and passwords
that haven't changed in a while. Everything is computed locally from what's
already in the vault; nothing is sent anywhere, and nothing is fixed
automatically. The connection log on the **Vault** screen (local to this
computer, never synced) shows when and how long each session lasted.

## Known hosts

The first connection to a server shows its fingerprint and asks whether to
trust it. Trusted keys are stored in the vault, so your other computers
don't ask again. If a key changes, the connection stops and you see the old
and new fingerprints; replacing the key needs you to type `replace`. The
**Known hosts** screen lists every trusted key with its history, and can
import your existing `~/.ssh/known_hosts`.

## Troubleshooting

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
