import { describe, expect, it } from "vitest";
import { buildRow, checkLines, healthOf, matchesRow, sortRows, summarize } from "../monitoringview";
import type { MonitorTarget, StateView, TargetStatus } from "../monitoring";
import type { NetInterface } from "../netdata";

const target = (label: string, extra: Partial<MonitorTarget> = {}): MonitorTarget => ({ label, address: `${label}.example`, tags: [], enabled: true, interval_secs: 60, checks: [], ssh_host_id: null, notes: "", ...extra });
const rec = (id: string, t: MonitorTarget) => ({ id, rev: 1, updated_at: 1, deleted: false, device_id: "d", data: t });
const state = (status: StateView["status"]): StateView => ({ rule_id: "r", rule: "CPU", subject: "", status, value: 1, since: 1 });
const status = (extra: Partial<TargetStatus> = {}): TargetStatus => ({ id: "x", checked_at: 1, ping: null, ports: [], web: [], metrics: null, states: [], errors: [], ...extra });
const nic = (name: string, rx: number | null, tx: number | null): NetInterface => ({ name, state: "up", physical: true, link_mbps: 1000, rx_bps: rx, tx_bps: tx, rx_pps: 0, tx_pps: 0, rx_total: 1e9, tx_total: 2e9, rx_errors: 0, tx_errors: 0, rx_drops: 0, tx_drops: 0 });
const metrics = (extra = {}) => ({ cpu: 10, mem: 20, disk: 30, net: null, net_headline: null, ...extra });

describe("how a host's checks add up", () => {
  const t = target("a");

  it("is offline when it can't be reached at all", () => {
    expect(healthOf(t, status({ ping: { loss_pct: 100, avg_ms: null, error: null } }))).toBe("offline");
    expect(healthOf(t, status({ ping: { loss_pct: 0, avg_ms: null, error: "ping not found" } }))).toBe("offline");
    expect(healthOf(t, status({ ports: [{ port: 22, open: false, latency_ms: null, error: null }, { port: 80, open: false, latency_ms: null, error: null }] }))).toBe("offline");
    // One port open means the host answers.
    expect(healthOf(t, status({ ports: [{ port: 22, open: true, latency_ms: 3, error: null }, { port: 80, open: false, latency_ms: null, error: null }] }))).toBe("healthy");
  });

  it("takes warning and critical from the rules, the worst one winning", () => {
    const ping = { loss_pct: 0, avg_ms: 5, error: null };
    expect(healthOf(t, status({ ping, states: [state("ok"), state("warning")] }))).toBe("warning");
    expect(healthOf(t, status({ ping, states: [state("warning"), state("critical"), state("ok")] }))).toBe("critical");
    expect(healthOf(t, status({ ping, states: [state("ok")] }))).toBe("healthy");
  });

  it("calls a failing web check critical even without a rule, and never calls an unchecked host healthy", () => {
    expect(healthOf(t, status({ web: [{ url: "https://a", status: 500, ok: false, time_ms: 9, cert_days: null, error: null }] }))).toBe("critical");
    expect(healthOf(t, status())).toBe("waiting");
    expect(healthOf(t, undefined)).toBe("waiting");
    expect(healthOf(target("p", { enabled: false }), status({ ping: { loss_pct: 100, avg_ms: null, error: null } }))).toBe("paused");
  });

  it("counts each kind, with paused and waiting hosts in the total only", () => {
    const rows = [
      buildRow(rec("1", t), status({ ping: { loss_pct: 0, avg_ms: 1, error: null } })),
      buildRow(rec("2", t), status({ ping: { loss_pct: 0, avg_ms: 1, error: null }, states: [state("warning")] })),
      buildRow(rec("3", t), status({ ping: { loss_pct: 0, avg_ms: 1, error: null }, states: [state("critical")] })),
      buildRow(rec("4", t), status({ ping: { loss_pct: 100, avg_ms: null, error: null } })),
      buildRow(rec("5", target("p", { enabled: false })), undefined),
      buildRow(rec("6", t), undefined),
    ];
    expect(summarize(rows)).toEqual({ total: 6, healthy: 1, warning: 1, critical: 1, offline: 1 });
    expect(summarize([])).toEqual({ total: 0, healthy: 0, warning: 0, critical: 0, offline: 0 });
  });
});

