// The inventory: what every provider last reported, joined. Plain data and rules, no network and no Svelte, so the
// rules (identity, reconciliation, freshness, search, relationships) are tested on their own.
//
// A provider reports a *scope* as a whole (all the containers of one host, say). Applying a new snapshot of a scope
// replaces the old one, so a container that was removed is gone and is not kept as live. A failed attempt keeps what
// was known and says so: it becomes stale, with the error, never zero.

import type { Freshness, Provider, Relationship, Resource, ResourceStatus, ResourceType, Snapshot } from "./model";

/** An answer this young is live; a connected view refreshes at about this pace. */
export const LIVE_MS = 15_000;
/** Older than this, a successful answer is stale. */
export const FRESH_MS = 5 * 60_000;

export interface ScopeState {
  scopeKey: string;
  provider: Provider;
  hostId: string | null;
  resources: Resource[];
  relationships: Relationship[];
  lastSuccessAt: number | null;
  lastAttemptAt: number | null;
  lastError: string | null;
  /** Why this provider does not apply to the host (no systemd, no container runtime…). */
  unsupported: string | null;
}

export interface ResourceView extends Resource {
  freshness: Freshness;
  observedAt: number | null;
  lastError: string | null;
  unsupportedReason: string | null;
}

export function freshnessOf(s: ScopeState | undefined, now: number): Freshness {
  if (!s) return "unavailable";
  if (s.unsupported) return "unsupported";
  if (s.lastSuccessAt === null) return "unavailable";
  // A failure after the last success means what is shown is the last known, however recent it is.
  if (s.lastError !== null) return "stale";
  const age = now - s.lastSuccessAt;
  return age <= LIVE_MS ? "live" : age <= FRESH_MS ? "fresh" : "stale";
}

