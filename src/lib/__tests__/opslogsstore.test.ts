// The Operations log store: what Follow asks the backend for, how a stream's state is read from its hosts, what a pause
// and a clear do (and don't), changing the hosts of a running stream, stopping, and the way in from other places.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
type Cb = (e: unknown) => void;
let channel: Cb | null = null;
let failStart: string | null = null;

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    constructor(public cb: Cb) {}
  },
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    if (cmd === "ops_logs_start") {
      if (failStart) throw { code: "validation", message: failStart };
      channel = (args.onEvent as { cb: Cb }).cb;
    }
    if (cmd === "ops_logs_stop") return true;
    if (cmd === "ops_logs_context") return { before: [{ host_id: "h1", seq: 0, ts_us: 1, message: "before", cursor: "c0" }], after: [{ host_id: "h1", seq: 0, ts_us: 3, message: "after", cursor: "c2" }] };
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(async () => {}) }));
const askMock = vi.fn(async (_message: string, _opts?: unknown) => true);
vi.mock("$lib/dialogs.svelte", () => ({ ask: askMock, askRemember: vi.fn(), askText: vi.fn() }));

const { opsLogs } = await import("$lib/stores/opslogs.svelte");
const { settings, DEFAULT_PREFS } = await import("$lib/stores/settings.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");
const { alerts } = await import("$lib/stores/alerts.svelte");
const { noFilter } = await import("$lib/ops/logfilter");
// Taken before anything changes a preference: the store works on the defaults object itself when nothing is saved yet.
const PRISTINE = structuredClone(DEFAULT_PREFS);

const host = (id: string, label: string, identity: string | null) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: `${label}.x`, port: 22, group: "", tags: [], notes: "", identity_id: identity ?? undefined } });
const count = (cmd: string) => calls.filter(([c]) => c === cmd).length;
const last = (cmd: string) => [...calls].reverse().find(([c]) => c === cmd)?.[1];
const emit = (e: unknown) => channel?.(e);
const state = (host_id: string, s: string, extra: Record<string, unknown> = {}) => emit({ event: "state", host_id, state: s, attempt: 0, retry_in_secs: null, message: null, notice: null, ...extra });
const entry = (host_id: string, seq: number, o: Record<string, unknown> = {}) => ({ host_id, seq, ts_us: 1_700_000_000_000_000 + seq * 1000, priority: 6, unit: "sshd.service", ident: "sshd", pid: 1, message: `m${seq}`, cursor: `c${seq}`, ...o });
const batch = (host_id: string, entries: unknown[], dropped = 0) => emit({ event: "batch", host_id, entries, dropped });
const tick = async () => {
  await vi.advanceTimersByTimeAsync(150);
};

beforeEach(async () => {
  vi.useFakeTimers();
  calls.length = 0;
  channel = null;
  failStart = null;
  askMock.mockClear();
  Object.assign(settings.prefs, structuredClone(PRISTINE));
  vaultStore.hosts = [host("h1", "claude", "i1"), host("h2", "Elastic", "i1"), host("h3", "lab", "i1"), host("h4", "no-creds", null)] as never;
  vaultStore.identities = [{ id: "i1", rev: 1, updated_at: 1, deleted: false, data: { label: "ops", username: "ops", auth: { type: "password", password: "" }, notes: "" } }] as never;
  vaultStore.status = { state: "unlocked", path: "/v", vault_id: "v" } as never;
  opsLogs.reset();
  opsLogs.filter = noFilter();
  await vi.advanceTimersByTimeAsync(10);
});

afterEach(async () => {
  opsLogs.reset();
  vi.useRealTimers();
});

