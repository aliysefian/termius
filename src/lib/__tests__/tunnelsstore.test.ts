// The Tunnels store: operations are accepted once, a partial failure is reported as one, backend lines are
// appended without repeats and kept bounded, and a deleted tunnel doesn't linger in the selection.
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
let failFor = new Set<string>();
const listeners: Record<string, ((e: { payload: unknown }) => void)[]> = {};

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    if (["forward_start", "forward_stop", "forward_restart"].includes(cmd) && failFor.has(String(args.id))) throw { code: "ssh", message: `refused ${String(args.id)}` };
    if (cmd === "forward_logs") return [];
    if (cmd === "forward_runtime") return {};
    if (cmd === "delete_forward") return null;
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn(async (event: string, handler: (e: { payload: unknown }) => void) => {
    (listeners[event] ??= []).push(handler);
    return () => (listeners[event] = (listeners[event] ?? []).filter((h) => h !== handler));
  }),
}));
vi.mock("$lib/dialogs.svelte", () => ({ ask: vi.fn(async () => true), askRemember: vi.fn(async () => ({ ok: true, checked: false })), askText: vi.fn(async () => null) }));

const { tunnels } = await import("$lib/stores/tunnels.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");
const { ui } = await import("$lib/stores/ui.svelte");

const rec = (id: string, label: string, port: number) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, host_id: "h1", auto_start: false, kind: "local" as const, bind_addr: "127.0.0.1", bind_port: port, dest_host: "127.0.0.1", dest_port: port } });
const count = (cmd: string, id?: string) => calls.filter(([c, a]) => c === cmd && (id === undefined || a.id === id)).length;

beforeEach(() => {
  calls.length = 0;
  failFor = new Set();
  for (const k of Object.keys(listeners)) delete listeners[k];
  ui.toasts = [];
  vaultStore.hosts = [{ id: "h1", rev: 1, updated_at: 1, deleted: false, data: { label: "claude", hostname: "h", port: 22, group: "", tags: [], notes: "", favorite: false } }] as never;
  vaultStore.forwards = [rec("a", "alpha", 8001), rec("b", "beta", 8002), rec("c", "gamma", 8003)] as never;
  vaultStore.forwardStatus = { a: { state: "stopped" }, b: { state: "active", port: 8002 }, c: { state: "error", message: "boom" } };
  tunnels.pending = {};
  tunnels.checked = new Set();
  tunnels.selectedId = null;
  tunnels.logs = {};
  tunnels.clearFilters();
});

describe("operations", () => {
  it("accepts a start once: a second click while it is underway does nothing", async () => {
    const first = tunnels.start("a");
    const second = await tunnels.start("a");
    await first;
    expect(second).toBe("not startable");
    expect(count("forward_start", "a")).toBe(1);
  });

  it("refuses to start what is running, to stop what is not, and to restart what is stopped", async () => {
    expect(await tunnels.start("b")).toBe("not startable");
    expect(await tunnels.stop("a")).toBe("not running");
    expect(await tunnels.restart("a")).toBe("not restartable");
    expect(calls.filter(([c]) => c.startsWith("forward_"))).toHaveLength(0);
  });

  it("stops and restarts through the backend and marks the operation until it reports", async () => {
    await tunnels.stop("b");
    expect(tunnels.pending.b).toBe("stopping");
    expect(tunnels.rows.find((r) => r.id === "b")?.status).toBe("stopping");
    tunnels.pending = {};
    await tunnels.restart("b");
    expect(count("forward_restart", "b")).toBe(1);
    expect(tunnels.rows.find((r) => r.id === "b")?.status).toBe("connecting");
  });

  it("shows a start the backend refuses as failed, with its words, and says so", async () => {
    failFor.add("a");
    const err = await tunnels.start("a");
    expect(err).toContain("refused a");
    expect(tunnels.pending.a).toBeUndefined();
    expect(tunnels.rows.find((r) => r.id === "a")?.status).toBe("failed");
    expect(ui.toasts.some((t) => t.kind === "error" && t.text.includes("refused a"))).toBe(true);
  });

  it("does not offer a start for a tunnel whose host is gone", async () => {
    vaultStore.hosts = [] as never;
    expect(await tunnels.start("a")).toBe("not startable");
  });
});

