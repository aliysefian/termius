# Design: plugins (SSHV-019)

Status: proposal for decision. No code. Nothing here is built.

## What it is for

Letting people add things SSHVault does not ship: an inventory source (a cloud, a CMDB), a notification target for alerts, a
snippet or runbook pack, a database connector. The app already has two seeds of this: snippet packs (`snippetpacks.ts`) and
inventory import from other tools' CLIs (`inventory.rs`). Both are *data*, not code.

## The risk that shapes everything

SSHVault holds an unlocked vault of SSH keys and passwords, and live SSH sessions. A plugin that runs inside that process can read
all of it. So the question is never "how do plugins work" but "what is a plugin allowed to touch, and what stops it".

## Three kinds, three different risks

| Kind | What it is | Can it run code? | Risk | Proposal |
|---|---|---|---|---|
| **Declarative** | A file of data: snippets, runbooks, an inventory mapping, an alert-notification webhook definition | No | Low: the app interprets it | **Build first.** Validate against a schema, show what it contains before install, no network except what it declares |
| **Sandboxed code** | A WebAssembly module with a small host API | Yes, in a sandbox with no ambient authority | Medium: bounded by the host API | **Consider second**, only if declarative is not enough |
| **Native code** | An executable or library | Yes, as the user | High: can read the vault's memory and files | **Do not build.** If someone needs it, run it as a separate program that talks to the CLI socket the app already has (`control.rs`, which already requires approval in the window) |

## Host API (for sandboxed code, if it is ever built)

Only resource ids and the results of operations, never credentials: `list_hosts()` (id, label, address, tags), `run_on_host(id,
command)` through the same approval and production-confirmation path as a person's command, `notify(text)`, `http_get(url)` limited
to hosts the manifest declares. No vault access, no file system, no raw sockets, no ability to read a secret or to approve its own
request. Every call is logged to the local audit list with the plugin's name.

## Manifest and permissions

A plugin declares, up front, what it needs: kinds of call, hosts it may contact, whether it may run commands. The install screen
shows these in plain words and the person accepts or not. A plugin that asks for more later must be reinstalled; there is no
silent upgrade of permissions. Versioned API (`api = 1`); a plugin for a newer API is refused, not guessed at.

## Installing and updating

Local file only to begin with (no marketplace, no auto-update): the person picks a file, sees its manifest and a hash, and
confirms. Updates are the same flow. Uninstalling removes the plugin and everything it stored. No plugin runs while the vault is
locked.

## Testing it would need

Permission-denied tests for every host call; a plugin that loops or allocates is stopped by a time and memory limit; a plugin
that returns hostile text cannot inject into the window (output is text, never HTML); malformed manifests are refused; an
uninstall leaves nothing behind.

## Decisions needed from the owner

1. Is declarative-only (packs, mappings, notification definitions) enough for the first release? (Recommended: yes.)
2. If code is wanted at all: WebAssembly with the host API above, or "use the CLI socket from your own program"? (Recommended:
   the latter until a real need appears.)
3. Is a catalogue or marketplace ever wanted? It adds a trust and signing problem that this design does not solve.
