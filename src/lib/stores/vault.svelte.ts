// Single source of truth for decrypted records on the frontend.
// Populated from IPC on unlock and patched live by `vault:changed` events.
import type { UnlistenFn } from "@tauri-apps/api/event";
import * as api from "$lib/api";
import { ui } from "$lib/stores/ui.svelte";
import {
  errorMessage,
  isApiError,
  type Collection,
  type CreateResult,
  type HostGroup,
  type Proxy,
  type SshKey,
  type UnlockReport,
  type VaultSettings,
  type Workspace,
  type ForwardRule,
  type Health,
  type HostCredentials,
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
  // Never let an older revision overwrite a newer in-memory copy.
  if (idx >= 0) {
    if ((list[idx].rev ?? 0) <= (rec.rev ?? 0)) list[idx] = rec;
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
  keys = $state<VaultRecord<SshKey>[]>([]);
  groups = $state<VaultRecord<HostGroup>[]>([]);
  proxies = $state<VaultRecord<Proxy>[]>([]);
  workspaces = $state<VaultRecord<Workspace>[]>([]);
  /** Settings shared by every device using this vault. */
  settings = $state<{ rev: number; settings: VaultSettings } | null>(null);
  /** Sync conflicts waiting for a decision (see the Vault screen). */
  openConflicts = $state(0);
  /** What the last unlock did, for a one-time notice. */
  lastUnlock = $state<UnlockReport | null>(null);
  /** Shown once after creating a vault or a new recovery key. */
  pendingRecoveryKey = $state<string | null>(null);
  /** Live status per forwarding rule, pushed by the Rust side. */
  forwardStatus = $state<Record<Uuid, ForwardStatus>>({});
  loading = $state(false);
  error = $state<string | null>(null);

  unlocked = $derived(this.status.state === "unlocked");
  hostById = $derived(new Map(this.hosts.map((h) => [h.id, h])));
  identityById = $derived(new Map(this.identities.map((i) => [i.id, i])));
  keyById = $derived(new Map(this.keys.map((k) => [k.id, k])));
  proxyById = $derived(new Map(this.proxies.map((p) => [p.id, p])));
  groupByPath = $derived(new Map(this.groups.filter((g) => g.data).map((g) => [g.data!.path, g])));

  /** The nearest group default for `field`, like the backend resolves it. */
  groupDefault<K extends keyof HostGroup>(host: Host, field: K): HostGroup[K] | undefined {
    const parts = host.group.split("/").map((p) => p.trim()).filter(Boolean);
    for (let n = parts.length; n > 0; n--) {
      const v = this.groupByPath.get(parts.slice(0, n).join("/"))?.data?.[field];
      if (v) return v;
    }
    return undefined;
  }

  /** A host's environment, falling back to its nearest group's default. */
  effectiveEnv(host: Host | undefined): string {
    if (!host) return "";
    return host.environment || (this.groupDefault(host, "environment") as string | undefined) || "";
  }

  /** The credential a host logs in with, after group defaults. */
  effectiveIdentity(host: Host | undefined): Uuid | undefined {
    if (!host) return undefined;
    return host.identity_id ?? (this.groupDefault(host, "default_identity_id") as Uuid | undefined);
  }

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
      [
        this.hosts,
        this.identities,
        this.snippets,
        this.forwards,
        this.forwardStatus,
        this.keys,
        this.groups,
        this.proxies,
        this.settings,
        this.workspaces,
      ] = await Promise.all([
        api.hosts.list(),
        api.identities.list(),
        api.snippets.list(),
        api.forwards.list(),
        api.forwards.statuses(),
        api.keys.list(),
        api.groups.list(),
        api.proxies.list(),
        api.vault.getSettings(),
        api.workspaces.list(),
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
    this.keys = [];
    this.groups = [];
    this.proxies = [];
    this.workspaces = [];
    this.settings = null;
    this.openConflicts = 0;
    this.forwardStatus = {};
    this.health = {};
  }

  /** Called for every record the Rust watcher sees change on disk. */
  applyChange(c: RecordChange) {
    if (c.conflict_copy) {
      this.openConflicts += 1;
      ui.notify("error", "Another device changed the same item at the same time. Review it under Vault → Conflicts.");
    }
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
      // Secrets arrive redacted through the list commands only, so re-list.
      case "keys":
        void api.keys.list().then((l) => (this.keys = l));
        break;
      case "proxies":
        void api.proxies.list().then((l) => (this.proxies = l));
        break;
      case "groups":
        upsert(this.groups, c.record, c.id);
        break;
      case "workspaces":
        upsert(this.workspaces, c.record, c.id);
        break;
      case "settings":
        void api.vault.getSettings().then((s) => (this.settings = s));
        break;
    }
  }

  // -- lifecycle --------------------------------------------------------

  async setPath(path: string) {
    this.status = await api.vault.setPath(path);
    this.clearRecords();
  }

  async create(password: string, withRecovery: boolean, remember: boolean): Promise<CreateResult> {
    const res = await api.vault.create(password, withRecovery, remember);
    this.pendingRecoveryKey = res.recovery_key;
    this.status = res.status;
    await this.reloadAll();
    return res;
  }

  async #afterUnlock(res: { status: VaultStatus; report: UnlockReport; keychain_error: string | null }) {
    this.status = res.status;
    this.lastUnlock = res.report;
    this.openConflicts = res.report.open_conflicts;
    await this.reloadAll();
    await this.#autoStartForwards();
    const notes: string[] = [];
    if (res.report.migrated) notes.push("The vault was upgraded to the new format; a backup of the old one was kept.");
    if (res.report.merged_conflicts) notes.push(`${res.report.merged_conflicts} sync conflict(s) were merged automatically.`);
    if (res.report.open_conflicts) notes.push(`${res.report.open_conflicts} sync conflict(s) need your decision (Vault screen).`);
    if (res.keychain_error) notes.push(`Couldn't remember the vault on this device: ${res.keychain_error}`);
    if (notes.length) ui.notify(res.report.open_conflicts || res.keychain_error ? "error" : "info", notes.join(" "));
  }

  async unlock(password: string, remember: boolean) {
    await this.#afterUnlock(await api.vault.unlock(password, remember));
  }

  async unlockWithDevice() {
    await this.#afterUnlock(await api.vault.unlockWithDevice());
  }

  async unlockWithRecovery(recoveryKey: string, newPassword: string, remember: boolean) {
    await this.#afterUnlock(await api.vault.unlockWithRecovery(recoveryKey, newPassword, remember));
  }

  /** Start rules marked auto-start that aren't already running. */
  async #autoStartForwards() {
    const idle = this.forwards.filter((f) => {
      const s = this.forwardStatus[f.id]?.state;
      return f.data?.auto_start && s !== "active" && s !== "starting";
    });
    await Promise.all(idle.map((f) => this.startForward(f.id)));
  }

  async lock() {
    this.status = await api.vault.lock();
    this.clearRecords();
    // Backend already closed every session; drop the tabs so they don't
    // silently reconnect after the next unlock.
    ui.resetSession();
  }

  // -- writes (optimistically patch local state with the returned record) --
  //
  // `baseRev` is the revision the caller was editing. Forms capture it when
  // they open, so a change that arrived from another device meanwhile is
  // merged or refused by the backend instead of silently overwritten.
  // Omitted, it defaults to the revision currently shown.

  #rev<T>(list: VaultRecord<T>[], id: Uuid | null, baseRev?: number | null): number | null {
    if (id === null) return null;
    if (baseRev !== undefined) return baseRev;
    return list.find((r) => r.id === id)?.rev ?? null;
  }

  /** After a refused save, pull the other device's version so the UI shows it. */
  async #onConflict(e: unknown) {
    if (isApiError(e) && (e.code === "conflict" || e.code === "deleted")) await this.reloadAll();
    throw e;
  }

  async saveHost(id: Uuid | null, host: Host, baseRev?: number | null) {
    const rec = await api.hosts.save(id, this.#rev(this.hosts, id, baseRev), host).catch((e) => this.#onConflict(e));
    upsert(this.hosts, rec!, rec!.id);
    return rec!;
  }
  /** Save a host and the credentials from its form; refreshes identities too. */
  async saveHostWithCredentials(id: Uuid | null, host: Host, credentials: HostCredentials, baseRev?: number | null) {
    const out = await api.hosts
      .saveWithCredentials(id, this.#rev(this.hosts, id, baseRev), host, credentials)
      .catch((e) => this.#onConflict(e));
    upsert(this.hosts, out!.host, out!.host.id);
    [this.identities, this.keys] = await Promise.all([api.identities.list(), api.keys.list()]);
    return out!;
  }

  /** Offer to put a just-deleted record back. The backend kept the secrets. */
  #offerUndo(collection: Collection, id: Uuid, what: string) {
    ui.notify("info", `${what} deleted.`, {
      label: "Undo",
      run: () =>
        void api
          .undelete(collection, id)
          .then(() => this.reloadAll())
          .catch((e) => ui.notify("error", errorMessage(e))),
    });
  }

  async deleteHost(id: Uuid, baseRev?: number | null) {
    const label = this.hostById.get(id)?.data?.label ?? "Host";
    await api.hosts.delete(id, this.#rev(this.hosts, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.hosts, null, id);
    this.#offerUndo("hosts", id, `"${label}"`);
    // The host's own identity may have been removed with it.
    this.identities = await api.identities.list();
    // Backend detached this host as a jump; mirror that locally.
    for (const h of this.hosts) if (h.data?.jump_host_id === id) h.data.jump_host_id = undefined;
  }

  async toggleFavorite(id: Uuid) {
    const rec = this.hostById.get(id);
    if (!rec?.data) return;
    await this.saveHost(id, { ...$state.snapshot(rec.data), favorite: !rec.data.favorite });
  }

  async saveIdentity(id: Uuid | null, identity: Identity, baseRev?: number | null) {
    const rec = await api.identities
      .save(id, this.#rev(this.identities, id, baseRev), identity)
      .catch((e) => this.#onConflict(e));
    upsert(this.identities, rec!, rec!.id);
    return rec!;
  }
  async deleteIdentity(id: Uuid, baseRev?: number | null) {
    const label = this.identityById.get(id)?.data?.label ?? "Credential";
    await api.identities.delete(id, this.#rev(this.identities, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.identities, null, id);
    this.#offerUndo("identities", id, `"${label}"`);
    // Backend detached this identity from hosts; mirror that locally.
    for (const h of this.hosts) if (h.data?.identity_id === id) h.data.identity_id = undefined;
  }

  async saveSnippet(id: Uuid | null, snippet: Snippet, baseRev?: number | null) {
    const rec = await api.snippets
      .save(id, this.#rev(this.snippets, id, baseRev), snippet)
      .catch((e) => this.#onConflict(e));
    upsert(this.snippets, rec!, rec!.id);
    return rec!;
  }
  async deleteSnippet(id: Uuid, baseRev?: number | null) {
    const label = this.snippets.find((s) => s.id === id)?.data?.label ?? "Snippet";
    await api.snippets.delete(id, this.#rev(this.snippets, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.snippets, null, id);
    this.#offerUndo("snippets", id, `"${label}"`);
  }

  async saveForward(id: Uuid | null, rule: ForwardRule, baseRev?: number | null) {
    const rec = await api.forwards
      .save(id, this.#rev(this.forwards, id, baseRev), rule)
      .catch((e) => this.#onConflict(e));
    upsert(this.forwards, rec!, rec!.id);
    return rec!;
  }
  async deleteForward(id: Uuid, baseRev?: number | null) {
    const label = this.forwards.find((f) => f.id === id)?.data?.label ?? "Rule";
    await api.forwards.delete(id, this.#rev(this.forwards, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.forwards, null, id);
    delete this.forwardStatus[id];
    this.#offerUndo("forwards", id, `"${label}"`);
  }

  async saveGroup(id: Uuid | null, group: HostGroup, baseRev?: number | null) {
    const rec = await api.groups.save(id, this.#rev(this.groups, id, baseRev), group).catch((e) => this.#onConflict(e));
    upsert(this.groups, rec!, rec!.id);
    return rec!;
  }
  async deleteGroup(id: Uuid, baseRev?: number | null) {
    await api.groups.delete(id, this.#rev(this.groups, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.groups, null, id);
  }

  async saveProxy(id: Uuid | null, proxy: Proxy, baseRev?: number | null) {
    const rec = await api.proxies.save(id, this.#rev(this.proxies, id, baseRev), proxy).catch((e) => this.#onConflict(e));
    upsert(this.proxies, rec!, rec!.id);
    return rec!;
  }
  async deleteProxy(id: Uuid, baseRev?: number | null) {
    await api.proxies.delete(id, this.#rev(this.proxies, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.proxies, null, id);
  }

  async saveWorkspace(id: Uuid | null, ws: Workspace, baseRev?: number | null) {
    const rec = await api.workspaces
      .save(id, this.#rev(this.workspaces, id, baseRev), ws)
      .catch((e) => this.#onConflict(e));
    upsert(this.workspaces, rec!, rec!.id);
    return rec!;
  }
  async deleteWorkspace(id: Uuid, baseRev?: number | null) {
    await api.workspaces.delete(id, this.#rev(this.workspaces, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.workspaces, null, id);
  }

  /** Replace a key record returned by a Key Manager command. */
  putKey(rec: VaultRecord<SshKey>) {
    upsert(this.keys, rec, rec.id);
  }
  async deleteKey(id: Uuid, baseRev?: number | null) {
    await api.keys.delete(id, this.#rev(this.keys, id, baseRev)).catch((e) => this.#onConflict(e));
    upsert(this.keys, null, id);
  }

  async saveSettings(next: VaultSettings) {
    const res = await api.vault.saveSettings(this.settings?.rev ?? 0, next).catch((e) => this.#onConflict(e));
    this.settings = res!;
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

  /** Latest reachability result per host, and when it was taken. */
  health = $state<Record<Uuid, Health & { at: number }>>({});
  checking = $state(false);

  async checkHealth(hostIds: Uuid[] | null = null) {
    this.checking = true;
    try {
      const results = await api.health.check(hostIds);
      const at = Date.now();
      for (const r of results) {
        const { host_id, ...h } = r;
        this.health[host_id] = { ...(h as Health), at };
      }
      const down = results.filter((r) => r.state === "down").length;
      ui.notify(down ? "error" : "info", `${results.length - down} of ${results.length} hosts reachable${down ? `, ${down} down` : ""}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      this.checking = false;
    }
  }
}

export const vaultStore = new VaultStore();