/** "Updated 4 s ago", "Last known 2 h ago": the age in words. */
export function ageText(observedAt: number | null, now: number): string {
  if (observedAt === null) return "never";
  const s = Math.max(0, Math.round((now - observedAt) / 1000));
  if (s < 5) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86_400)} d ago`;
}

/** What to print beside a status: the status when it is current, "last known" when it is not. */
export function statusLine(r: ResourceView, now: number): string {
  const text = r.statusText || r.status;
  switch (r.freshness) {
    case "live":
    case "fresh":
      return `${text} · updated ${ageText(r.observedAt, now)}`;
    case "stale":
      return `Last known: ${text} · ${ageText(r.observedAt, now)}`;
    case "unsupported":
      return r.unsupportedReason ?? "Not supported on this host";
    default:
      return "Status unavailable";
  }
}

export class Inventory {
  readonly scopes = new Map<string, ScopeState>();

  #state(scopeKey: string, provider: Provider, hostId: string | null): ScopeState {
    let s = this.scopes.get(scopeKey);
    if (!s) {
      s = { scopeKey, provider, hostId, resources: [], relationships: [], lastSuccessAt: null, lastAttemptAt: null, lastError: null, unsupported: null };
      this.scopes.set(scopeKey, s);
    }
    return s;
  }

  /** The scope's new, complete contents. What was in it before and is not in it now is gone. */
  apply(snap: Snapshot, now: number): void {
    const s = this.#state(snap.scopeKey, snap.provider, snap.hostId);
    s.resources = snap.resources.filter((r) => r.scopeKey === snap.scopeKey);
    s.relationships = snap.relationships;
    s.lastSuccessAt = snap.observedAt;
    s.lastAttemptAt = now;
    s.lastError = null;
    s.unsupported = null;
  }

  /** The attempt failed: keep what was known, and say that it is no longer current. */
  fail(scopeKey: string, provider: Provider, hostId: string | null, error: string, now: number): void {
    const s = this.#state(scopeKey, provider, hostId);
    s.lastAttemptAt = now;
    s.lastError = error;
  }

  /** The host has no such provider. What an earlier answer held does not apply, so it is dropped. */
  markUnsupported(scopeKey: string, provider: Provider, hostId: string | null, reason: string, now: number): void {
    const s = this.#state(scopeKey, provider, hostId);
    s.resources = [];
    s.relationships = [];
    s.lastAttemptAt = now;
    s.lastError = null;
    s.unsupported = reason;
  }

  /** A host was deleted: nothing about it may stay. */
  forgetHost(hostId: string): void {
    for (const [k, s] of this.scopes) if (s.hostId === hostId) this.scopes.delete(k);
  }

  clear(): void {
    this.scopes.clear();
  }

  /** Every resource with the freshness of the scope that reported it. The first report of an id wins. */
  resources(now: number): ResourceView[] {
    const seen = new Set<string>();
    const out: ResourceView[] = [];
    for (const s of this.scopes.values()) {
      const freshness = freshnessOf(s, now);
      for (const r of s.resources) {
        if (seen.has(r.id)) continue;
        seen.add(r.id);
        out.push({ ...r, freshness, observedAt: s.lastSuccessAt, lastError: s.lastError, unsupportedReason: s.unsupported });
      }
    }
    return out;
  }

  /** The verified edges whose two ends both exist: an edge never points at something this inventory does not hold. */
  relationships(now: number): Relationship[] {
    const ids = new Set(this.resources(now).map((r) => r.id));
    const seen = new Set<string>();
    const out: Relationship[] = [];
    for (const s of this.scopes.values()) {
      for (const e of s.relationships) {
        const k = `${e.from}>${e.type}>${e.to}`;
        if (!e.verified || seen.has(k) || !ids.has(e.from) || !ids.has(e.to)) continue;
        seen.add(k);
        out.push(e);
      }
    }
    return out;
  }

  scopesOf(hostId: string): ScopeState[] {
    return [...this.scopes.values()].filter((s) => s.hostId === hostId);
  }

  scope(scopeKey: string): ScopeState | undefined {
    return this.scopes.get(scopeKey);
  }
}

// -- search and filters --------------------------------------------------------------------------------------------

export interface Filters {
  query: string;
  types: ResourceType[];
  statuses: ResourceStatus[];
  /** `""` for any. */
  environment: string;
  hostId: string | null;
}

export const NO_FILTERS: Filters = { query: "", types: [], statuses: [], environment: "", hostId: null };

export const isFiltering = (f: Filters) => f.query.trim() !== "" || f.types.length > 0 || f.statuses.length > 0 || f.environment !== "" || f.hostId !== null;

/** Whether a resource is among what was asked for. `envOf` says a host's environment. Every word must be found. */
export function matches(r: Resource, f: Filters, envOf: (hostId: string | null) => string): boolean {
  if (f.types.length && !f.types.includes(r.type)) return false;
  if (f.statuses.length && !f.statuses.includes(r.status)) return false;
  if (f.hostId !== null && r.hostId !== f.hostId) return false;
  if (f.environment && envOf(r.hostId) !== f.environment) return false;
  const words = f.query.toLowerCase().split(/\s+/).filter(Boolean);
  return words.every((w) => r.haystack.includes(w));
}

/** Matches best first: a name that starts with the query, then one that contains it, then the rest. */
export function search(all: ResourceView[], f: Filters, envOf: (hostId: string | null) => string, limit = 200): ResourceView[] {
  const q = f.query.trim().toLowerCase();
  const rank = (r: Resource) => (!q ? 0 : r.name.toLowerCase().startsWith(q) ? 0 : r.name.toLowerCase().includes(q) ? 1 : 2);
  return all
    .filter((r) => matches(r, f, envOf))
    .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
    .slice(0, limit);
}

// -- counts ------------------------------------------------------------------------------------------------------

export interface Count {
  /** What the inventory holds of this kind, from whatever was discovered. */
  known: number;
  /** Of those, how many are from a scope that is stale. */
  stale: number;
  /** Hosts whose scope for this kind has been read at least once. */
  hostsObserved: number;
  /** Hosts that could have it (saved SSH hosts), so "3 of 10 hosts" can be said instead of a total that looks complete. */
  hostsTotal: number;
  /** Hosts whose last attempt failed. */
  hostsFailed: number;
}

export function summarize(inv: Inventory, now: number, hostIds: string[]): Record<"hosts" | "containers" | "services" | "tmux", Count> {
  const views = inv.resources(now);
  const of = (type: ResourceType) => views.filter((r) => r.type === type);
  const count = (type: ResourceType, provider: Provider): Count => {
    const rs = of(type);
    const scopes = [...inv.scopes.values()].filter((s) => s.provider === provider);
    return {
      known: rs.length,
      stale: rs.filter((r) => r.freshness === "stale").length,
      hostsObserved: new Set(scopes.filter((s) => s.lastSuccessAt !== null || s.unsupported).map((s) => s.hostId)).size,
      hostsTotal: hostIds.length,
      hostsFailed: new Set(scopes.filter((s) => s.lastError !== null).map((s) => s.hostId)).size,
    };
  };
  const hosts = of("host");
  return {
    hosts: { known: hosts.length, stale: 0, hostsObserved: hosts.length, hostsTotal: hostIds.length, hostsFailed: 0 },
    containers: count("container", "docker"),
    services: count("systemd_service", "systemd"),
    tmux: count("tmux_session", "tmux"),
  };
}

/** "38 known · 4 stale · 3 of 10 hosts read": what the number covers, so it is never mistaken for a total. */
export function countText(c: Count, perHost = true): string {
  const bits = [`${c.known} known`];
  if (c.stale) bits.push(`${c.stale} stale`);
  if (perHost && c.hostsObserved < c.hostsTotal) bits.push(`${c.hostsObserved} of ${c.hostsTotal} hosts read`);
  if (c.hostsFailed) bits.push(`${c.hostsFailed} failed`);
  return bits.join(" · ");
}
