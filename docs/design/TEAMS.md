# Design: teams and shared use (SSHV-020)

Status: proposal for decision. No code. The vault format would change, so this needs the same care as the v1 to v2 migration.

## What exists, and why teams do not fit it

Today a vault has one key. Anyone with the master password (or the recovery key) can read everything in it, and every device
that opens it is equal. That is right for one person with several computers. It is wrong for a team, where people should see
different hosts and credentials, someone leaving must lose access, and there should be a record of who did what.

## Do not do the cheap thing

Sharing the master password is what people do now. A "teams" feature that is only a shared vault with names on it gives the
feeling of access control without any: anyone with a copy of the folder and the password has everything, and revoking a person
means changing the password for everyone and trusting that nobody kept a copy of the old data.

## The shape that would be honest

1. **Per-person key slots.** The vault already wraps its one data key under a password slot and a recovery slot
   (`vault/format.rs`). A team vault adds one slot per member, wrapped to that member's own key (their password, or a device key).
   Adding a member is done by an existing admin, who unwraps the data key and wraps it for the newcomer.
2. **Removing a member is not undoing their access.** Whoever held the data key could have copied it, and could read what they
   had synced. A real revoke means rotating the data key and re-encrypting (the work that was dropped from SSHV-001 by the
   owner's decision). **Without rotation, "remove member" must not be offered**, or it would be a lie. This is the main reason
   teams depend on a decision that has been made the other way.
3. **Roles and scoped access** (who may see which hosts or credentials) cannot be enforced by a shared data key: a member
   with the key can read every record. Per-collection or per-group keys would be needed, which multiplies the key management.
   Honest options: separate vaults per access level, or a server that holds the secrets and hands out short-lived access (see the
   controller design).
4. **Audit** needs a log that members cannot rewrite. A synced folder cannot give that (anyone with write access can edit it);
   it needs a server, or an append-only log with signatures from each member's key.

## What can be built without any of that

A single-owner vault used from several computers (already done), shared read-only exports of non-secret data (hosts, snippets,
runbooks) as files, and approval prompts for risky actions on production hosts (already there). These help a team without
claiming access control.

## Decisions needed from the owner

1. Is multi-person access a goal for this product, or is SSHVault a personal tool that exports to teams? (Recommended: a personal
   tool; a team product is a different product with a server.)
2. If it is a goal: is key rotation (SSHV-001's dropped item) back on the table, since removal needs it?
3. Is a server acceptable at all (see CONTROLLER.md)? Roles and audit are not honestly possible without one.
