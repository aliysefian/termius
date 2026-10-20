// `authorized_keys` on the host: read it, add a key from the vault, revoke one, rotate one.
import * as api from "$lib/api";
import { ask } from "$lib/dialogs.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type Identity, type SshKey, type VaultRecord } from "$lib/types";
import { READ_SCRIPT, checkPublicKeyLine, fingerprintOf, parseAuthorizedKeys, withKey, withoutKey, writeScript, type AuthorizedKey } from "./authkeys";
import type { SecuritySession } from "./securitySession.svelte";

export class AuthorizedKeysManager {
  keys = $state<AuthorizedKey[]>([]);
  who = $state<"me" | "root">("me");
  loading = $state(false);
  loaded = $state(false);
  busy = $state<string | null>(null);
  error = $state<string | null>(null);
  /** The file as last read, to prove it hasn't changed before it is written. */
  #text = "";

  constructor(private readonly session: SecuritySession) {}

  /** The vault key this host's own login uses: the one that is letting this session in. */
  get activeKey(): VaultRecord<SshKey> | undefined {
    const host = this.session.host;
    const id = vaultStore.effectiveIdentity(host);
    const auth = id ? vaultStore.identityById.get(id)?.data?.auth : undefined;
    return auth?.type === "key" ? vaultStore.keyById.get(auth.key_id) : undefined;
  }

  get activeFingerprint(): string | null {
    return this.activeKey?.data?.fingerprint ?? null;
  }

  /** Keys in the vault that have a public half to push. */
  get vaultKeys() {
    return vaultStore.keys.filter((k) => k.data && !k.deleted && k.data.public_key);
  }

  async refresh() {
    this.loading = true;
    this.error = null;
    try {
      const r = await this.session.run(READ_SCRIPT(this.who), { mode: this.who === "root" ? "always" : "auto" });
      this.#text = r.stdout;
      this.keys = await parseAuthorizedKeys(r.stdout);
      this.loaded = true;
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  async #write(text: string): Promise<void> {
    const r = await this.session.run(writeScript(this.who, text, this.#text), { mode: this.who === "root" ? "always" : "auto", timeoutSecs: 30 });
    if (r.code !== 0) throw new Error((r.stderr || r.stdout).trim().split("\n")[0] || `exit ${r.code}`);
  }

  /** Add a public key line. Already there: nothing changes. */
  async add(line: string, label = "key"): Promise<boolean> {
    const bad = checkPublicKeyLine(line);
    if (bad) {
      ui.notify("error", bad);
      return false;
    }
    this.busy = `Add ${label}`;
    try {
      await this.#write(withKey(this.#text, line));
      ui.notify("info", `${label} added to authorized_keys (directory 700, file 600).`);
      return true;
    } catch (e) {
      ui.notify("error", `Add ${label}: ${errorMessage(e)}`);
      return false;
    } finally {
      this.busy = null;
      await this.refresh();
    }
  }

  addFromVault(key: VaultRecord<SshKey>) {
    if (!key.data) return Promise.resolve(false);
    // The vault stores `type base64 [comment]`; the host gets the name as the comment if there isn't one.
    const parts = key.data.public_key.trim().split(/\s+/);
    const line = parts.length >= 3 ? parts.join(" ") : `${parts[0]} ${parts[1]} ${key.data.name}`;
    return this.add(line, key.data.name);
  }

  /** Whether this key is the one letting the cockpit in. */
  isActive(k: AuthorizedKey): boolean {
    return !!this.activeFingerprint && k.fingerprint === this.activeFingerprint;
  }

  async revoke(k: AuthorizedKey): Promise<boolean> {
    const active = this.isActive(k);
    const host = this.session.host;
    const text = active
      ? `This is the key your saved login for ${host?.label ?? "this host"} uses. If you remove it, this app and anything else using it can no longer connect.\n\n${k.fingerprint}`
      : `Remove this key from authorized_keys?\n\n${k.comment || k.type}\n${k.fingerprint}`;
    const ok = await ask(text, { title: active ? "Remove the key you are using" : "Revoke key", confirm: active ? "Remove it anyway" : "Revoke", danger: true, requireText: active ? (host?.label ?? "remove") : undefined });
    if (!ok) return false;
    this.busy = "Revoke";
    try {
      await this.#write(withoutKey(this.#text, k));
      ui.notify("info", "Key removed.");
      return true;
    } catch (e) {
      ui.notify("error", `Revoke: ${errorMessage(e)}`);
      return false;
    } finally {
      this.busy = null;
      await this.refresh();
    }
  }

  /**
   * New ed25519 key in the vault, added to the host, proven by a login with it, and only then is the old one removed.
   * Any step failing leaves the old key where it was, and takes the new one back off the host.
   */
  async rotate(old: AuthorizedKey): Promise<boolean> {
    const hostId = this.session.hostId;
    const host = this.session.host;
    if (!hostId || !host) return false;
    const active = this.isActive(old);
    const ownIdentity = host.identity_id ? vaultStore.identityById.get(host.identity_id) : undefined;
    if (!(await ask(`Make a new ed25519 key, add it to ${host.label}, check that it logs in, then remove the old one (${old.comment || old.fingerprint})?`, { title: "Rotate key", confirm: "Rotate" }))) return false;

    this.busy = "Rotate";
    let created: VaultRecord<SshKey> | null = null;
    let newLine = "";
    try {
      const stamp = new Date().toISOString().slice(0, 10);
      const label = old.comment ? `${old.comment} (rotated ${stamp})` : `${host.label} (rotated ${stamp})`;
      created = await api.keys.create(label, "ed25519", label, null, false);
      const pub = created.data?.public_key.trim() ?? "";
      const parts = pub.split(/\s+/);
      newLine = parts.length >= 3 ? pub : `${parts[0]} ${parts[1]} ${label}`;

      await this.#write(withKey(this.#text, newLine));
      await this.refresh();
      try {
        await api.security.verifyKey(hostId, created.id);
      } catch (e) {
        // Back out: the new key comes off the host again and out of the vault; the old one was never touched.
        const added = this.keys.find((k) => k.blob === parts[1]);
        if (added) await this.#write(withoutKey(this.#text, added)).catch(() => {});
        await api.keys.delete(created.id, created.rev).catch(() => {});
        throw new Error(`the new key did not log in (${errorMessage(e)}). Nothing was removed.`);
      }

      // The host's own login uses the old key: point this host at the new one, or the app locks itself out.
      if (active && ownIdentity?.data) {
        const next: Identity = { ...ownIdentity.data, label: `${ownIdentity.data.label} (rotated ${stamp})`, auth: { type: "key", key_id: created.id } };
        const saved = await api.identities.save(null, null, next);
        const hostRec = vaultStore.hostById.get(hostId);
        if (hostRec?.data) await api.hosts.save(hostId, hostRec.rev, { ...hostRec.data, identity_id: saved.id });
      } else if (active) {
        ui.notify("warning", "The old key is the one this host's login uses, through a group default, so it was left in place. Point the group at the new key, then remove the old one.");
        return true;
      }

      const current = this.keys.find((k) => k.fingerprint === old.fingerprint);
      if (current) await this.#write(withoutKey(this.#text, current));
      ui.notify("info", `Rotated. The new key logs in to ${host.label}, and the old one is removed.`);
      return true;
    } catch (e) {
      ui.notify("error", `Rotate: ${errorMessage(e)}`);
      return false;
    } finally {
      this.busy = null;
      await this.refresh();
    }
  }
}

export { fingerprintOf };
