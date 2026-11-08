import { describe, expect, it } from "vitest";
import { COLS, History, WIDGETS, WIDGET_TYPES, addTemplate, addWidget, applyTemplate, clampSize, defaultLayout, deleteTemplate, emptySaved, layoutFor, moveBy, moveTo, nameProblem, normalize, readSaved, removeItem, renameTemplate, resetHost, resize, saveHost, sameLayout, setHidden, setView, shown, viewOf, type Layout } from "../dashlayout";

const keys = (l: Layout) => l.items.map((i) => i.key);
const vis = (l: Layout) => shown(l).map((i) => i.key);

describe("the default layout", () => {
  it("is a row of figures, then charts, with the widgets that read on request off", () => {
    const l = defaultLayout();
    expect(vis(l).slice(0, 6)).toEqual(["cpu-1", "memory-1", "filesystems-1", "network-1", "gpu-1", "temperature-1"]);
    expect(l.items.filter((i) => WIDGETS[i.type].reads).every((i) => i.hidden)).toBe(true);
    expect(new Set(keys(l)).size).toBe(l.items.length);
    for (const i of l.items) {
      expect(i.w).toBeGreaterThanOrEqual(WIDGETS[i.type].minW);
      expect(i.w).toBeLessThanOrEqual(COLS);
      expect(WIDGETS[i.type].views.some((v) => v.id === i.view)).toBe(true);
    }
  });

  it("offers every widget type at least once", () => {
    expect(new Set(defaultLayout().items.map((i) => i.type))).toEqual(new Set(WIDGET_TYPES));
  });

  it("is never shared: each call is its own", () => {
    const a = defaultLayout();
    a.items[0].w = 9;
    expect(defaultLayout().items[0].w).toBe(2);
  });
});

describe("moving", () => {
  const base = { items: defaultLayout().items.slice(0, 5).map((i) => ({ ...i })) };

  it("moves a widget to a place among the shown ones", () => {
    expect(keys(moveTo(base, "cpu-1", 3))).toEqual(["memory-1", "filesystems-1", "network-1", "cpu-1", "gpu-1"]);
    expect(keys(moveTo(base, "gpu-1", 0))).toEqual(["gpu-1", "cpu-1", "memory-1", "filesystems-1", "network-1"]);
    expect(keys(moveTo(base, "cpu-1", 99))).toEqual(["memory-1", "filesystems-1", "network-1", "gpu-1", "cpu-1"]);
  });

  it("counts only the widgets this host can show, so a drop lands where it was aimed", () => {
    const only = (t: string) => t !== "memory";
    // On screen: cpu, filesystems, network, gpu. Dropping gpu on the second one (filesystems) puts it there, past memory.
    expect(keys(moveTo(base, "gpu-1", 1, only))).toEqual(["cpu-1", "memory-1", "gpu-1", "filesystems-1", "network-1"]);
    expect(keys(moveTo(base, "cpu-1", 2, only))).toEqual(["memory-1", "filesystems-1", "network-1", "cpu-1", "gpu-1"]);
  });

  it("returns the same layout when nothing moved, and ignores an unknown key", () => {
    expect(moveTo(base, "cpu-1", 0)).toBe(base);
    expect(moveTo(base, "nope", 2)).toBe(base);
  });

  it("steps one place at a time over the shown widgets, skipping hidden ones, and stops at the ends", () => {
    const l = setHidden(base, "memory-1", true);
    expect(vis(moveBy(l, "cpu-1", 1))).toEqual(["filesystems-1", "cpu-1", "network-1", "gpu-1"]);
    expect(moveBy(l, "cpu-1", -1)).toBe(l);
    expect(moveBy(l, "gpu-1", 1)).toBe(l);
    expect(moveBy(l, "memory-1", 1)).toBe(l); // hidden: not on the screen to move
  });

  it("does not move the widget past ones the host cannot show", () => {
    const only = (t: string) => t !== "memory";
    const l = moveBy(base, "cpu-1", 1, only);
    // It swaps with the next widget that would be on screen (filesystems), stepping over memory, which this host cannot show.
    expect(keys(l)).toEqual(["filesystems-1", "memory-1", "cpu-1", "network-1", "gpu-1"]);
    expect(shown(l, only).map((i) => i.key)).toEqual(["filesystems-1", "cpu-1", "network-1", "gpu-1"]);
  });
});

