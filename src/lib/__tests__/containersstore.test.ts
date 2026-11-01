// The Containers store: what a host switch lets go of, what a refresh keeps, how actions are asked for and reported,
// and how a published port is reached (directly on this computer, through a tunnel on a host).
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: [string, Record<string, unknown>][] = [];
const opened: string[] = [];
let failFor = new Set<string>();
let statsRound = 0;
const urls: string[] = [];

const container = (id: string, name: string, state: string) => ({ id: id.padEnd(64, "0"), name, image: "nginx", state, status: state === "running" ? "Up 1 hour" : "Exited (0) 1 hour ago", ports: [], created: 1, size: null, labels: {}, command: "", pod: null, mounts: [], networks: [] });

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  convertFileSrc: (p: string) => p,
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push([cmd, args]);
    switch (cmd) {
      case "containers_open":
        opened.push(String(args.hostId));
        return { session_id: `s-${String(args.hostId)}`, runtimes: ["docker", "podman"] };
      case "containers_list":
        // Each host has different containers, so a mix-up would show.
        return { containers: args.sessionId === "s-null" ? [container("a1", "alpha", "running"), container("b1", "beta", "exited")] : [container("c1", "gamma", "running")], images: [], images_error: null };
      case "containers_stats":
        statsRound++;
        return [{ id: "a1".padEnd(12, "0"), name: "alpha", cpu_pct: statsRound, mem_usage: "1MiB / 1GiB", mem_pct: 0.1, net_io: "0B / 0B", block_io: "0B / 0B", pids: 1 }];
      case "containers_act":
        if (failFor.has(String(args.id).slice(0, 2))) throw { code: "ssh", message: `refused ${String(args.id).slice(0, 2)}\nmore` };
        return null;
      case "forward_start_adhoc":
        return "tunnel-1";
      case "forward_stop":
        return null;
      default:
        return null;
    }
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async (u: string) => void urls.push(u)) }));
vi.mock("$lib/dialogs.svelte", () => ({ ask: vi.fn(async () => true), askRemember: vi.fn(async () => ({ ok: true, checked: false })), askText: vi.fn(async () => null) }));

const { containers, LOCAL } = await import("$lib/stores/containers.svelte");
const { settings } = await import("$lib/stores/settings.svelte");
const { vaultStore } = await import("$lib/stores/vault.svelte");
const { ui } = await import("$lib/stores/ui.svelte");
const { ask } = await import("$lib/dialogs.svelte");
const { collapseKey, portTargets } = await import("$lib/containerview");

const count = (cmd: string, pick?: (a: Record<string, unknown>) => boolean) => calls.filter(([c, a]) => c === cmd && (!pick || pick(a))).length;

beforeEach(async () => {
  opened.length = 0;
  urls.length = 0;
  failFor = new Set();
  statsRound = 0;
  ui.toasts = [];
  for (const k of Object.keys(containers.sources)) await containers.close(k);
  containers.reset();
  vaultStore.hosts = [{ id: "h1", rev: 1, updated_at: 1, deleted: false, data: { label: "claude", hostname: "h", port: 22, group: "", tags: [], notes: "", favorite: false } }] as never;
  vi.mocked(ask).mockClear();
  settings.prefs.containerCollapsed = {};
  calls.length = 0;
});

describe("hosts and runtimes", () => {
  it("keeps each host's containers to itself, and lets go of the selection when the host changes", async () => {
    await containers.switchTo(LOCAL);
    containers.select("a1".padEnd(64, "0"));
    containers.toggleChecked("a1".padEnd(64, "0"));
    expect(containers.active?.listing?.containers.map((c) => c.name)).toEqual(["alpha", "beta"]);
    await containers.switchTo("h1");
    expect(containers.activeKey).toBe("h1");
    expect(containers.selectedId).toBeNull();
    expect(containers.checked.size).toBe(0);
    expect(containers.active?.listing?.containers.map((c) => c.name)).toEqual(["gamma"]);
    // Going back finds the first host as it was, without opening it again.
    await containers.switchTo(LOCAL);
    expect(containers.active?.listing?.containers.map((c) => c.name)).toEqual(["alpha", "beta"]);
    expect(opened).toEqual(["null", "h1"]);
  });

  it("changing the runtime forgets the old listing and asks the new runtime", async () => {
    await containers.switchTo(LOCAL);
    containers.setRuntime(LOCAL, "podman");
    expect(containers.active?.listing).toBeNull();
    await vi.waitFor(() => expect(containers.active?.listing).not.toBeNull());
    expect(calls.filter(([c]) => c === "containers_list").at(-1)?.[1].runtime).toBe("podman");
  });

  it("closing a host forgets its selection and stops the tunnels it opened", async () => {
    await containers.switchTo("h1");
    containers.portTunnels["h1:8080"] = { id: "tunnel-1", port: 40000 };
    containers.select("c1".padEnd(64, "0"));
    await containers.close("h1");
    expect(containers.selectedId).toBeNull();
    expect(containers.portTunnels["h1:8080"]).toBeUndefined();
    expect(count("forward_stop")).toBe(1);
    expect(count("containers_close")).toBe(1);
  });
});

