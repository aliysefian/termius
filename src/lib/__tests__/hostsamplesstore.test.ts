// The host-samples store: one timer per host however many views ask, readings shared, a hidden window and a locked vault
// taking none, and a failed reading kept as a message without losing the last good one.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
let fail: unknown = null;
let counter = 0;

const reading = (id: string, t: number) => ({
  host_id: id,
  taken_at_ms: t,
  platform: { kind: "linux" },
  baseline: { kind: "measured", elapsed_secs: 2 },
  uptime_secs: 5,
  cpu: { total_pct: 10 + counter, iowait_pct: null, steal_pct: null, cores: [], logical: 1, freq_mhz: null },
  memory: null,
  load: null,
  processes: null,
  filesystems: [],
  disks: [],
  network: null,
  temperatures: [],
  gpus: [],
  notes: [],
  capabilities: {},
  info: {},
});

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    if (cmd === "hostmetrics_sample") {
      if (fail) throw fail;
      counter++;
      return reading(String(args.hostId), 1000 * counter);
    }
    if (cmd === "hostmetrics_history") return [{ t: 100, cpu: 5, mem: 6, disk_used: null, load1: null, rx_bps: null, tx_bps: null, disk_busy: null }];
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));

// A page for the store to listen to (the tests run without one); in place before the store is made.
const page = Object.assign(new EventTarget(), { hidden: false });
vi.stubGlobal("document", page);

const { hostSamples } = await import("$lib/stores/hostsamples.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");

const samples = () => calls.filter(([c]) => c === "hostmetrics_sample").length;
const flush = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
};

beforeEach(() => {
  vi.useFakeTimers();
  calls.length = 0;
  counter = 0;
  fail = null;
  hostSamples.reset();
  vi.spyOn(vaultStore, "unlocked", "get").mockReturnValue(true);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("subscribing", () => {
  it("takes a first reading at once and loads the engine's kept history", async () => {
    const off = hostSamples.subscribe("h1", 2000);
    await flush();
    expect(calls.map(([c]) => c)).toEqual(["hostmetrics_history", "hostmetrics_sample"]);
    expect(hostSamples.latest.h1.cpu?.total_pct).toBe(11);
    // The kept point from before, then the new reading.
    expect(hostSamples.frames.h1.map((f) => f.t)).toEqual([100, 1000]);
    expect(hostSamples.loading.h1).toBe(false);
    off();
  });

  it("runs one timer per host however many views ask, at the fastest asked", async () => {
    const a = hostSamples.subscribe("h1", 5000);
    const b = hostSamples.subscribe("h1", 1000);
    await flush();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(5000);
    expect(samples()).toBe(5); // 1 s timer for both, not 1 s + 5 s
    b();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(samples()).toBe(2); // back to the 5 s one
    a();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(20_000);
    expect(samples()).toBe(0); // nobody left: no timer
  });

  it("changes its timer when a view changes its rate", async () => {
    const off = hostSamples.subscribe("h1", 0);
    await flush();
    calls.length = 0;
    off.set(1000);
    await vi.advanceTimersByTimeAsync(3000);
    expect(samples()).toBe(3);
    off.set(0);
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(5000);
    expect(samples()).toBe(0);
    off();
  });

  it("with the rate off takes the opening reading and no more", async () => {
    const off = hostSamples.subscribe("h1", 0);
    await flush();
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(30_000);
    expect(samples()).toBe(0);
    off();
  });

  it("asks the engine for a reading a little fresher than the interval, so views share", async () => {
    const off = hostSamples.subscribe("h1", 2000);
    await flush();
    const ask = calls.find(([c]) => c === "hostmetrics_sample")![1];
    expect(ask.maxAgeMs).toBe(1500);
    off();
  });
});

describe("pausing", () => {
  it("takes no reading while the window is hidden, and one when it comes back", async () => {
    const off = hostSamples.subscribe("h1", 1000);
    await flush();
    calls.length = 0;
    page.hidden = true;
    await vi.advanceTimersByTimeAsync(5000);
    expect(samples()).toBe(0);
    page.hidden = false;
    page.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(samples()).toBe(1);
    off();
  });

  it("takes none while the vault is locked, and forgets what it had", async () => {
    const off = hostSamples.subscribe("h1", 1000);
    await flush();
    expect(hostSamples.latest.h1).toBeTruthy();
    vi.spyOn(vaultStore, "unlocked", "get").mockReturnValue(false);
    calls.length = 0;
    await hostSamples.refresh("h1", true);
    expect(samples()).toBe(0);
    hostSamples.reset();
    expect(hostSamples.latest.h1).toBeUndefined();
    expect(hostSamples.frames.h1).toBeUndefined();
    off();
  });
});

describe("failures", () => {
  it("keeps the last good reading, shows the message, and clears it when a reading works", async () => {
    const off = hostSamples.subscribe("h1", 0);
    await flush();
    const good = hostSamples.latest.h1;
    fail = { code: "metrics_exec", message: "connection refused" };
    await hostSamples.refresh("h1", true);
    expect(hostSamples.errors.h1).toBe("connection refused");
    expect(hostSamples.latest.h1).toBe(good);
    fail = null;
    await hostSamples.refresh("h1", true);
    expect(hostSamples.errors.h1).toBeNull();
    off();
  });

  it("does not show the engine's pause after a failure as a new failure", async () => {
    const off = hostSamples.subscribe("h1", 0);
    await flush();
    fail = { code: "metrics_exec", message: "refused" };
    await hostSamples.refresh("h1", true);
    fail = { code: "metrics_cooling", message: "refused (trying again in 2 s)" };
    await hostSamples.refresh("h1", true);
    expect(hostSamples.errors.h1).toBe("refused");
    off();
  });

  it("shows a failure of the very first reading instead of loading forever", async () => {
    fail = { code: "metrics_timeout", message: "the host did not answer in time" };
    const off = hostSamples.subscribe("h2", 2000);
    await flush();
    expect(hostSamples.loading.h2).toBe(false);
    expect(hostSamples.errors.h2).toMatch(/did not answer/);
    off();
  });
});

describe("ingest", () => {
  it("counts one reading once, whether it came by event or by reply", () => {
    const r = reading("h3", 5000) as never;
    hostSamples.ingest(r);
    hostSamples.ingest(r);
    expect(hostSamples.frames.h3).toHaveLength(1);
  });
});