describe("resizing, hiding and changing the view", () => {
  const l = defaultLayout();

  it("keeps a size inside what the widget allows, in whole columns and rows", () => {
    expect(clampSize("cpu", 0, 0)).toEqual({ w: 2, h: 2 });
    expect(clampSize("cpu", 50, 50)).toEqual({ w: 12, h: 12 });
    expect(clampSize("system", 1, 1)).toEqual({ w: 3, h: 3 });
    expect(clampSize("cpu", 5.6, 3.4)).toEqual({ w: 6, h: 3 });
    expect(clampSize("cpu", NaN, NaN)).toEqual({ w: 2, h: 2 });
    const r = resize(l, "cpu-2", 12, 6);
    expect(r.items.find((i) => i.key === "cpu-2")).toMatchObject({ w: 12, h: 6 });
    expect(resize(l, "cpu-2", 6, 4)).toBe(l);
    expect(resize(l, "nope", 6, 4)).toBe(l);
  });

  it("hides and shows without losing the place", () => {
    const h = setHidden(l, "cpu-1", true);
    expect(vis(h)).not.toContain("cpu-1");
    expect(keys(h)).toEqual(keys(l));
    expect(vis(setHidden(h, "cpu-1", false))[0]).toBe("cpu-1");
    expect(setHidden(l, "cpu-1", false)).toBe(l);
  });

  it("only offers a widget's own views", () => {
    expect(viewOf("cpu", "cores")).toBe("cores");
    expect(viewOf("cpu", "table")).toBe("card");
    expect(setView(l, "cpu-1", "cores").items[0].view).toBe("cores");
    expect(setView(l, "cpu-1", "bogus")).toBe(l);
    expect(setView(l, "cpu-1", "card")).toBe(l);
  });

  it("adds a widget with the next free key, and removes one for good", () => {
    const a = addWidget(l, "cpu");
    expect(a.items[a.items.length - 1]).toMatchObject({ key: "cpu-3", type: "cpu", view: "card", hidden: false });
    expect(addWidget(a, "cpu").items.at(-1)?.key).toBe("cpu-4");
    expect(keys(removeItem(a, "cpu-3"))).toEqual(keys(l));
    expect(removeItem(l, "nope")).toBe(l);
  });

  it("stops adding at a limit", () => {
    let big = l;
    for (let i = 0; i < 100; i++) big = addWidget(big, "uptime");
    expect(big.items.length).toBe(60);
  });
});

describe("undo and redo", () => {
  it("takes an edit back and puts it again, and a new edit forgets what was undone", () => {
    const h = new History(defaultLayout());
    const a = h.push(setHidden(h.now, "cpu-1", true));
    h.push(resize(a, "cpu-2", 12, 4));
    expect(h.canUndo && !h.canRedo).toBe(true);
    expect(h.undo().items.find((i) => i.key === "cpu-2")?.w).toBe(6);
    expect(h.canRedo).toBe(true);
    expect(h.redo().items.find((i) => i.key === "cpu-2")?.w).toBe(12);
    h.undo();
    h.push(addWidget(h.now, "gpu"));
    expect(h.canRedo).toBe(false);
    expect(h.undo().items.some((i) => i.key === "gpu-2")).toBe(false);
  });

  it("does not count an edit that changed nothing, and does nothing at the ends", () => {
    const h = new History(defaultLayout());
    h.push(defaultLayout());
    expect(h.canUndo).toBe(false);
    const start = h.now;
    expect(h.undo()).toBe(start);
    expect(h.redo()).toBe(start);
  });

  it("is bounded", () => {
    const h = new History(defaultLayout(), 3);
    let l = h.now;
    for (let i = 0; i < 10; i++) l = h.push(resize(l, "cpu-2", 2 + (i % 2) * 3, 4 + i));
    let steps = 0;
    while (h.canUndo) {
      h.undo();
      steps++;
    }
    expect(steps).toBe(3);
  });
});