describe("what Follow needs", () => {
  it("explains why it can't start, and starts when it can", () => {
    expect(opsLogs.blocker).toMatch(/at least one host/);
    opsLogs.hosts = ["h4"];
    expect(opsLogs.blocker).toMatch(/saved credentials/);
    opsLogs.hosts = ["h1", "h4"];
    expect(opsLogs.blocker).toBeNull();
    settings.prefs.opsLogUnit = "nginx; reboot";
    expect(opsLogs.blocker).toMatch(/nginx\.service/);
    settings.prefs.opsLogUnit = "nginx.service";
    expect(opsLogs.blocker).toBeNull();
    settings.prefs.opsLogSource = "file";
    settings.prefs.opsLogPath = "relative";
    expect(opsLogs.blocker).toMatch(/absolute/);
  });

  it("asks the backend for the usable hosts and the chosen source, history and secret hiding", async () => {
    opsLogs.hosts = ["h1", "h2", "h4"];
    settings.prefs.opsLogUnit = "nginx.service";
    settings.prefs.opsLogPriority = 4;
    settings.prefs.opsLogHistory = "last:500";
    await opsLogs.start();
    const a = last("ops_logs_start")!;
    expect(a.hostIds).toEqual(["h1", "h2"]);
    expect(a.spec).toEqual({ source: { kind: "journal", unit: "nginx.service", kernel: undefined, priority: 4 }, history: { last: 500 }, auto_reconnect: true, redact: true });
    expect(opsLogs.running).toBe(true);
  });

  it("is started once however many times it is pressed", async () => {
    opsLogs.hosts = ["h1"];
    await Promise.all([opsLogs.start(), opsLogs.start(), opsLogs.start()]);
    expect(count("ops_logs_start")).toBe(1);
  });

  it("shows a refusal from the backend and doesn't pretend to be following", async () => {
    opsLogs.hosts = ["h1"];
    failStart = "Too many logs are being followed already.";
    await opsLogs.start();
    expect(opsLogs.startError).toMatch(/Too many/);
    expect(opsLogs.running).toBe(false);
    expect(opsLogs.phase).toBe("idle");
  });

  it("uses a log file's path, and its history can only be lines", async () => {
    opsLogs.hosts = ["h1"];
    settings.prefs.opsLogSource = "file";
    settings.prefs.opsLogPath = "/var/log/syslog";
    settings.prefs.opsLogHistory = "since:3600";
    await opsLogs.start();
    expect(last("ops_logs_start")!.spec).toMatchObject({ source: { kind: "file", path: "/var/log/syslog" }, history: { last: 100 } });
  });
});

describe("what a stream says about itself", () => {
  beforeEach(async () => {
    opsLogs.hosts = ["h1", "h2", "h3"];
    await opsLogs.start();
  });

  it("is connecting, then live only when every host streams", () => {
    expect(opsLogs.phase).toBe("connecting");
    state("h1", "streaming");
    expect(opsLogs.phase).toBe("connecting".replace("connecting", "reconnecting"));
    state("h2", "streaming");
    state("h3", "streaming");
    expect(opsLogs.phase).toBe("streaming");
    expect(opsLogs.counts).toMatchObject({ streaming: 3, total: 3 });
  });

  it("is partial, not healthy, while one host reconnects, and live again after", () => {
    for (const h of ["h1", "h2", "h3"]) state(h, "streaming");
    state("h2", "reconnecting", { attempt: 2, retry_in_secs: 8, message: "The connection closed." });
    expect(opsLogs.phase).toBe("reconnecting");
    expect(opsLogs.counts).toMatchObject({ streaming: 2, reconnecting: 1 });
    expect(opsLogs.hostStates.h2).toMatchObject({ state: "reconnecting", attempt: 2, message: "The connection closed." });
    expect(opsLogs.hostStates.h2.retryAt).toBeGreaterThan(Date.now());
    state("h2", "streaming");
    expect(opsLogs.phase).toBe("streaming");
  });

  it("is failed only when nothing is left streaming or trying", () => {
    state("h1", "failed", { message: "authentication failed" });
    state("h2", "failed", { message: "authentication failed" });
    state("h3", "failed", { message: "authentication failed" });
    expect(opsLogs.phase).toBe("failed");
    state("h3", "reconnecting");
    expect(opsLogs.phase).toBe("reconnecting");
  });

  it("keeps what the command printed that isn't a log line", () => {
    state("h1", "streaming", { notice: "Hint: You are currently not seeing messages from other users and the system." });
    expect(opsLogs.hostStates.h1.notice).toMatch(/not seeing messages/);
    state("h1", "streaming");
    expect(opsLogs.hostStates.h1.notice).toMatch(/not seeing messages/);
  });

  it("ignores events from a stream that was replaced", async () => {
    const old = channel!;
    for (const h of ["h1", "h2", "h3"]) state(h, "stopped");
    await opsLogs.stop();
    expect(opsLogs.running).toBe(false);
    await opsLogs.start();
    old({ event: "batch", host_id: "h1", entries: [entry("h1", 1)], dropped: 0 });
    await tick();
    expect(opsLogs.rows).toHaveLength(0);
  });
});

