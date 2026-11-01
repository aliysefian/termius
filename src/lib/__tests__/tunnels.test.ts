import { describe, expect, it } from "vitest";
import { bulkMessage, buildRow, canRestart, canStart, canStop, counts, diagnostics, endpoints, filterLogs, formatUptime, mappingText, matches, needsRestart, summary, tunnelStatus, validateTunnel } from "../tunnels";
import type { ForwardLog, ForwardRule, Host } from "../types";

const host = (label: string, hostname = "h.example"): Host => ({ label, hostname, port: 22, group: "", tags: [], notes: "", favorite: false });
type Rule<K extends ForwardRule["kind"]> = Extract<ForwardRule, { kind: K }>;
const local = (label: string, bind_port: number, dest_port = bind_port, host_id = "h1"): Rule<"local"> => ({ label, host_id, auto_start: false, kind: "local", bind_addr: "127.0.0.1", bind_port, dest_host: "127.0.0.1", dest_port });
const remote = (label: string, bind_port: number, host_id = "h1"): Rule<"remote"> => ({ label, host_id, auto_start: false, kind: "remote", bind_addr: "0.0.0.0", bind_port, dest_host: "127.0.0.1", dest_port: 1420 });
const socks = (label: string, bind_port: number): Rule<"dynamic"> => ({ label, host_id: "h1", auto_start: false, kind: "dynamic", bind_addr: "127.0.0.1", bind_port });

describe("tunnel status", () => {
  it("comes from the backend's state, and a pending stop shows as stopping", () => {
    expect(tunnelStatus(undefined)).toBe("stopped");
    expect(tunnelStatus({ state: "stopped" })).toBe("stopped");
    expect(tunnelStatus({ state: "starting" })).toBe("connecting");
    expect(tunnelStatus({ state: "active", port: 1 })).toBe("active");
    expect(tunnelStatus({ state: "error", message: "x" })).toBe("failed");
    expect(tunnelStatus({ state: "active", port: 1 }, "stopping")).toBe("stopping");
  });

  it("a restart is not shown as active until the backend says it is again, and a failure is shown at once", () => {
    expect(tunnelStatus({ state: "active", port: 1 }, "restarting")).toBe("connecting");
    expect(tunnelStatus({ state: "error", message: "bind" }, "restarting")).toBe("failed");
    expect(tunnelStatus({ state: "stopped" }, "starting")).toBe("connecting");
  });

  it("allows only the actions that make sense", () => {
    const row = (st: Parameters<typeof tunnelStatus>[0], hostExists = true, pending?: Parameters<typeof tunnelStatus>[1]) => buildRow("a", local("a", 1), hostExists ? host("c") : undefined, st, pending);
    expect([canStart(row(undefined)), canStop(row(undefined)), canRestart(row(undefined))]).toEqual([true, false, false]);
    expect([canStart(row({ state: "active", port: 1 })), canStop(row({ state: "active", port: 1 })), canRestart(row({ state: "active", port: 1 }))]).toEqual([false, true, true]);
    expect([canStart(row({ state: "starting" })), canStop(row({ state: "starting" }))]).toEqual([false, true]);
    expect([canStart(row({ state: "error", message: "x" })), canRestart(row({ state: "error", message: "x" }))]).toEqual([true, true]);
    expect(canStart(row(undefined, false))).toBe(false);
    expect([canStart(row({ state: "active", port: 1 }, true, "stopping")), canStop(row({ state: "active", port: 1 }, true, "stopping"))]).toEqual([false, false]);
  });
});

describe("how a tunnel reads", () => {
  it("writes the mapping and the endpoints for each type", () => {
    expect(mappingText(local("a", 8000, 80))).toBe("127.0.0.1:8000 → 127.0.0.1:80");
    expect(mappingText(socks("s", 1080))).toBe("127.0.0.1:1080 → SOCKS5");
    expect(mappingText(local("a", 0, 80))).toBe("127.0.0.1:auto → 127.0.0.1:80");
    expect(mappingText({ ...local("a", 0, 80) }, 41234)).toBe("127.0.0.1:41234 → 127.0.0.1:80");
    expect(endpoints(local("a", 8000), "claude").start.label).toBe("Local endpoint");
    expect(endpoints(remote("r", 1420), "claude").start.value).toBe("0.0.0.0:1420 on claude");
    expect(endpoints(remote("r", 1420), "claude").end.label).toBe("Local destination");
    const d = endpoints(socks("s", 1080), "claude");
    expect([d.start.label, d.end.value]).toEqual(["SOCKS5 listener", "chosen by each connecting program"]);
    expect(summary(socks("s", 1080), "claude").lines).toHaveLength(2);
    expect(summary(remote("r", 1420), "claude").title).toBe("Remote forwarding");
  });

  it("formats uptime in its two largest units", () => {
    expect([0, 59_000, 192_000, 7_500_000, 273_600_000].map(formatUptime)).toEqual(["0s", "59s", "3m 12s", "2h 5m", "3d 4h"]);
    expect(formatUptime(-5)).toBe("0s");
  });
});

