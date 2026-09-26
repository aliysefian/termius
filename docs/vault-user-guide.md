# Your vault: user guide

SSHVault keeps your hosts, credentials, SSH keys and trusted server keys in a **vault**: an ordinary folder of encrypted files. Put it in a folder your sync service already syncs, and every computer you use sees the same hosts. The sync service only ever stores encrypted files. It can't read your data, and it never sees your master password.

## Getting started

When SSHVault opens without a vault, it offers three choices.

### Create New Vault

1. **Vault name**: the folder name, for example `MySSHVault`.
2. **Location**: where that folder goes. Pick a folder that syncs, such as `~/Dropbox`, `OneDrive` or a Syncthing folder.
3. **Master password**: at least 8 characters; longer is better. The strength meter helps. **It is never stored anywhere**, not even in the vault.
4. **Create a recovery key** (recommended): a one-time code that can reset a forgotten master password. It's shown **once**. Print it or write it down and keep it offline. Don't keep it in the synced folder. Without it, a forgotten password means the vault can't be opened, by you or anyone else.
5. **Remember on this device** (optional): unlock without typing the password on this computer. The vault key goes into the system's credential store (Windows Credential Manager, macOS Keychain, or the Secret Service on Linux), never the password. Only enable it on a computer you alone use.

### Open Existing Vault

On a second computer, wait until your sync service has downloaded the folder. Then choose **Open Existing Vault**, pick the folder, and enter the master password.

- If a remembered device key no longer matches (for example, the folder now holds a different vault), you're simply asked for the password.
- **Forgot password?** Enter your recovery key and choose a new master password. Your data isn't touched; the new password works on every device.

### Import SSH Config

