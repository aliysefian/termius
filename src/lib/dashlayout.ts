// The layout of a host's metrics dashboard: which widgets, in what order, how big, shown how. Pure, so every edit (move,
// resize, hide, change the view, undo) is tested. A layout is presentation only: it never touches how often a host is read,
// who may read it, or any alert setting, and applying one host's layout to another changes nothing but this.
//
// The grid has 12 columns. Widgets flow in order and each spans `w` columns and `h` rows; the browser places them, so a layout
// cannot overlap or leave a widget off-screen.

export const COLS = 12;
export const MAX_H = 12;
const MAX_ITEMS = 60;

export type WidgetType = "cpu" | "memory" | "filesystems" | "diskio" | "network" | "gpu" | "temperature" | "uptime" | "system" | "processes" | "ports" | "logins";

export interface WidgetView {
  id: string;
  label: string;
}

export interface WidgetDef {
  type: WidgetType;
  title: string;
  /** The ways it can be shown; the first is the default. */
  views: WidgetView[];
  minW: number;
  minH: number;
  /** It reads something on request (the list of processes, ports or logins), so it costs the host a reading while shown. */
  reads: boolean;
}

const v = (id: string, label: string): WidgetView => ({ id, label });

export const WIDGETS: Record<WidgetType, WidgetDef> = {
  cpu: { type: "cpu", title: "CPU", views: [v("card", "Compact"), v("chart", "Chart"), v("cores", "Per core")], minW: 2, minH: 2, reads: false },
  memory: { type: "memory", title: "Memory", views: [v("card", "Compact"), v("chart", "Chart"), v("bar", "Composition")], minW: 2, minH: 2, reads: false },
  filesystems: { type: "filesystems", title: "Filesystem usage", views: [v("summary", "Compact"), v("bars", "Bars"), v("table", "Table")], minW: 2, minH: 2, reads: false },
  diskio: { type: "diskio", title: "Physical disk I/O", views: [v("table", "Table"), v("chart", "Chart")], minW: 3, minH: 2, reads: false },
  network: { type: "network", title: "Network RX/TX", views: [v("card", "Compact"), v("chart", "Chart"), v("interfaces", "Interfaces")], minW: 2, minH: 2, reads: false },
  gpu: { type: "gpu", title: "GPU", views: [v("card", "Compact"), v("chart", "Chart")], minW: 2, minH: 2, reads: false },
  temperature: { type: "temperature", title: "Temperature", views: [v("card", "Compact"), v("list", "Sensors"), v("chart", "Chart")], minW: 2, minH: 2, reads: false },
  uptime: { type: "uptime", title: "Uptime", views: [v("card", "Compact")], minW: 2, minH: 1, reads: false },
  system: { type: "system", title: "System information", views: [v("list", "List")], minW: 3, minH: 3, reads: false },
  processes: { type: "processes", title: "Top processes", views: [v("top", "Top by CPU")], minW: 3, minH: 3, reads: true },
  ports: { type: "ports", title: "Listening ports", views: [v("list", "List")], minW: 3, minH: 3, reads: true },
  logins: { type: "logins", title: "Recent logins", views: [v("list", "List")], minW: 3, minH: 3, reads: true },
};

export const WIDGET_TYPES = Object.keys(WIDGETS) as WidgetType[];

export interface Item {
  /** Unique within a layout: a type may appear more than once (a compact CPU card and a CPU chart). */
  key: string;
  type: WidgetType;
  w: number;
  h: number;
  view: string;
  hidden: boolean;
}

export interface Layout {
  items: Item[];
}

const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, Math.round(Number.isFinite(n) ? n : lo)));

export function clampSize(type: WidgetType, w: number, h: number): { w: number; h: number } {
  const d = WIDGETS[type];
  return { w: clamp(w, d.minW, COLS), h: clamp(h, d.minH, MAX_H) };
}

/** A view the widget really has, else its first. */
export const viewOf = (type: WidgetType, view: string): string => (WIDGETS[type].views.some((x) => x.id === view) ? view : WIDGETS[type].views[0].id);

