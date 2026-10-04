<!-- The full guide to SSHVault's features. The overview is in the README. -->
# Using SSHVault

## Hosts and identities

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
- **Local terminals.** **Ctrl+Shift+`**, or the terminal icon next to the tabs,
  opens your own shell in a tab: your login shell on Linux and macOS, the
  default shell on Windows. Local tabs work with splits, recording, snippets
  and synchronized typing like SSH tabs.

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
- Right-click inside a terminal for copy, paste, select all, find and clear.
- When a program such as tmux, vim or Claude Code takes the mouse, hold
  **Shift** while dragging to select text, or click the **Program has the
  mouse** pill to make plain drags select until you switch back.
- Programs that copy through the terminal (tmux's mouse selection with
  `set-clipboard on`, Claude Code, Neovim's OSC 52 provider) put the text on
  your clipboard. Turn this off under **Settings → Terminal** if you would
  rather remote programs could not write to it.
- If the connection drops, a **Reconnect** button appears at the bottom of the
  pane.
- Change the colour theme, font, cursor and scrollback under **Settings**.
  These settings are per computer and are not synced.

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

Plain Ctrl shortcuts such as Ctrl+W, Ctrl+T and Ctrl+K go to the remote
shell, where editors and readline use them.

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

## Locking

The lock icon at the bottom of the sidebar, or **Ctrl+Shift+L**, closes every
terminal, SFTP session and forwarding rule, removes temporary copies of
remote files, and wipes the key from memory. Under **Settings → Auto-lock**
you can lock automatically after 5, 15, 30 or 60 minutes without keyboard
or mouse activity.

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