describe("what arrives", () => {
  beforeEach(async () => {
    opsLogs.hosts = ["h1", "h2"];
    await opsLogs.start();
    state("h1", "streaming");
    state("h2", "streaming");
  });

  it("turns batches into rows that say which SSHVault host they came from, in batches no faster than a tenth of a second", async () => {
    batch("h1", [entry("h1", 1), entry("h1", 2)]);
    batch("h2", [entry("h2", 1, { journal_host: "some-other-name" })]);
    expect(opsLogs.rows).toHaveLength(0);
    await tick();
    expect(opsLogs.rows).toHaveLength(3);
    const byHost = opsLogs.rows.map((r) => r.host);
    expect(byHost.filter((h) => h === "claude")).toHaveLength(2);
    expect(byHost.filter((h) => h === "Elastic")).toHaveLength(1);
    expect(opsLogs.rows.find((r) => r.hostId === "h2")!.journalHost).toBe("some-other-name");
    expect(opsLogs.hostStates.h1.count).toBe(2);
    expect(opsLogs.lastEventAt).not.toBeNull();
  });

  it("counts what the backend left out, and what fell out of a full buffer", async () => {
    batch("h1", [], 40);
    batch("h1", [], 2);
    await tick();
    expect(opsLogs.droppedRemote).toBe(42);
    // (The store applies the preference with an effect; here it is called directly.)
    opsLogs.resize(100);
    batch("h1", Array.from({ length: 150 }, (_, i) => entry("h1", i + 1)));
    await tick();
    expect(opsLogs.rows).toHaveLength(100);
    expect(opsLogs.evicted).toBe(50);
  });

  it("holds the view still on pause while collecting goes on, and shows what came on resume", async () => {
    batch("h1", [entry("h1", 1)]);
    await tick();
    opsLogs.togglePause();
    batch("h1", [entry("h1", 2), entry("h1", 3)]);
    await tick();
    expect(opsLogs.shown).toHaveLength(1);
    expect(opsLogs.arrivedWhilePaused).toBe(2);
    expect(opsLogs.running).toBe(true);
    expect(count("ops_logs_stop")).toBe(0);
    opsLogs.togglePause();
    expect(opsLogs.shown).toHaveLength(3);
    expect(opsLogs.arrivedWhilePaused).toBe(0);
  });

  it("clears only the display: the stream carries on and nothing is stopped", async () => {
    batch("h1", [entry("h1", 1)]);
    await tick();
    opsLogs.clear();
    expect(opsLogs.rows).toHaveLength(0);
    expect(opsLogs.running).toBe(true);
    expect(count("ops_logs_stop")).toBe(0);
    batch("h1", [entry("h1", 2)]);
    await tick();
    expect(opsLogs.rows).toHaveLength(1);
  });

  it("filters what is shown without losing what is held", async () => {
    batch("h1", [entry("h1", 1, { priority: 3, message: "bad thing" }), entry("h1", 2, { priority: 6 })]);
    await tick();
    opsLogs.filter = { ...noFilter(), sev: "error" };
    expect(opsLogs.shown.map((r) => r.message)).toEqual(["bad thing"]);
    expect(opsLogs.rows).toHaveLength(2);
    expect(opsLogs.isFiltered).toBe(true);
  });

  it("opens a row's details and asks the host for the entries around it, by the entry's position", async () => {
    batch("h1", [entry("h1", 1, { cursor: "s=1;i=1" })]);
    await tick();
    const row = opsLogs.rows[0];
    opsLogs.toggle(row);
    await vi.advanceTimersByTimeAsync(10);
    const a = last("ops_logs_context")!;
    expect(a).toMatchObject({ hostId: "h1", cursor: "s=1;i=1", before: 5, after: 5, redact: true });
    expect(opsLogs.contexts[row.id].state).toBe("ready");
    expect(opsLogs.contexts[row.id].before.map((r) => r.message)).toEqual(["before"]);
    expect(opsLogs.expanded).toBe(row.id);
    opsLogs.toggle(row);
    expect(opsLogs.expanded).toBeNull();
  });

  it("says plainly that a log file's lines have no surroundings to read", async () => {
    batch("h1", [entry("h1", 1, { cursor: undefined })]);
    await tick();
    opsLogs.toggle(opsLogs.rows[0]);
    await vi.advanceTimersByTimeAsync(10);
    expect(opsLogs.contexts[opsLogs.rows[0].id].state).toBe("unavailable");
    expect(count("ops_logs_context")).toBe(0);
  });

  it("raises an alert for a critical entry only when that is turned on, and not twice", async () => {
    settings.prefs.alerts = true;
    settings.prefs.alertLogCritical = true;
    alerts.log = [];
    batch("h1", [entry("h1", 1, { priority: 2, message: "hardware error", unit: "kernel", ident: "kernel" })]);
    await tick();
    batch("h1", [entry("h1", 2, { priority: 2, message: "hardware error", unit: "kernel", ident: "kernel" })]);
    await tick();
    expect(alerts.log.filter((a) => a.kind === "log")).toHaveLength(1);
    expect(alerts.log[0]).toMatchObject({ hostId: "h1", host: "claude", severity: "critical" });
  });
});