const mk = (key: string, type: WidgetType, w: number, h: number, view: string, hidden = false): Item => ({ key, type, w, h, view, hidden });

/** What a host starts with: a row of compact figures, then the charts, then the lists. Widgets that read on request are off. */
export function defaultLayout(): Layout {
  return {
    items: [
      mk("cpu-1", "cpu", 2, 2, "card"),
      mk("memory-1", "memory", 2, 2, "card"),
      mk("filesystems-1", "filesystems", 2, 2, "summary"),
      mk("network-1", "network", 2, 2, "card"),
      mk("gpu-1", "gpu", 2, 2, "card"),
      mk("temperature-1", "temperature", 2, 2, "card"),
      mk("cpu-2", "cpu", 6, 4, "chart"),
      mk("network-2", "network", 6, 4, "chart"),
      mk("memory-2", "memory", 6, 4, "chart"),
      mk("filesystems-2", "filesystems", 6, 4, "bars"),
      mk("system-1", "system", 4, 4, "list"),
      mk("diskio-1", "diskio", 8, 4, "table"),
      mk("uptime-1", "uptime", 4, 2, "card"),
      mk("processes-1", "processes", 6, 5, "top", true),
      mk("ports-1", "ports", 6, 5, "list", true),
      mk("logins-1", "logins", 6, 5, "list", true),
    ],
  };
}

const copy = (l: Layout): Layout => ({ items: l.items.map((i) => ({ ...i })) });
const idx = (l: Layout, key: string) => l.items.findIndex((i) => i.key === key);

/** The items that are shown, in order: not hidden, and (when given) of a kind this host can show. */
export function shown(l: Layout, available?: (t: WidgetType) => boolean): Item[] {
  return l.items.filter((i) => !i.hidden && (!available || available(i.type)));
}

/** Put an item where another shown item is (or after the last). `to` counts the items that are on screen only. */
export function moveTo(l: Layout, key: string, to: number, available?: (t: WidgetType) => boolean): Layout {
  const from = idx(l, key);
  if (from < 0) return l;
  const out = copy(l);
  const [item] = out.items.splice(from, 1);
  const visible = shown(out, available);
  const target = visible[Math.max(0, to)];
  const at = target ? out.items.indexOf(target) : out.items.length;
  out.items.splice(at, 0, item);
  return out.items.every((x, i) => x.key === l.items[i].key) ? l : out;
}

/** One step earlier (-1) or later (+1) among the shown items. The edge is the edge: it does not wrap. */
export function moveBy(l: Layout, key: string, delta: -1 | 1, available?: (t: WidgetType) => boolean): Layout {
  const vis = shown(l, available);
  const at = vis.findIndex((i) => i.key === key);
  const to = at + delta;
  if (at < 0 || to < 0 || to >= vis.length) return l;
  // Swap with the neighbour: the item takes the neighbour's place.
  const out = copy(l);
  const a = out.items.findIndex((i) => i.key === key);
  const b = out.items.findIndex((i) => i.key === vis[to].key);
  [out.items[a], out.items[b]] = [out.items[b], out.items[a]];
  return out;
}

export function resize(l: Layout, key: string, w: number, h: number): Layout {
  const at = idx(l, key);
  if (at < 0) return l;
  const { w: nw, h: nh } = clampSize(l.items[at].type, w, h);
  if (nw === l.items[at].w && nh === l.items[at].h) return l;
  const out = copy(l);
  out.items[at] = { ...out.items[at], w: nw, h: nh };
  return out;
}

export function setHidden(l: Layout, key: string, hidden: boolean): Layout {
  const at = idx(l, key);
  if (at < 0 || l.items[at].hidden === hidden) return l;
  const out = copy(l);
  out.items[at] = { ...out.items[at], hidden };
  return out;
}

export function setView(l: Layout, key: string, view: string): Layout {
  const at = idx(l, key);
  if (at < 0) return l;
  const nv = viewOf(l.items[at].type, view);
  if (nv === l.items[at].view) return l;
  const out = copy(l);
  out.items[at] = { ...out.items[at], view: nv };
  return out;
}