describe("the host table", () => {
  const rows = [
    buildRow(rec("1", target("web-1", { tags: ["prod"], notes: "frontend" })), status({ ping: { loss_pct: 0, avg_ms: 1, error: null }, metrics: metrics({ cpu: 80, net: { interfaces: [nic("eth0", 5_000_000, 100)], primary: "eth0", window_secs: 1 }, net_headline: "eth0" }) })),
    buildRow(rec("2", target("db-1", { tags: ["prod", "db"] })), status({ ping: { loss_pct: 0, avg_ms: 1, error: null }, metrics: metrics({ cpu: 20, net: { interfaces: [nic("ens18", 900, 8_000_000)], primary: "ens18", window_secs: 1 }, net_headline: "ens18" }), states: [state("warning")] })),
    buildRow(rec("3", target("router")), status({ ping: { loss_pct: 100, avg_ms: null, error: null } })),
  ];
  const names = (list: typeof rows) => list.map((r) => r.target.label);

  it("reads the headline interface's speed into the row, and nothing when there is none", () => {
    expect(rows[0].net).toEqual({ name: "eth0", rx: 5_000_000, tx: 100 });
    expect(rows[1].net?.name).toBe("ens18");
    expect(rows[2].net).toBeNull();
    // An interface named as headline that isn't in the reading is not guessed at.
    expect(buildRow(rec("9", target("x")), status({ metrics: metrics({ net: { interfaces: [nic("eth0", 1, 1)], primary: null, window_secs: 1 }, net_headline: "zzz" }) })).net).toBeNull();
  });

  it("searches name, address, tags, notes and health together with the filters", () => {
    const m = (q: string, h: Parameters<typeof matchesRow>[2] = "all", tag = "") => names(rows.filter((r) => matchesRow(r, q, h, tag)));
    expect(m("web")).toEqual(["web-1"]);
    expect(m("frontend")).toEqual(["web-1"]);
    expect(m("db-1.example")).toEqual(["db-1"]);
    expect(m("offline")).toEqual(["router"]);
    expect(m("", "warning")).toEqual(["db-1"]);
    expect(m("", "all", "prod")).toEqual(["web-1", "db-1"]);
    expect(m("prod", "healthy")).toEqual(["web-1"]);
    expect(m("zzz")).toEqual([]);
  });

  it("sorts by name, by how bad it is, and by speed with missing values last either way", () => {
    expect(names(sortRows(rows, "name", "asc"))).toEqual(["db-1", "router", "web-1"]);
    expect(names(sortRows(rows, "health", "asc"))).toEqual(["router", "db-1", "web-1"]);
    expect(names(sortRows(rows, "rx", "desc"))).toEqual(["web-1", "db-1", "router"]);
    expect(names(sortRows(rows, "rx", "asc"))).toEqual(["db-1", "web-1", "router"]);
    expect(names(sortRows(rows, "tx", "desc"))).toEqual(["db-1", "web-1", "router"]);
    expect(names(sortRows(rows, "cpu", "desc"))).toEqual(["web-1", "db-1", "router"]);
  });
});

describe("the checks of one host", () => {
  it("lists each check with how it went, and says when SSH readings failed", () => {
    const lines = checkLines(
      status({
        ping: { loss_pct: 25, avg_ms: 12.34, error: null },
        ports: [{ port: 22, open: true, latency_ms: 4, error: null }, { port: 5432, open: false, latency_ms: null, error: "connection refused" }],
        web: [{ url: "https://a.example/health", status: 200, ok: true, time_ms: 80, cert_days: 30, error: null }],
        metrics: metrics(),
      }),
    );
    expect(lines.map((l) => [l.name, l.ok])).toEqual([["Ping", true], ["Port 22", true], ["Port 5432", false], ["https://a.example/health", true], ["SSH readings", true]]);
    expect(lines[0].detail).toBe("25% lost, 12.3 ms");
    expect(lines[2].detail).toBe("connection refused");
    const failed = checkLines(status({ errors: ["SSH: the host did not answer in time"] }));
    expect(failed).toEqual([{ kind: "ssh", name: "SSH readings", ok: false, detail: "the host did not answer in time" }]);
    expect(checkLines(undefined)).toEqual([]);
  });
});