describe("changing the hosts of a running stream", () => {
  beforeEach(async () => {
    opsLogs.hosts = ["h1", "h2"];
    await opsLogs.start();
    state("h1", "streaming");
    state("h2", "streaming");
  });

  it("holds the change until it is applied, then adds and removes without restarting", async () => {
    opsLogs.hosts = ["h2", "h3"];
    expect(opsLogs.pendingHosts).toEqual({ add: ["h3"], remove: ["h1"] });
    expect(count("ops_logs_hosts")).toBe(0);
    await opsLogs.applyHosts();
    expect(last("ops_logs_hosts")).toMatchObject({ add: ["h3"], remove: ["h1"] });
    expect(count("ops_logs_start")).toBe(1);
    expect(opsLogs.pendingHosts).toEqual({ add: [], remove: [] });
    expect(opsLogs.hostStates.h3.state).toBe("connecting");
  });

  it("can drop the change", () => {
    opsLogs.hosts = ["h3"];
    opsLogs.revertHosts();
    expect(opsLogs.hosts).toEqual(["h1", "h2"]);
    expect(opsLogs.pendingHosts).toEqual({ add: [], remove: [] });
  });

  it("knows when the source, unit, priority or history changed, which only a restart applies", () => {
    expect(opsLogs.pendingSpec).toBe(false);
    settings.prefs.opsLogUnit = "nginx.service";
    expect(opsLogs.pendingSpec).toBe(true);
    settings.prefs.opsLogUnit = "";
    expect(opsLogs.pendingSpec).toBe(false);
    // Auto-reconnect is applied at once, so it is never "pending".
    return opsLogs.setAutoReconnect(false).then(() => {
      expect(last("ops_logs_auto_reconnect")).toMatchObject({ on: false });
      expect(opsLogs.pendingSpec).toBe(false);
    });
  });

  it("applies a host added by hand to the stream's own spec without asking for hosts without credentials", () => {
    opsLogs.hosts = ["h1", "h2", "h4"];
    expect(opsLogs.pendingHosts.add).toEqual([]);
  });
});