This creates a vault and then opens the importer for `~/.ssh/config` (see [Importing](#importing)).

## Everyday use

The sidebar has these screens:

| Screen | What it's for |
|---|---|
| Hosts | Your servers, in nested groups. Search, filter by environment or tag, sort, drag hosts into groups. Double-click to connect. |
| Favorites | Hosts you starred. |
| Groups and proxies | Defaults for each group (credential, jump host, proxy, environment), and your proxies. |
| Keys | The Key Manager. |
| Credentials | User names with a password, a key, a key file or ssh-agent. Hosts and groups use them. |
| Tunnels | Port forwarding rules: local, remote and SOCKS (dynamic). |
| Snippets | Saved commands with folders, tags and `{{variables}}`. Snippets only run when you click Run. |
| SFTP | Files on your servers. |
| Known hosts | Server keys you've trusted. |
| Vault | Details, password, recovery, backups, integrity, sync conflicts, devices, moving the vault. |
| Settings | Appearance, auto-lock, and safety settings shared by every device. |

### Hosts, groups and environments

- A host's **group** is a path such as `Production/Databases`. Set defaults on a group (under *Groups and proxies*) and every host inside it, including sub-groups, uses them unless the host sets its own. For example, one credential and one bastion for all of Production.
- **Environments:** Development, Staging, Production, or any custom name. Production hosts get a red banner, and SSHVault asks before multi-line pastes and commands that look destructive.
- Hosts can also hold **tags**, **notes**, **custom fields** (`owner=platform-team`), a **keep-alive interval**, a **proxy**, agent and X11 forwarding, and a command to run after connecting.

### Keys

- **Generate** Ed25519 (recommended), ECDSA or RSA keys, optionally with a passphrase. You choose whether the passphrase is saved in the vault.
- **Import** an existing private key: paste it or pick the file. OpenSSH, PEM, PKCS#8 and PuTTY formats are accepted, and the key and passphrase are checked before saving. Add **public keys** of colleagues to keep them at hand.
- Click a key to:
  - copy its public key (for `~/.ssh/authorized_keys`)
  - see which credentials and hosts use it
  - rename it
  - add an OpenSSH **certificate** (used automatically when connecting)
  - change its passphrase
  - delete it
- **Export private key** is deliberately awkward. It asks for your master password every time, won't overwrite an existing file, and writes a file only you can read. Delete the file when you're done with it.
- To use a key, create a credential with authentication **Key Manager** (or pick *Key Manager* in a host's own credentials). Replacing or re-encrypting the key then applies everywhere it's used.

### Trusting servers

The first time you connect to a server, SSHVault shows its **fingerprint** and asks whether to trust it. Compare it with the fingerprint your administrator or hosting provider gives you. Trusted keys are stored in the vault, so your other computers won't ask again.

If a server's key **changes**, the connection is stopped and you see the old and new fingerprints. That's normal after a server is reinstalled, but it's also what an attack looks like. Only replace the key if you know why it changed; you'll be asked to type `replace`. Old keys are kept in the host's history on the *Known hosts* screen, where you can also import your existing `~/.ssh/known_hosts`.

### Proxies and jump hosts

- **Jump hosts** (bastions) can be set per host or per group, and can be chained.
- A **proxy** (SOCKS5, HTTP CONNECT, or an OpenSSH `ProxyCommand`) is used to reach a host, or the first jump of its chain.
- A **ProxyCommand runs a program on your computer**. It only runs after you tick *Allow this command to run on my computers*, and imported ones always start switched off. That approval is stored in the vault, so it applies on every device that uses it.

### Terminal safety

- **Paste protection:** pasting several lines, or anything that would run immediately on a production host, shows you the text first. Change the threshold under *Settings → Safety*.
- **Destructive commands** on production hosts (for example `rm -rf /`, `DROP TABLE`, `kubectl delete`, `terraform destroy`) ask for confirmation before Enter is sent. The patterns are editable. This is a safety net, not a guarantee: SSHVault only sees what you type, not what history expansion, completion, aliases or scripts turn it into.
- **Clipboard:** copied passwords, keys and recovery keys are cleared from the clipboard after 30 seconds by default (*Settings → Safety*).
- **Auto-lock:** lock after 5, 15, 30 or 60 minutes without activity (*Settings → Auto-lock*). Locking closes every session and removes temporary copies of files you were editing.

## Importing

Open **Import from SSH config** from the Hosts screen (or the Import option at start-up). SSHVault reads these options:

- `Host`, `HostName`, `User`, `Port`
- `IdentityFile`
- `ProxyJump` (single hop)
- `ProxyCommand`
- `ForwardAgent`, `ForwardX11`
- `LocalForward`, `RemoteForward`, `DynamicForward`
- `ServerAliveInterval`

`Host *` blocks act as defaults, as they do in `ssh`. Wildcard entries, `Match` blocks and `Include` are listed as warnings.

For key files you choose between two options:

- **Reference by path** (default). The key stays in `~/.ssh` on this computer and nothing secret enters the vault. Other computers need the same file at the same path.
- **Copy into the vault.** The key is stored encrypted in the Key Manager and syncs everywhere. Keys protected by a passphrase stay protected.

Forwards become tunnels that don't start automatically. ProxyCommands become proxies that are switched off until you approve them. Ansible INI inventories can be imported the same way.

## Several computers at once

You can have the vault open on several computers at the same time; the Vault screen shows where else it's open. Changes appear on the other computers within a few seconds of your sync service copying them.

- **Different changes to the same item** (say, notes on one computer and tags on another) are merged automatically.
- **The same field changed on two computers** before they synced: the second save is refused with an explanation, and you see the current version to redo your change. Credentials are never silently overwritten.
- **Offline edits** sometimes make a sync service keep two copies of a file. SSHVault merges them when it can. Otherwise a dot appears on the Vault icon, and *Vault → Sync conflicts* shows both versions to pick from.

## Backups and restore

- An encrypted backup is taken automatically once a day when you unlock, and you can **Back up now** at any time. Backups live in the vault's `backups/` folder, so they sync too.
- *Settings → Safety → Keep backups* sets how many are kept (30 by default).
- **Restore** checks the backup completely first. It then takes a backup of the current state (so a restore can itself be undone) and writes the restored items as normal changes, so your other computers pick them up.
- **Verify integrity** checks every record, reference and backup, and reports what it finds. It never changes anything.

## Changing things later

- **Change master password:** instant. Other computers need the new password the next time they unlock; remembered devices keep working.
- **Recovery key:** create one, replace it (the old one stops working) or remove it, from the Vault screen. Each needs your master password.
- **Move vault:** copies the vault to an empty folder, verifies the copy, and switches to it. The old folder is left in place: once your other computers have opened the new folder, delete the old one.
- **Remember / forget on this device:** on the Vault screen.

## Upgrading from an older SSHVault

The first time a newer SSHVault unlocks a vault made by an older version, it upgrades the vault:

- The original encrypted files are copied to `backups/v1-…` first, and the upgraded copy is checked before anything is replaced.
- If the computer loses power mid-upgrade, the next start finishes or rolls back cleanly.
- Update SSHVault on all your computers: older versions refuse to open the new format rather than risk damaging it.

## What is and isn't protected

- **Protected:** everything in the vault folder, against anyone who gets a copy of it, including the sync provider. Without your master password or recovery key it reveals no hosts, names, addresses, credentials or keys.
- **Not protected:** a computer that is already compromised while the vault is unlocked. Malware running as you can see what you see. Lock the vault when you step away, and keep your computers updated.
- **Your master password is the key to everything.** Use a long, unique one, and keep the recovery key somewhere safe and offline.
