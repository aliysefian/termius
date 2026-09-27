# Security policy

SSHVault stores SSH keys and passwords, so security reports are taken
seriously and handled before anything else.

## Reporting a vulnerability

**Please don't open a public issue for a security problem.**

Report it privately through GitHub:
**[Report a vulnerability](https://github.com/aliysefian/termius/security/advisories/new)**
(the **Security** tab → **Advisories** → **Report a vulnerability**).

Please include:

- what an attacker can do, and what they need first (for example a copy of
  the synced folder, local access while the vault is locked, or a malicious
  server);
- steps to reproduce, and the SSHVault version and operating system;
- any fix or mitigation you have in mind.

You should get a reply within a few days. Once a fix is released, the
advisory is published with credit to you, unless you'd rather stay
anonymous.

## Scope

In scope, for example:

- anything that reveals vault contents without the master password or the
  recovery key, including from the synced folder, backups or files left on
  disk;
- ways to make SSHVault trust a server key, run a command, or use a key
  without the user's approval;
- secrets reaching logs, the clipboard beyond its timeout, or other local
  users.

Out of scope: attacks that need a computer already compromised while the
vault is unlocked (see the threat model in
[docs/vault-architecture.md](docs/vault-architecture.md)), and weaknesses
in a server you connect to.

## Supported versions

Only the latest release gets security fixes. The app can update itself, and
new releases are listed on the
[releases page](https://github.com/aliysefian/termius/releases).
