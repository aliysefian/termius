// The saved dashboards: one layout per host and the reusable templates, kept on this computer only (never in the vault,
// never synced) because a layout is how a person likes to look at a host, not a fact about it. Nothing here touches how a
// host is read, who may read it, or any alert: see dashlayout.ts.
import { KEY, addTemplate, applyTemplate, deleteTemplate, layoutFor, readSaved, renameTemplate, resetHost, saveHost, type Layout, type Saved } from "$lib/dashlayout";

function load(): Saved {
  try {
    return readSaved(localStorage.getItem(KEY));
  } catch {
    return readSaved(null);
  }
}

class DashboardsStore {
  saved = $state<Saved>(load());

  constructor() {
    $effect.root(() => {
      $effect(() => {
        const text = JSON.stringify($state.snapshot(this.saved));
        try {
          localStorage.setItem(KEY, text);
        } catch {
          // Not fatal: the layouts just won't be there next time.
        }
      });
    });
  }

  /** The host's own layout, or the default for one that has none. Always a copy: editing it changes nothing saved. */
  layout(hostId: string): Layout {
    return structuredClone($state.snapshot(layoutFor(this.saved, hostId)));
  }

  hasOwn(hostId: string): boolean {
    return hostId in this.saved.hosts;
  }

  save(hostId: string, layout: Layout) {
    this.saved = saveHost(this.saved, hostId, layout);
  }

  /** Back to the default for this host: its saved layout is forgotten. */
  reset(hostId: string) {
    this.saved = resetHost(this.saved, hostId);
  }

  /** Keep a layout as a reusable template; returns its id. */
  saveTemplate(name: string, layout: Layout): string {
    const id = crypto.randomUUID();
    this.saved = addTemplate(this.saved, id, name, layout, Date.now());
    return id;
  }

  rename(id: string, name: string) {
    this.saved = renameTemplate(this.saved, id, name);
  }

  remove(id: string) {
    this.saved = deleteTemplate(this.saved, id);
  }

  /** Give each of these hosts a copy of the template's layout. */
  apply(id: string, hostIds: string[]) {
    this.saved = applyTemplate(this.saved, id, hostIds);
  }

  get templates() {
    return Object.entries(this.saved.templates)
      .map(([id, t]) => ({ id, name: t.name, saved: t.saved }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }
}

export const dashboards = new DashboardsStore();