/** A new widget of a type, at the end, shown. Capped so a layout cannot grow without limit. */
export function addWidget(l: Layout, type: WidgetType): Layout {
  if (l.items.length >= MAX_ITEMS) return l;
  let n = 1;
  while (l.items.some((i) => i.key === `${type}-${n}`)) n++;
  const d = WIDGETS[type];
  const out = copy(l);
  out.items.push(mk(`${type}-${n}`, type, Math.max(d.minW, 4), Math.max(d.minH, 3), d.views[0].id));
  return out;
}

/** Take an item out of the layout for good (hiding keeps it; this does not). */
export function removeItem(l: Layout, key: string): Layout {
  return idx(l, key) < 0 ? l : { items: l.items.filter((i) => i.key !== key) };
}

export function sameLayout(a: Layout, b: Layout): boolean {
  return a.items.length === b.items.length && a.items.every((x, i) => x.key === b.items[i].key && x.type === b.items[i].type && x.w === b.items[i].w && x.h === b.items[i].h && x.view === b.items[i].view && x.hidden === b.items[i].hidden);
}

/**
 * A layout from whatever was stored: unknown widget kinds dropped, sizes and views repaired, duplicate keys made unique,
 * and widgets a newer version added appended hidden, so nothing appears that the person did not put there. Null when it
 * is not a layout at all.
 */
export function normalize(raw: unknown): Layout | null {
  if (!raw || typeof raw !== "object" || !Array.isArray((raw as { items?: unknown }).items)) return null;
  const seen = new Set<string>();
  const items: Item[] = [];
  for (const r of (raw as { items: unknown[] }).items) {
    if (items.length >= MAX_ITEMS || !r || typeof r !== "object") continue;
    const o = r as Record<string, unknown>;
    const type = o.type as WidgetType;
    if (typeof type !== "string" || !(type in WIDGETS)) continue;
    let key = typeof o.key === "string" && o.key.length > 0 && o.key.length <= 40 ? o.key : `${type}-1`;
    for (let n = 2; seen.has(key); n++) key = `${type}-${n}`;
    seen.add(key);
    const { w, h } = clampSize(type, Number(o.w), Number(o.h));
    items.push({ key, type, w, h, view: viewOf(type, String(o.view ?? "")), hidden: o.hidden === true });
  }
  // A widget type the saved layout has never heard of (added since) is offered, but not put on the screen.
  for (const t of WIDGET_TYPES) {
    if (!items.some((i) => i.type === t) && items.length < MAX_ITEMS) {
      let n = 1;
      while (seen.has(`${t}-${n}`)) n++;
      seen.add(`${t}-${n}`);
      items.push({ key: `${t}-${n}`, type: t, ...clampSize(t, 4, 3), view: WIDGETS[t].views[0].id, hidden: true });
    }
  }
  return { items };
}

// -- undo and redo -----------------------------------------------------------------------------------------------------