describe("bulk actions", () => {
  it("reports a partial failure as one rather than a success", async () => {
    vaultStore.forwardStatus = { a: { state: "stopped" }, b: { state: "stopped" }, c: { state: "stopped" } };
    failFor.add("b");
    tunnels.setAllChecked(["a", "b", "c"], true);
    await tunnels.startChecked();
    expect(count("forward_start")).toBe(3);
    const toast = ui.toasts.find((t) => t.text.startsWith("Started"));
    expect(toast?.kind).toBe("error");
    expect(toast?.text).toContain("Started 2 of 3");
    expect(toast?.text).toContain("beta (refused b)");
  });

  it("acts only on the tunnels it applies to, and says when none do", async () => {
    tunnels.setAllChecked(["a"], true);
    await tunnels.stopChecked();
    expect(count("forward_stop")).toBe(0);
    expect(ui.toasts.some((t) => t.text.includes("None of the selected tunnels can be stopped"))).toBe(true);
    tunnels.setAllChecked(["a", "b", "c"], true);
    await tunnels.startChecked();
    expect([count("forward_start", "a"), count("forward_start", "b"), count("forward_start", "c")]).toEqual([1, 0, 1]);
  });

  it("deletes the selected, after asking, and forgets them", async () => {
    tunnels.setAllChecked(["a", "c"], true);
    tunnels.select("a");
    await tunnels.deleteChecked();
    expect(count("delete_forward", "a") + count("delete_forward", "c")).toBe(2);
    expect(tunnels.checked.size).toBe(0);
    expect(tunnels.selectedId).toBeNull();
  });
});

describe("selection and filters", () => {
  it("keeps the filters when a tunnel is selected, and clears them together", () => {
    tunnels.query = "alpha";
    tunnels.typeFilter = "local";
    tunnels.statusFilter = "stopped";
    tunnels.select("a", "logs");
    expect([tunnels.query, tunnels.typeFilter, tunnels.statusFilter, tunnels.tab, tunnels.selectedId]).toEqual(["alpha", "local", "stopped", "logs", "a"]);
    expect(tunnels.filtering).toBe(true);
    tunnels.clearFilters();
    expect(tunnels.filtering).toBe(false);
  });

  it("lists the tunnels by name with their real states", () => {
    expect(tunnels.rows.map((r) => [r.rule.label, r.status])).toEqual([["alpha", "stopped"], ["beta", "active"], ["gamma", "failed"]]);
  });

  it("only slides the details over the list when they can't sit beside it", () => {
    tunnels.docked = true;
    tunnels.reveal();
    expect(tunnels.slideOver).toBe(false);
    tunnels.docked = false;
    tunnels.reveal();
    expect(tunnels.slideOver).toBe(true);
  });
});

describe("what the backend pushes", () => {
  it("appends log lines without repeats and keeps at most 200", async () => {
    const off = await tunnels.subscribe();
    const push = (n: number) => listeners["forward:log"].forEach((h) => h({ payload: { rule_id: "a", entry: { at_ms: n, level: "info", message: `line ${n}` } } }));
    for (let i = 0; i < 230; i++) push(i);
    push(229);
    expect(tunnels.logs.a).toHaveLength(200);
    expect(tunnels.logs.a.at(-1)?.message).toBe("line 229");
    expect(tunnels.logs.a[0].message).toBe("line 30");
    off();
    expect(listeners["forward:log"] ?? []).toHaveLength(0);
  });

  it("announces a start once it really is active, and a failure with the backend's words", async () => {
    const off = await tunnels.subscribe();
    await tunnels.start("a");
    listeners["forward:status"].forEach((h) => h({ payload: { rule_id: "a", status: { state: "active", port: 8001 } } }));
    expect(tunnels.pending.a).toBeUndefined();
    expect(ui.toasts.some((t) => t.kind === "info" && t.text === 'Started tunnel "alpha".')).toBe(true);
    listeners["forward:status"].forEach((h) => h({ payload: { rule_id: "c", status: { state: "error", message: "lost" } } }));
    // Whether or not it was asked for, the failure is announced with the backend's own words.
    expect(ui.toasts.some((t) => t.kind === "error" && t.text.includes("gamma") && t.text.includes("lost"))).toBe(true);
    off();
  });
});