describe("stopping", () => {
  it("is stopping until every host has said so, then stopped, with what was read kept", async () => {
    opsLogs.hosts = ["h1", "h2"];
    await opsLogs.start();
    for (const h of ["h1", "h2"]) state(h, "streaming");
    batch("h1", [entry("h1", 1)]);
    await tick();
    const done = opsLogs.stop();
    await done;
    expect(count("ops_logs_stop")).toBe(1);
    expect(opsLogs.phase).toBe("stopping");
    expect(opsLogs.running).toBe(true);
    state("h1", "stopped");
    expect(opsLogs.phase).toBe("stopping");
    state("h2", "stopped");
    expect(opsLogs.phase).toBe("stopped");
    expect(opsLogs.running).toBe(false);
    expect(opsLogs.rows).toHaveLength(1);
    expect(Object.values(opsLogs.hostStates).every((h) => h.state === "stopped")).toBe(true);
  });

  it("doesn't wait for ever for a host that never answers", async () => {
    opsLogs.hosts = ["h1"];
    await opsLogs.start();
    state("h1", "streaming");
    await opsLogs.stop();
    await vi.advanceTimersByTimeAsync(11_000);
    expect(opsLogs.running).toBe(false);
  });

  it("starts a new stream with an empty view", async () => {
    opsLogs.hosts = ["h1"];
    await opsLogs.start();
    state("h1", "streaming");
    batch("h1", [entry("h1", 1)]);
    await tick();
    await opsLogs.stop();
    state("h1", "stopped");
    await opsLogs.start();
    expect(opsLogs.rows).toHaveLength(0);
    expect(opsLogs.received).toBe(0);
  });

  it("ends the stream and drops what was read when told the vault locked", async () => {
    opsLogs.hosts = ["h1"];
    await opsLogs.start();
    // What the effect on the vault's state does when it locks.
    opsLogs.reset();
    expect(count("ops_logs_stop")).toBeGreaterThan(0);
    expect(opsLogs.running).toBe(false);
    expect(opsLogs.rows).toHaveLength(0);
  });
});

describe("coming from somewhere else", () => {
  it("follows one host and unit when nothing is running", async () => {
    await opsLogs.inspect("h2", "nginx.service");
    expect(last("ops_logs_start")).toMatchObject({ hostIds: ["h2"] });
    expect(settings.prefs.opsLogUnit).toBe("nginx.service");
  });

  it("filters a stream that is running, adding the host if it wasn't in it", async () => {
    opsLogs.hosts = ["h1"];
    await opsLogs.start();
    state("h1", "streaming");
    await opsLogs.inspect("h3", "ssh.service");
    expect(last("ops_logs_hosts")).toMatchObject({ add: ["h3"] });
    expect(count("ops_logs_start")).toBe(1);
    expect(opsLogs.filter).toMatchObject({ hosts: ["h3"], units: ["ssh.service"] });
  });
});

describe("secrets in what is saved", () => {
  it("asks before copying or saving lines that were read with secret hiding off", async () => {
    opsLogs.hosts = ["h1"];
    settings.prefs.opsLogRedact = false;
    await opsLogs.start();
    state("h1", "streaming");
    batch("h1", [entry("h1", 1, { message: "password=hunter2" })]);
    await tick();
    expect(last("ops_logs_start")!.spec).toMatchObject({ redact: false });
    expect(opsLogs.rawHeld).toBe(true);
    await opsLogs.copyShown();
    expect(askMock).toHaveBeenCalled();
    expect(String(askMock.mock.calls[0][0])).toMatch(/not hidden/);
  });

  it("doesn't ask when they were hidden", async () => {
    opsLogs.hosts = ["h1"];
    await opsLogs.start();
    state("h1", "streaming");
    batch("h1", [entry("h1", 1)]);
    await tick();
    await opsLogs.copyShown();
    expect(askMock).not.toHaveBeenCalled();
  });
});