describe("search and filters work together", () => {
  const rows = [
    buildRow("1", { ...local("backend", 8000), description: "api for the app" }, host("claude", "146.70.44.165"), { state: "active", port: 8000 }),
    buildRow("2", remote("gold", 1420), host("Shahreza"), { state: "stopped" }),
    buildRow("3", local("ui-ran", 8082, 8080), host("Spark"), { state: "error", message: "cannot listen" }),
    buildRow("4", socks("proxy", 1080), host("claude"), { state: "starting" }),
  ];
  const ids = (q: string, t: Parameters<typeof matches>[2] = "all", s: Parameters<typeof matches>[3] = "all") => rows.filter((r) => matches(r, q, t, s)).map((r) => r.id);

  it("finds by name, description, host, address, either port and status", () => {
    expect(ids("backend")).toEqual(["1"]);
    expect(ids("api for")).toEqual(["1"]);
    expect(ids("spark")).toEqual(["3"]);
    expect(ids("146.70")).toEqual(["1"]);
    expect(ids("8080")).toEqual(["3"]);
    expect(ids("1420")).toEqual(["2"]);
    expect(ids("socks")).toEqual(["4"]);
    expect(ids("failed")).toEqual(["3"]);
    expect(ids("claude 8000")).toEqual(["1"]);
    expect(ids("")).toHaveLength(4);
  });

  it("filters by type and by status, together with the search", () => {
    expect(ids("", "local")).toEqual(["1", "3"]);
    expect(ids("", "remote")).toEqual(["2"]);
    expect(ids("", "dynamic")).toEqual(["4"]);
    expect(ids("", "all", "active")).toEqual(["1"]);
    expect(ids("", "all", "stopped")).toEqual(["2"]);
    expect(ids("", "all", "connecting")).toEqual(["4"]);
    expect(ids("", "all", "failed")).toEqual(["3"]);
    expect(ids("claude", "local", "active")).toEqual(["1"]);
    expect(ids("claude", "remote")).toEqual([]);
  });

  it("counts every tunnel, whatever the filter", () => {
    expect(counts(rows)).toEqual({ total: 4, active: 1, stopped: 1, failed: 1, connecting: 1 });
    expect(counts([])).toEqual({ total: 0, active: 0, stopped: 0, failed: 0, connecting: 0 });
  });
});

describe("validation", () => {
  const hosts = new Set(["h1"]);
  const others = [{ id: "x", rule: local("web", 8000) }];

  it("accepts a good tunnel, port 0 and an IPv6 listener", () => {
    expect(validateTunnel(local("a", 5432), hosts, null, [])).toEqual({});
    expect(validateTunnel(local("a", 0, 80), hosts, null, [])).toEqual({});
    expect(validateTunnel({ ...local("a", 5432), bind_addr: "::1" }, hosts, null, [])).toEqual({});
  });

  it("says what is wrong, by field", () => {
    const e = validateTunnel({ ...local(" ", 70000, 0), host_id: "", bind_addr: "not an address", dest_host: "bad host" }, hosts, null, []);
    expect(Object.keys(e).sort()).toEqual(["bind_addr", "bind_port", "dest_host", "dest_port", "host_id", "label"]);
    expect(validateTunnel({ ...local("a", 1), host_id: "gone" }, hosts, null, []).host_id).toContain("no longer exists");
    expect(validateTunnel({ ...local("a", 80), dest_port: 70000 }, hosts, null, []).dest_port).toBeTruthy();
  });

  it("only a remote forward may leave the address empty, and a SOCKS tunnel needs no destination", () => {
    expect(validateTunnel({ ...remote("r", 9000), bind_addr: "" }, hosts, null, []).bind_addr).toBeUndefined();
    expect(validateTunnel({ ...local("l", 9000), bind_addr: "" }, hosts, null, []).bind_addr).toBeTruthy();
    expect(validateTunnel(socks("s", 1080), hosts, null, [])).toEqual({});
  });

  it("refuses a local port another tunnel already wants, but not the tunnel's own, and not other ports", () => {
    expect(validateTunnel(local("b", 8000), hosts, null, others).bind_port).toContain('already used by the tunnel "web"');
    expect(validateTunnel(local("web", 8000), hosts, "x", others)).toEqual({});
    expect(validateTunnel(local("b", 8001), hosts, null, others)).toEqual({});
    expect(validateTunnel({ ...local("b", 8000), bind_addr: "0.0.0.0" }, hosts, null, others).bind_port).toBeTruthy();
    expect(validateTunnel(socks("s", 8000), hosts, null, others).bind_port).toBeTruthy();
    // Remote ports are on the server: the same port on another host is fine.
    expect(validateTunnel(remote("r", 9000, "h1"), hosts, null, [{ id: "y", rule: remote("r0", 9000, "h2") }])).toEqual({});
    expect(validateTunnel(remote("r", 9000, "h1"), hosts, null, [{ id: "y", rule: remote("r0", 9000, "h1") }]).bind_port).toBeTruthy();
  });

  it("knows which edits need a restart", () => {
    const a = local("a", 8000);
    expect(needsRestart(a, { ...a, label: "renamed", description: "x", auto_start: true })).toBe(false);
    expect(needsRestart(a, { ...a, bind_port: 8001 })).toBe(true);
    expect(needsRestart(a, { ...a, host_id: "h2" })).toBe(true);
    expect(needsRestart(a, { ...a, dest_port: 81 })).toBe(true);
    expect(needsRestart(a, socks("a", 8000))).toBe(true);
  });
});