describe("stats", () => {
  it("is read only while monitoring is on, and kept as a bounded history per container", async () => {
    await containers.switchTo(LOCAL);
    expect(count("containers_stats")).toBe(0);
    containers.setStats(LOCAL, true);
    await vi.waitFor(() => expect(count("containers_stats")).toBe(1));
    for (let i = 0; i < 70; i++) await containers.refresh(LOCAL);
    const history = containers.history(LOCAL, "a1".padEnd(64, "0"));
    expect(history).toHaveLength(60);
    expect(history.at(-1)?.cpu).toBe(statsRound);
    containers.setStats(LOCAL, false);
    const before = count("containers_stats");
    await containers.refresh(LOCAL);
    expect(count("containers_stats")).toBe(before);
  });
});

describe("actions", () => {
  it("pauses and resumes through the backend and says so", async () => {
    await containers.switchTo(LOCAL);
    const a = containers.active!.listing!.containers[0];
    expect(await containers.act(LOCAL, a, { action: "pause" })).toBe(true);
    expect(await containers.act(LOCAL, a, { action: "unpause" })).toBe(true);
    expect(calls.filter(([c]) => c === "containers_act").map(([, x]) => (x.action as { action: string }).action)).toEqual(["pause", "unpause"]);
    expect(ui.toasts.map((t) => t.text)).toEqual(['Paused "alpha".', 'Resumed "alpha".']);
  });

  it("closes the drawer of a container that was removed, and refuses a second action while one is underway", async () => {
    await containers.switchTo(LOCAL);
    const [a] = containers.active!.listing!.containers;
    containers.select(a.id);
    const first = containers.act(LOCAL, a, { action: "stop" });
    expect(await containers.act(LOCAL, a, { action: "stop" })).toBe(false);
    await first;
    expect(count("containers_act")).toBe(1);
    await containers.act(LOCAL, a, { action: "remove", force: false });
    expect(containers.selectedId).toBeNull();
  });

  it("asks once for a bulk action, and reports a partial failure with the reason", async () => {
    await containers.switchTo(LOCAL);
    const list = containers.active!.listing!.containers;
    failFor.add("b1");
    const done = await containers.bulk(LOCAL, list, "remove");
    expect(done).toBe(1);
    expect(vi.mocked(ask)).toHaveBeenCalledTimes(1);
    const toast = ui.toasts.at(-1);
    expect(toast?.kind).toBe("error");
    expect(toast?.text).toContain("1 of 2 done");
    expect(toast?.text).toContain("beta (refused b1)");
  });

  it("a bulk start on an ordinary host needs no question", async () => {
    await containers.switchTo(LOCAL);
    await containers.bulk(LOCAL, [containers.active!.listing!.containers[1]], "start");
    expect(vi.mocked(ask)).not.toHaveBeenCalled();
    expect(ui.toasts.at(-1)?.text).toBe("Started 1 container.");
  });
});

describe("reaching a published port", () => {
  const web = portTargets([{ host_ip: "0.0.0.0", host_port: "8080", container_port: "80", proto: "tcp" }])[0];

  it("opens this computer's own published port directly", async () => {
    await containers.switchTo(LOCAL);
    await containers.openPort(LOCAL, web);
    expect(urls).toEqual(["http://127.0.0.1:8080"]);
    expect(count("forward_start_adhoc")).toBe(0);
  });

  it("reaches a host's port through a tunnel to the host, not by treating the host's loopback as this computer's", async () => {
    await containers.switchTo("h1");
    vaultStore.forwardStatus = { "tunnel-1": { state: "active", port: 41234 } };
    await containers.openPort("h1", web);
    expect(calls.find(([c]) => c === "forward_start_adhoc")?.[1]).toMatchObject({ hostId: "h1", kind: { kind: "local", bind_port: 0, dest_host: "127.0.0.1", dest_port: 8080 } });
    expect(urls).toEqual(["http://127.0.0.1:41234"]);
    // A second click reuses the tunnel that is up.
    await containers.openPort("h1", web);
    expect(count("forward_start_adhoc")).toBe(1);
    expect(urls).toHaveLength(2);
  });

  it("says why when the tunnel can't be made", async () => {
    await containers.switchTo("h1");
    vaultStore.forwardStatus = { "tunnel-1": { state: "error", message: "cannot listen" } };
    await containers.openPort("h1", web);
    expect(urls).toEqual([]);
    expect(ui.toasts.at(-1)?.text).toContain("cannot listen");
    expect(containers.portTunnels["h1:8080"]).toBeUndefined();
  });
});

describe("remembered state", () => {
  it("remembers which Compose projects are collapsed, per host", () => {
    settings.prefs.containerCollapsed = { [collapseKey("h1", "shop")]: true };
    expect(settings.prefs.containerCollapsed[collapseKey("h1", "shop")]).toBe(true);
    expect(settings.prefs.containerCollapsed[collapseKey("h2", "shop")]).toBeUndefined();
  });
});
