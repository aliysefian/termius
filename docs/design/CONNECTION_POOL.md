# Connection pooling (SSHV-003, step 4): design, not built

Status: **needs an owner decision.** Written 2026-10-09. No code changes.

## Today
Every feature opens its own SSH connection: a terminal, the SFTP pane, the monitor (which keeps one for its whole window),
each runbook host, each command-runner host, containers and Kubernetes. Opening costs a TCP connect, key exchange, the
host-key check and authentication (plus a ProxyJump chain or ProxyCommand when the host has one). Servers also count
connections: `MaxStartups` and `MaxSessions` limits can refuse a burst.

## Proposal
One pool keyed by `(host id, credential fingerprint, proxy chain)`. A feature asks for a lease, opens a channel on the shared
client, and gives the lease back. An idle connection closes after 60 s with no leases. Keepalive stays as it is.

## Why this is not simply done
- **Fate sharing.** If one connection dies, every channel on it dies: a long runbook and a terminal would drop together.
- **Credentials.** A pooled connection outlives the unlock that produced its key. With "Lock forgets the key"
  (`keep_key_on_lock` off) the pool must close on lock; a leaked lease would keep a host reachable after the lock.
- **Approvals and forwarding.** ProxyCommand approval is per computer and per command; a lease must never be handed to a feature
  the user has not approved for that host. Agent forwarding or port forwards on a shared client would leak to other features.
- **Server limits.** `MaxSessions` (default 10) now applies to all features at once; a pool needs a cap and a fallback to a
  second connection.
- **Cancellation.** Cancelling a runbook must close its channels without closing the connection others use.
- **Host-key changes.** A changed key must evict every pooled connection for that host.

## What would be pooled first
Read-only, short, repeated work with the least blast radius: the monitor's samples, container and Kubernetes listings, and
host facts. Terminals, SFTP and anything that forwards ports stay on their own connection.

## Tests needed
Lease and release, idle expiry, eviction on lock and on host-key change, cap and fallback, a dead connection replaced
transparently, two features never sharing a connection across different credentials.

## Decisions for the owner
1. Is the saving worth the new failure mode (fate sharing)? Opening a connection is typically 100 to 500 ms; the monitor already
   reuses its own.
2. If yes: start with the read-only users above, behind a setting that defaults off.
3. If no: mark step 4 of SSHV-003 "won't do" and close the item.