/** The states of an edit, so a change can be taken back and put again. An edit equal to the one before is not a step. */
export class History {
  #past: Layout[] = [];
  #future: Layout[] = [];
  #now: Layout;
  constructor(start: Layout, private readonly limit = 50) {
    this.#now = start;
  }
  get now() {
    return this.#now;
  }
  get canUndo() {
    return this.#past.length > 0;
  }
  get canRedo() {
    return this.#future.length > 0;
  }
  push(next: Layout): Layout {
    if (sameLayout(next, this.#now)) return this.#now;
    this.#past.push(this.#now);
    if (this.#past.length > this.limit) this.#past.shift();
    this.#future = [];
    this.#now = next;
    return next;
  }
  undo(): Layout {
    const p = this.#past.pop();
    if (p) {
      this.#future.push(this.#now);
      this.#now = p;
    }
    return this.#now;
  }
  redo(): Layout {
    const f = this.#future.pop();
    if (f) {
      this.#past.push(this.#now);
      this.#now = f;
    }
    return this.#now;
  }
}

// -- what is kept ----------------------------------------------------------------------------------------------------------

export interface Template {
  name: string;
  layout: Layout;
  saved: number;
}

export interface Saved {
  /** One layout per host id. A host with none uses the default. */
  hosts: Record<string, Layout>;
  templates: Record<string, Template>;
}

export const KEY = "sshvault.hostmetrics.dashboards.v1";
export const MAX_NAME = 60;
const MAX_HOSTS = 500;
const MAX_TEMPLATES = 50;

export const emptySaved = (): Saved => ({ hosts: {}, templates: {} });

/** Whatever was stored, repaired piece by piece: a bad host's layout is dropped, the rest is kept. */
export function readSaved(raw: string | null | undefined): Saved {
  const out = emptySaved();
  let o: unknown;
  try {
    o = raw ? JSON.parse(raw) : null;
  } catch {
    return out;
  }
  if (!o || typeof o !== "object" || Array.isArray(o)) return out;
  const { hosts, templates } = o as { hosts?: unknown; templates?: unknown };
  if (hosts && typeof hosts === "object" && !Array.isArray(hosts)) {
    for (const [id, l] of Object.entries(hosts as Record<string, unknown>)) {
      const n = id.length <= 64 && Object.keys(out.hosts).length < MAX_HOSTS ? normalize(l) : null;
      if (n) out.hosts[id] = n;
    }
  }
  if (templates && typeof templates === "object" && !Array.isArray(templates)) {
    for (const [id, t] of Object.entries(templates as Record<string, unknown>)) {
      if (id.length > 64 || Object.keys(out.templates).length >= MAX_TEMPLATES || !t || typeof t !== "object") continue;
      const { name, layout, saved } = t as Record<string, unknown>;
      const n = normalize(layout);
      const nm = typeof name === "string" ? name.trim().slice(0, MAX_NAME) : "";
      if (n && nm) out.templates[id] = { name: nm, layout: n, saved: typeof saved === "number" ? saved : 0 };
    }
  }
  return out;
}

/** Why a template name is not acceptable, or null. Names are unique ignoring case, so two cannot be told apart in a list. */
export function nameProblem(name: string, saved: Saved, exceptId?: string): string | null {
  const n = name.trim();
  if (!n) return "Give the template a name.";
  if (n.length > MAX_NAME) return `A name is at most ${MAX_NAME} characters.`;
  if (Object.entries(saved.templates).some(([id, t]) => id !== exceptId && t.name.toLowerCase() === n.toLowerCase())) return "A template with that name already exists.";
  return null;
}

export function saveHost(s: Saved, hostId: string, layout: Layout): Saved {
  return { ...s, hosts: { ...s.hosts, [hostId]: copy(layout) } };
}

export function resetHost(s: Saved, hostId: string): Saved {
  const hosts = { ...s.hosts };
  delete hosts[hostId];
  return { ...s, hosts };
}

export function addTemplate(s: Saved, id: string, name: string, layout: Layout, now: number): Saved {
  return Object.keys(s.templates).length >= MAX_TEMPLATES ? s : { ...s, templates: { ...s.templates, [id]: { name: name.trim(), layout: copy(layout), saved: now } } };
}

export function renameTemplate(s: Saved, id: string, name: string): Saved {
  const t = s.templates[id];
  return t ? { ...s, templates: { ...s.templates, [id]: { ...t, name: name.trim() } } } : s;
}

export function deleteTemplate(s: Saved, id: string): Saved {
  const templates = { ...s.templates };
  delete templates[id];
  return { ...s, templates };
}

/** Give each chosen host its own copy of a template's layout. Only layouts change: a host's other settings are not in `Saved`. */
export function applyTemplate(s: Saved, id: string, hostIds: string[]): Saved {
  const t = s.templates[id];
  if (!t) return s;
  const hosts = { ...s.hosts };
  for (const h of hostIds) hosts[h] = copy(t.layout);
  return { ...s, hosts };
}

export const layoutFor = (s: Saved, hostId: string): Layout => s.hosts[hostId] ?? defaultLayout();
