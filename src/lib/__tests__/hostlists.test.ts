import { describe, expect, it } from "vitest";
import { addressText, agoText, failuresBySource, filterPorts, matchesProcess, newestFirst, processRows, scopeText, signalQuestion, signalResult, sortPorts, sortProcesses, tunnelFor } from "../hostlists";
import type { AuthEvent, ListeningPort, LoginEntry, ProcessInfo } from "../hostsample";

const proc = (pid: number, name: string, over: Partial<ProcessInfo> = {}): ProcessInfo => ({ pid, ppid: 1, user: "root", state: "S", name, command: name, cpu_pct: 0, rss: 1000, mem_pct: 0, threads: 1, start_ticks: pid, age_secs: 10, ...over });
const port = (p: number, over: Partial<ListeningPort> = {}): ListeningPort => ({ proto: "tcp", address: "0.0.0.0", port: p, ipv6: false, pid: null, process: null, user: null, state: "LISTEN", local_only: false, ...over });

describe("searching and sorting processes", () => {
  const list = [proc(10, "nginx", { user: "www-data", command: "nginx: worker process", cpu_pct: 6.1, rss: 5000 }), proc(20, "postgres", { cpu_pct: 3.9, rss: 9000 }), proc(30, "idle", { cpu_pct: null }), proc(40, "zzz", { cpu_pct: 0 })];

  it("matches every word against name, command, user and pid", () => {
    expect(matchesProcess(list[0], "nginx worker")).toBe(true);
    expect(matchesProcess(list[0], "www-data")).toBe(true);
    expect(matchesProcess(list[0], "10")).toBe(true);
    expect(matchesProcess(list[0], "nginx postgres")).toBe(false);
    expect(matchesProcess(list[0], "  ")).toBe(true);
  });

  it("puts a process with no figure last whichever way it is sorted, never among the zeros", () => {
    expect(sortProcesses(list, "cpu", "desc").map((p) => p.pid)).toEqual([10, 20, 40, 30]);
    expect(sortProcesses(list, "cpu", "asc").map((p) => p.pid)).toEqual([40, 20, 10, 30]);
    expect(sortProcesses(list, "name", "asc").map((p) => p.name)).toEqual(["idle", "nginx", "postgres", "zzz"]);
    expect(sortProcesses(list, "mem", "desc")[0].pid).toBe(20);
  });
});

describe("the process tree", () => {
  const list = [proc(1, "init", { ppid: 0 }), proc(100, "sshd", { ppid: 1, cpu_pct: 1 }), proc(200, "sshd-session", { ppid: 100, cpu_pct: 5 }), proc(300, "bash", { ppid: 200, cpu_pct: 0 }), proc(150, "cron", { ppid: 1, cpu_pct: 9 }), proc(900, "orphan", { ppid: 777, cpu_pct: 2 })];
  const rows = (query = "") => processRows(list, { query, sort: "cpu", dir: "desc", tree: true }).map((r) => `${"  ".repeat(r.depth)}${r.p.name}`);

  it("nests each process under its parent, siblings in sort order, and a parentless one starts its own branch", () => {
    expect(rows()).toEqual(["orphan", "init", "  cron", "  sshd", "    sshd-session", "      bash"]);
  });

  it("shows a match with the parents above it, and nothing else", () => {
    expect(rows("bash")).toEqual(["init", "  sshd", "    sshd-session", "      bash"]);
    expect(rows("nothing")).toEqual([]);
  });

  it("marks the rows that have children, and is flat when asked", () => {
    expect(processRows(list, { query: "", sort: "pid", dir: "asc", tree: true }).filter((r) => r.parent).map((r) => r.p.pid)).toEqual([1, 100, 200]);
    const flat = processRows(list, { query: "sshd", sort: "pid", dir: "asc", tree: false });
    expect(flat.map((r) => r.p.pid)).toEqual([100, 200]);
    expect(flat.every((r) => r.depth === 0)).toBe(true);
  });

  it("survives a loop in the parent links", () => {
    const loop = [proc(1, "a", { ppid: 2 }), proc(2, "b", { ppid: 1 })];
    expect(processRows(loop, { query: "", sort: "pid", dir: "asc", tree: true }).length).toBeLessThanOrEqual(2);
    expect(processRows([proc(5, "self", { ppid: 5 })], { query: "", sort: "pid", dir: "asc", tree: true })).toHaveLength(1);
  });
});

describe("signals", () => {
  const p = proc(812, "sshd", { user: "root" });

  it("says who, where and what, including the identity check", () => {
    const q = signalQuestion(p, "claude", "KILL");
    expect(q).toContain("sshd (process 812, user root) on claude");
    expect(q).toContain("SIGKILL");
    expect(q).toContain("SSH server process");
    expect(q).toContain("still the same program");
    expect(signalQuestion(proc(5, "nginx"), "web", "TERM")).not.toContain("SSH server");
  });

  it("reports every outcome in words, and an unknown one is not retried", () => {
    expect(signalResult({ kind: "sent" }, p, "TERM")).toEqual({ kind: "info", text: "Sent SIGTERM to sshd (812)." });
    expect(signalResult({ kind: "gone" }, p, "TERM").kind).toBe("info");
    expect(signalResult({ kind: "changed" }, p, "TERM").text).toMatch(/reused.*Nothing was sent/);
    expect(signalResult({ kind: "refused", reason: "Operation not permitted" }, p, "KILL").text).toMatch(/did not allow it: Operation not permitted/);
    const u = signalResult({ kind: "unknown" }, p, "TERM");
    expect(u.kind).toBe("error");
    expect(u.text).toMatch(/not sent again/);
  });
});

