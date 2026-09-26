// Single source of truth for decrypted records on the frontend.
// Populated from IPC on unlock and patched live by `vault:changed` events.
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "$lib/api";
import {
  errorMessage,
  type ForwardRule,
  type ForwardStatus,
  type Host,
  type Identity,
  type RecordChange,
  type Snippet,
  type Uuid,
  type VaultRecord,
  type VaultStatus,
} from "$lib/types";

function upsert<T>(list: VaultRecord<T>[], incoming: VaultRecord<unknown> | null, id: Uuid) {
  const idx = list.findIndex((r) => r.id === id);
  if (!incoming || incoming.deleted) {
    if (idx >= 0) list.splice(idx, 1);
    return;
  }
  const rec = incoming as VaultRecord<T>;
  // Last-writer-wins: never let an older file overwrite a newer in-memory copy.
  if (idx >= 0) {
    if (list[idx].updated_at <= rec.updated_at) list[idx] = rec;
  } else {
    list.push(rec);
  }
  list.sort((a, b) => b.updated_at - a.updated_at);
}

class VaultStore {
  status = $state<VaultStatus>({ state: "not_configured" });
  hosts = $state<VaultRecord<Host>[]>([]);
  identities = $state<VaultRecord<Identity>[]>([]);
  snippets = $state<VaultRecord<Snippet>[]>([]);
  forwards = $state<VaultRecord<ForwardRule>[]>([]);
  /** Live status per forwarding rule, pushed by the Rust side. */
  forwardStatus = $state<Record<Uuid, ForwardStatus>>({});
  loading = $state(false);
  error = $state<string | null>(null);

  unlocked = $derived(this.status.state === "unlocked");
  hostById = $derived(new Map(this.hosts.map((h) => [h.id, h])));
  identityById = $derived(new Map(this.identities.map((i) => [i.id, i])));

  #unlisten: UnlistenFn | null = null;
  #unlistenForward: UnlistenFn | null = null;

  async init() {
    await this.refreshStatus();
    if (!this.#unlisten) {
      this.#unlisten = await api.vault.onChanged((c) => this.applyChange(c));
    }
    if (!this.#unlistenForward) {
      this.#unlistenForward = await api.forwards.onStatus(({ rule_id, status }) => {
        this.forwardStatus[rule_id] = status;
      });
    }
  }

  async refreshStatus() {
    try {
      this.status = await api.vault.status();
      if (this.unlocked) await this.reloadAll();
      else this.clearRecords();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async reloadAll() {
    this.loading = true;
    try {
      [this.hosts, this.identities, this.snippets, this.forwards, this.forwardStatus] = await Promise.all([
        api.hosts.list(),
        api.identities.list(),
        api.snippets.list(),
        api.forwards.list(),
        api.forwards.statuses(),
      ]);
      this.error = null;
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  clearRecords() {
    this.hosts = [];
    this.identities = [];
    this.snippets = [];
    this.forwards = [];
    this.forwardStatus = {};
  }

  /** Called for every record the Rust watcher sees change on disk. */
  applyChange(c: RecordChange) {
    switch (c.collection) {
      case "hosts":
        upsert(this.hosts, c.record, c.id);
        break;
      case "identities":
        upsert(this.identities, c.record, c.id);
        break;
      case "snippets":
        upsert(this.snippets, c.record, c.id);
        break;
      case "forwards":
        upsert(this.forwards, c.record, c.id);
        break;
    }
  }

  // -- lifecycle --------------------------------------------------------

  async setPath(path: string) {
    this.status = await api.vault.setPath(path);
    this.clearRecords();
  }

  async create(password: string) {
    this.status = await api.vault.create(password);
    await this.reloadAll();
  }

  async unlock(password: string) {
    this.status = await api.vault.unlock(password);
    await this.reloadAll();
  }

  async lock() {
    this.status = await api.vault.lock();
    this.clearRecords();
  }

  // -- writes (optimistically patch local state with the returned record) --

  async saveHost(id: Uuid | null, host: Host) {
    const rec = await api.hosts.save(id, host);
    upsert(this.hosts, rec, rec.id);
    return rec;
  }
  async deleteHost(id: Uuid) {
    await api.hosts.delete(id);
    upsert(this.hosts, null, id);
    // Backend detached this host as a jump; mirror that locally.
    for (const h of this.hosts) if (h.data?.jump_host_id === id) h.data.jump_host_id = undefined;
  }

  async saveIdentity(id: Uuid | null, identity: Identity) {
    const rec = await api.identities.save(id, identity);
    upsert(this.identities, rec, rec.id);
    return rec;
  }
  async deleteIdentity(id: Uuid) {
    await api.identities.delete(id);
    upsert(this.identities, null, id);
    // Backend detached this identity from hosts; mirror that locally.
    for (const h of this.hosts) if (h.data?.identity_id === id) h.data.identity_id = undefined;
  }

  async saveSnippet(id: Uuid | null, snippet: Snippet) {
    const rec = await api.snippets.save(id, snippet);
    upsert(this.snippets, rec, rec.id);
    return rec;
  }
  async deleteSnippet(id: Uuid) {
    await api.snippets.delete(id);
    upsert(this.snippets, null, id);
  }

  async saveForward(id: Uuid | null, rule: ForwardRule) {
    const rec = await api.forwards.save(id, rule);
    upsert(this.forwards, rec, rec.id);
    return rec;
  }
  async deleteForward(id: Uuid) {
    await api.forwards.delete(id);
    upsert(this.forwards, null, id);
    delete this.forwardStatus[id];
  }
  async startForward(id: Uuid) {
    this.forwardStatus[id] = { state: "starting" };
    try {
      await api.forwards.start(id);
    } catch (e) {
      this.forwardStatus[id] = { state: "error", message: errorMessage(e) };
    }
  }
  async stopForward(id: Uuid) {
    await api.forwards.stop(id);
  }
}

export const vaultStore = new VaultStore();
