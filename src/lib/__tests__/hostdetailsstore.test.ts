// The processes / ports / logins store: one timer per view and host, nothing while hidden or locked, and the last good list
// kept through a failure.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
let fail: unknown = null;
let n = 0;

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    if (!cmd.startsWith("hostmetrics_")) return null;
    if (fail) throw fail;
    n++;
    if (cmd === "hostmetrics_processes") return { host_id: args.hostId, taken_at_ms: n, total: 1, truncated: false, baseline: { kind: "first" }, commands: true, items: [] };
    if (cmd === "hostmetrics_ports") return { host_id: args.hostId, taken_at_ms: n, source: "ss", owners_hidden: false, items: [] };
    if (cmd === "hostmetrics_logins") return { host_id: args.hostId, taken_at_ms: n, sessions: [], recent: [], failed_logins: [], auth: [], auth_source: null, limits: [] };
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));

const page = Object.assign(new EventTarget(), { hidden: false });
vi.stubGlobal("document", page);

const { hostDetails } = await import("$lib/stores/hostdetails.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");

const count = (cmd: string) => calls.filter(([c]) => c === cmd).length;
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
};

beforeEach(() => {
  vi.useFakeTimers();
  calls.length = 0;
  n = 0;
  fail = null;
  page.hidden = false;
  hostDetails.reset();
  vi.spyOn(vaultStore, "unlocked", "get").mockReturnValue(true);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("watching a view", () => {
  it("reads it at once, then on its own timer, and shares one timer between watchers", async () => {
    const a = hostDetails.watch("processes", "h1");
    const b = hostDetails.watch("processes", "h1");
    await flush();
    expect(count("hostmetrics_processes")).toBe(1);
    expect(hostDetails.processes.h1.total).toBe(1);
    expect(hostDetails.loading["processes:h1"]).toBe(false);
    await vi.advanceTimersByTimeAsync(9000);
    expect(count("hostmetrics_processes")).toBe(4); // 3 s timer, not doubled by the second watcher
    a();
    await vi.advanceTimersByTimeAsync(3000);
    expect(count("hostmetrics_processes")).toBe(5); // still one watcher
    b();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(count("hostmetrics_processes")).toBe(5); // none left: no timer
  });

  it("gives each view its own pace", async () => {
    const off = [hostDetails.watch("ports", "h1"), hostDetails.watch("logins", "h1")];
    await flush();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(30_000);
    expect(count("hostmetrics_ports")).toBe(3);
    expect(count("hostmetrics_logins")).toBe(1);
    off.forEach((f) => f());
  });

  it("asks for a list a little fresher than its interval so views share, and for a new one when forced", async () => {
    const off = hostDetails.watch("processes", "h1");
    await flush();
    expect(calls.find(([c]) => c === "hostmetrics_processes")![1].maxAgeMs).toBe(2250);
    await hostDetails.refresh("processes", "h1", true);
    expect(calls.filter(([c]) => c === "hostmetrics_processes").pop()![1].maxAgeMs).toBe(0);
    off();
  });

  it("stopping twice does not stop someone else's watch", async () => {
    const a = hostDetails.watch("ports", "h1");
    const b = hostDetails.watch("ports", "h1");
    a();
    a();
    await flush();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(count("hostmetrics_ports")).toBe(1);
    b();
  });
});

describe("pausing and failing", () => {
  it("takes nothing while hidden, and reads again when the window is back", async () => {
    const off = hostDetails.watch("processes", "h1");
    await flush();
    calls.length = 0;
    page.hidden = true;
    await vi.advanceTimersByTimeAsync(9000);
    expect(count("hostmetrics_processes")).toBe(0);
    page.hidden = false;
    page.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(count("hostmetrics_processes")).toBe(1);
    off();
  });

  it("takes nothing while the vault is locked and forgets what it had", async () => {
    const off = hostDetails.watch("logins", "h1");
    await flush();
    expect(hostDetails.logins.h1).toBeTruthy();
    vi.spyOn(vaultStore, "unlocked", "get").mockReturnValue(false);
    calls.length = 0;
    await hostDetails.refresh("logins", "h1", true);
    expect(count("hostmetrics_logins")).toBe(0);
    hostDetails.reset();
    expect(hostDetails.logins.h1).toBeUndefined();
    off();
  });

  it("keeps the last good list through a failure, shows it, and ignores the engine's own pause", async () => {
    const off = hostDetails.watch("ports", "h1");
    await flush();
    const good = hostDetails.ports.h1;
    fail = { code: "metrics_exec", message: "connection refused" };
    await hostDetails.refresh("ports", "h1", true);
    expect(hostDetails.errors["ports:h1"]).toBe("connection refused");
    expect(hostDetails.ports.h1).toBe(good);
    fail = { code: "metrics_cooling", message: "x (trying again in 2 s)" };
    await hostDetails.refresh("ports", "h1", true);
    expect(hostDetails.errors["ports:h1"]).toBe("connection refused");
    fail = null;
    await hostDetails.refresh("ports", "h1", true);
    expect(hostDetails.errors["ports:h1"]).toBeNull();
    off();
  });

  it("a failure of the first reading ends the loading state with a message", async () => {
    fail = { code: "metrics_timeout", message: "the host did not answer in time" };
    const off = hostDetails.watch("processes", "h2");
    await flush();
    expect(hostDetails.loading["processes:h2"]).toBe(false);
    expect(hostDetails.errors["processes:h2"]).toMatch(/did not answer/);
    off();
  });
});