describe("repairing a stored layout", () => {
  it("drops what is unknown, fixes sizes and views, and makes keys unique", () => {
    const l = normalize({ items: [{ key: "a", type: "cpu", w: 99, h: 0, view: "nope", hidden: false }, { key: "a", type: "memory", w: 3, h: 3, view: "bar" }, { type: "bogus" }, "junk", null, { type: "gpu", w: 4, h: 3, view: "chart", hidden: true }] });
    expect(l).not.toBeNull();
    const [a, b, c] = l!.items;
    expect(a).toMatchObject({ key: "a", type: "cpu", w: 12, h: 2, view: "card", hidden: false });
    expect(b).toMatchObject({ type: "memory", view: "bar" });
    expect(b.key).not.toBe("a");
    expect(new Set(keys(l!)).size).toBe(l!.items.length);
    expect(c).toMatchObject({ type: "gpu", hidden: true });
  });

  it("offers widgets it has never heard of, hidden, and never puts one on screen by itself", () => {
    const l = normalize({ items: [{ key: "cpu-1", type: "cpu", w: 2, h: 2, view: "card", hidden: false }] })!;
    expect(vis(l)).toEqual(["cpu-1"]);
    expect(l.items.filter((i) => i.hidden).map((i) => i.type).sort()).toEqual(WIDGET_TYPES.filter((t) => t !== "cpu").sort());
  });

  it("refuses what is not a layout", () => {
    for (const x of [null, undefined, 5, "x", [], {}, { items: "no" }]) expect(normalize(x)).toBeNull();
  });
});

describe("what is kept", () => {
  it("reads nothing, junk or the wrong shape as empty", () => {
    for (const raw of [null, undefined, "", "nope", "[]", "5", "null", "{}"]) expect(readSaved(raw)).toEqual(emptySaved());
  });

  it("keeps good hosts and templates and drops bad ones one by one", () => {
    const good = defaultLayout();
    const s = readSaved(JSON.stringify({ hosts: { h1: good, h2: "junk", h3: { items: 5 } }, templates: { t1: { name: "  Web  ", layout: good, saved: 5 }, t2: { name: "", layout: good }, t3: { name: "x", layout: 7 } } }));
    expect(Object.keys(s.hosts)).toEqual(["h1"]);
    expect(Object.keys(s.templates)).toEqual(["t1"]);
    expect(s.templates.t1.name).toBe("Web");
  });

  it("gives a host its own layout, or the default", () => {
    let s = emptySaved();
    expect(sameLayout(layoutFor(s, "h1"), defaultLayout())).toBe(true);
    const mine = setHidden(defaultLayout(), "cpu-1", true);
    s = saveHost(s, "h1", mine);
    expect(sameLayout(layoutFor(s, "h1"), mine)).toBe(true);
    expect(sameLayout(layoutFor(s, "h2"), defaultLayout())).toBe(true);
    expect(sameLayout(layoutFor(resetHost(s, "h1"), "h1"), defaultLayout())).toBe(true);
    // A saved layout is a copy, not the draft being edited.
    mine.items[0].w = 9;
    expect(layoutFor(s, "h1").items[0].w).toBe(2);
  });
});

describe("templates", () => {
  const mine = setHidden(defaultLayout(), "cpu-1", true);

  it("are saved, renamed and deleted by id, with a name that is unique and bounded", () => {
    let s = addTemplate(emptySaved(), "t1", "  Web servers ", mine, 100);
    expect(s.templates.t1).toMatchObject({ name: "Web servers", saved: 100 });
    expect(nameProblem("web SERVERS", s)).toMatch(/already exists/);
    expect(nameProblem("web servers", s, "t1")).toBeNull();
    expect(nameProblem("   ", s)).toMatch(/Give the template a name/);
    expect(nameProblem("x".repeat(61), s)).toMatch(/at most 60/);
    s = renameTemplate(s, "t1", " DB ");
    expect(s.templates.t1.name).toBe("DB");
    expect(renameTemplate(s, "nope", "x")).toBe(s);
    expect(deleteTemplate(s, "t1").templates).toEqual({});
  });

  it("give each chosen host its own copy and touch no other host", () => {
    let s = addTemplate(emptySaved(), "t1", "Web", mine, 1);
    s = saveHost(s, "keep", setHidden(defaultLayout(), "gpu-1", true));
    const before = s.hosts.keep;
    const after = applyTemplate(s, "t1", ["a", "b"]);
    expect(Object.keys(after.hosts).sort()).toEqual(["a", "b", "keep"]);
    expect(after.hosts.keep).toBe(before);
    expect(sameLayout(after.hosts.a, mine)).toBe(true);
    after.hosts.a.items[0].w = 12;
    expect(after.hosts.b.items[0].w).toBe(2);
    expect(after.templates.t1.layout.items[0].w).toBe(2);
    expect(applyTemplate(s, "nope", ["a"])).toBe(s);
  });

  it("a template survives the host layouts being reset, and the other way round", () => {
    let s = addTemplate(emptySaved(), "t1", "Web", mine, 1);
    s = saveHost(s, "h1", mine);
    expect(resetHost(s, "h1").templates.t1).toBeTruthy();
    expect(deleteTemplate(s, "t1").hosts.h1).toBeTruthy();
  });
});