describe("listening ports", () => {
  const list = [port(22, { process: "sshd", pid: 812 }), port(5432, { address: "127.0.0.1", local_only: true, process: "postgres" }), port(53, { proto: "udp", address: "127.0.0.53", local_only: true }), port(443, { address: "::", ipv6: true, process: "nginx" })];

  it("filters by search, protocol, scope and process", () => {
    const f = { query: "", proto: "all" as const, scope: "all" as const, process: "" };
    expect(filterPorts(list, f)).toHaveLength(4);
    expect(filterPorts(list, { ...f, query: "post" }).map((p) => p.port)).toEqual([5432]);
    expect(filterPorts(list, { ...f, proto: "udp" }).map((p) => p.port)).toEqual([53]);
    expect(filterPorts(list, { ...f, scope: "local" }).map((p) => p.port)).toEqual([5432, 53]);
    expect(filterPorts(list, { ...f, scope: "public" }).map((p) => p.port)).toEqual([22, 443]);
    expect(filterPorts(list, { ...f, process: "nginx" }).map((p) => p.port)).toEqual([443]);
  });

  it("sorts, with a listener whose owner is unknown last by process", () => {
    expect(sortPorts(list, "port", "asc").map((p) => p.port)).toEqual([22, 53, 443, 5432]);
    expect(sortPorts(list, "process", "asc").map((p) => p.port)).toEqual([443, 5432, 22, 53]);
  });

  it("writes an address and says what its scope means without promising reachability", () => {
    expect(addressText(list[0])).toBe(":22");
    expect(addressText(list[1])).toBe("127.0.0.1:5432");
    expect(addressText(port(80, { address: "::1", ipv6: true, local_only: true }))).toBe("[::1]:80");
    expect(scopeText(list[1])).toMatch(/This host only/);
    expect(scopeText(list[0])).toMatch(/wherever the firewall and network allow/);
    expect(scopeText(port(80, { address: "10.0.0.5" }))).toBe("Bound to 10.0.0.5");
  });
});

describe("a tunnel from a listener", () => {
  it("is a local forward to the host's loopback, or to the one address it is bound to", () => {
    expect(tunnelFor(port(5432, { address: "127.0.0.1", local_only: true, process: "postgres" }), "h1", "claude")).toMatchObject({ kind: "local", host_id: "h1", label: "claude :5432 (postgres)", bind_addr: "127.0.0.1", bind_port: 5432, dest_host: "127.0.0.1", dest_port: 5432, auto_start: false });
    expect(tunnelFor(port(80), "h1", "claude")?.dest_host).toBe("127.0.0.1");
    expect(tunnelFor(port(8080, { address: "10.0.0.5" }), "h1", "claude")?.dest_host).toBe("10.0.0.5");
    expect(tunnelFor(port(53, { proto: "udp" }), "h1", "claude")).toBeNull();
  });
});

describe("logins", () => {
  const ev = (kind: AuthEvent["kind"], from: string | null, user: string | null): AuthEvent => ({ kind, from, user, method: null, time: "t" });

  it("counts failures by source as facts, most first, and ignores successes", () => {
    const r = failuresBySource([ev("failed", "1.1.1.1", "root"), ev("invalid_user", "1.1.1.1", "admin"), ev("accepted", "1.1.1.1", "ali"), ev("failed", "2.2.2.2", "bob"), ev("failed", null, null)]);
    expect(r).toEqual([{ from: "1.1.1.1", count: 2, users: ["root", "admin"] }, { from: "2.2.2.2", count: 1, users: ["bob"] }, { from: "unknown address", count: 1, users: [] }]);
  });

  it("says how long ago, on the host's clock", () => {
    const now = 10_000_000_000;
    expect(agoText(null, now)).toBeNull();
    expect(agoText(now - 30_000, now)).toBe("just now");
    expect(agoText(now - 2 * 60_000, now)).toBe("2m ago");
    expect(agoText(now - (82 * 60_000), now)).toBe("1h 22m ago");
    expect(agoText(now - 3 * 86_400_000, now)).toBe("3d ago");
    expect(agoText(now + 5000, now)).toBe("just now");
  });

  it("orders newest first with the untimed ones last", () => {
    const e = (ms: number | null): LoginEntry => ({ user: "u", tty: "t", from: null, started_ms: ms, started: "", ended: "" });
    expect(newestFirst([e(1), e(null), e(3)]).map((x) => x.started_ms)).toEqual([3, 1, null]);
  });
});