describe("logs and reports", () => {
  const logs: ForwardLog[] = [
    { at_ms: 1_000, level: "info", message: "Starting tunnel" },
    { at_ms: 2_000, level: "warn", message: "Connection from 127.0.0.1:5 failed: refused" },
    { at_ms: 3_000, level: "error", message: "cannot listen on 127.0.0.1:8000" },
  ];

  it("filters by severity and by text", () => {
    expect(filterLogs(logs, "", "all")).toHaveLength(3);
    expect(filterLogs(logs, "", "error")).toHaveLength(1);
    expect(filterLogs(logs, "REFUSED", "all")).toHaveLength(1);
    expect(filterLogs(logs, "listen", "warn")).toHaveLength(0);
  });

  it("writes diagnostics that name the tunnel, its state and every line", () => {
    const row = buildRow("1", local("backend", 8000), host("claude"), { state: "error", message: "cannot listen" });
    const text = diagnostics(row, logs);
    expect(text).toContain("Tunnel: backend");
    expect(text).toContain("Status: Failed (cannot listen)");
    expect(text).toContain("SSH host: claude");
    expect(text.split("\n").filter((l) => l.includes("ERROR"))).toHaveLength(1);
    expect(diagnostics(row, [])).toContain("(no log lines)");
  });

  it("reports a partial failure as one, not as a success", () => {
    expect(bulkMessage("Started", 3, [])).toEqual({ kind: "info", text: "Started 3 tunnels." });
    expect(bulkMessage("Stopped", 1, [])).toEqual({ kind: "info", text: "Stopped 1 tunnel." });
    const m = bulkMessage("Started", 5, [{ label: "a", error: "no host" }, { label: "b", error: "x" }]);
    expect(m.kind).toBe("error");
    expect(m.text).toContain("Started 3 of 5");
    expect(m.text).toContain("a (no host)");
  });
});

import { draftFromRule, newDraft, ruleFromDraft } from "../tunnels";

describe("the form's draft", () => {
  it("round-trips a rule of every type, keeping the description", () => {
    for (const r of [{ ...local("a", 8000, 80), description: "api" }, remote("r", 9000), socks("s", 1080)] as ForwardRule[]) {
      expect(ruleFromDraft(draftFromRule(r))).toEqual(r);
    }
  });

  it("keeps what was typed for another type when the type is switched back and forth", () => {
    const d = { ...newDraft("h1"), dest_host: "db.internal", dest_port: 5432 };
    const asSocks = ruleFromDraft({ ...d, kind: "dynamic" });
    expect(asSocks).not.toHaveProperty("dest_host");
    expect(ruleFromDraft({ ...d, kind: "remote" })).toMatchObject({ kind: "remote", dest_host: "db.internal", dest_port: 5432 });
  });

  it("trims text, turns typed ports into numbers, and leaves out an empty description", () => {
    const r = ruleFromDraft({ ...newDraft("h1"), label: "  web ", bind_port: "8001" as unknown as number, description: "  " });
    expect(r).toMatchObject({ label: "web", bind_port: 8001 });
    expect(r).not.toHaveProperty("description");
  });

  it("starts a SOCKS tunnel on the usual port", () => {
    expect(newDraft("h1", "dynamic").bind_port).toBe(1080);
    expect(newDraft("h1", "local").bind_addr).toBe("127.0.0.1");
  });
});
