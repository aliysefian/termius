import { describe, expect, it } from "vitest";
import type { Proxy, VaultRecord } from "../types";
import {
  PRESETS,
  ago,
  botProxyChoices,
  checkRule,
  checkTarget,
  describeDeliveries,
  describeLimits,
  describeScope,
  emptyChannels,
  emptyTarget,
  formatValue,
  hostOf,
  parsePorts,
  ruleFromPreset,
  splitList,
  validAddress,
  validUrl,
  worst,
} from "../monitoring";

describe("a host to watch", () => {
  it("starts with ping and the SSH port, and is ready to save once it has a name and address", () => {
    const t = emptyTarget();
    expect(t.checks.map((c) => c.kind)).toEqual(["ping", "tcp"]);
    expect(checkTarget(t)).toMatch(/name/);
    t.label = "web";
    expect(checkTarget(t)).toMatch(/address/);
    t.address = "10.0.0.5";
    expect(checkTarget(t)).toBeNull();
  });

  it("refuses what the backend would: bad addresses, a short interval, no checks, a login check with no host", () => {
    const t = { ...emptyTarget(), label: "web", address: "10.0.0.5" };
    expect(checkTarget({ ...t, address: "-oProxyCommand=x" })).toMatch(/address/);
    expect(checkTarget({ ...t, address: "a b" })).toMatch(/address/);
    expect(checkTarget({ ...t, interval_secs: 5 })).toMatch(/10 seconds/);
    expect(checkTarget({ ...t, checks: [] })).toMatch(/at least one/);
    expect(checkTarget({ ...t, checks: [{ kind: "tcp", port: 0 }] })).toMatch(/Ports/);
    expect(checkTarget({ ...t, checks: [{ kind: "ping", count: 99, timeout_ms: 1000 }] })).toMatch(/Ping/);
    expect(checkTarget({ ...t, checks: [{ kind: "ssh_metrics" }] })).toMatch(/SSH host/);
    expect(checkTarget({ ...t, checks: [{ kind: "ssh_metrics" }], ssh_host_id: "abc" })).toBeNull();
    expect(checkTarget({ ...t, checks: [{ kind: "ssh_metrics" }], ssh_host_id: "abc", interval_secs: 20 })).toMatch(/30 seconds/);
    expect(checkTarget({ ...t, checks: Array.from({ length: 21 }, (_, i) => ({ kind: "tcp" as const, port: i + 1 })) })).toMatch(/too many/);
  });

  it("tells names and addresses from things that look like options", () => {
    for (const ok of ["10.0.0.5", "web-01.example.com", "fe80::1%eth0", "my_host"]) expect(validAddress(ok), ok).toBe(true);
    for (const bad of ["", "-c", "a b", "a;b", "a$(x)"]) expect(validAddress(bad), bad).toBe(false);
  });

  it("reads lists of tags and ports", () => {
    expect(splitList(" a, b ;c,,")).toEqual(["a", "b", "c"]);
    expect(parsePorts("22, 443 ,22")).toEqual([22, 443]);
    expect(parsePorts("")).toEqual([]);
    expect(parsePorts("22, http")).toBeNull();
    expect(parsePorts("0")).toBeNull();
    expect(parsePorts("70000")).toBeNull();
  });
});

describe("a website to watch", () => {
  const site = (over: Partial<Extract<import("../monitoring").CheckSpec, { kind: "http" }>> = {}) => ({
    ...emptyTarget(),
    label: "shop",
    address: "example.com",
    checks: [{ kind: "http" as const, url: "https://example.com/", expect_status: 0, timeout_ms: 10000, contains: null, ...over }],
  });

  it("accepts http and https addresses and refuses the rest", () => {
    for (const ok of ["https://example.com", "http://10.0.0.5:8080/health?x=1", "https://[::1]:8443/", "HTTPS://Example.com/"]) expect(validUrl(ok), ok).toBe(true);
    for (const bad of ["", "example.com", "ftp://example.com", "https://", "https://user:pw@example.com/", "http://a b/", "http://host:0/", "http://host:99999/", "http://-oops/", "javascript:alert(1)"]) expect(validUrl(bad), bad).toBe(false);
  });

  it("takes a host's address from its website", () => {
    expect(hostOf("https://shop.example.com:8443/a?b=1")).toBe("shop.example.com");
    expect(hostOf("http://[::1]:80/")).toBe("::1");
    expect(hostOf("nonsense")).toBe("");
  });

  it("is checked the way the backend checks it", () => {
    expect(checkTarget(site())).toBeNull();
    expect(checkTarget(site({ url: "example.com" }))).toMatch(/http:\/\//);
    expect(checkTarget(site({ url: "https://user:pw@example.com/" }))).toMatch(/user name/);
    expect(checkTarget(site({ expect_status: 99 }))).toMatch(/100 to 599/);
    expect(checkTarget(site({ expect_status: 204 }))).toBeNull();
    expect(checkTarget(site({ timeout_ms: 100 }))).toMatch(/500 to 30000/);
    expect(checkTarget(site({ contains: "x".repeat(201) }))).toMatch(/200 characters/);
    const web = (url: string) => ({ kind: "http" as const, url, expect_status: 0, timeout_ms: 10000, contains: null });
    const many = (n: number) => ({ ...site(), checks: Array.from({ length: n }, (_, i) => web(`https://example.com/${i}`)) });
    expect(checkTarget(many(5))).toBeNull();
    expect(checkTarget(many(6))).toMatch(/at most 5/);
    expect(checkTarget({ ...site(), checks: [web("https://example.com/a?x=1"), web("https://example.com/a?x=2")] })).toMatch(/the same/);
  });

  it("has rules for it: down, slow, and the certificate running out", () => {
    const down = ruleFromPreset("Site down");
    expect([down.metric, down.crit, down.op]).toEqual(["http_down", 0, "gt"]);
    const cert = ruleFromPreset("Certificate expiring");
    expect([cert.metric, cert.op, cert.warn, cert.crit]).toEqual(["cert_days", "lt", 21, 7]);
    expect(describeLimits(cert)).toBe("< 21 days warning, < 7 days critical");
    expect(describeLimits(down)).toMatch(/isn't answering/);
    expect(formatValue("http_down", 1)).toBe("down");
    expect(formatValue("http_down", 0)).toBe("up");
    expect(formatValue("cert_days", 12)).toBe("12 days");
    expect(formatValue("http_time", 340.2)).toBe("340.2 ms");
  });
});

describe("an alert rule", () => {
  it("each preset is a valid rule", () => {
    for (const p of PRESETS) expect(checkRule(ruleFromPreset(p.name)), p.name).toBeNull();
  });

  it("needs a limit, in the right order, and a sensible number of checks in a row", () => {
    const r = ruleFromPreset("CPU use");
    expect(checkRule({ ...r, name: " " })).toMatch(/name/);
    expect(checkRule({ ...r, warn: null, crit: null })).toMatch(/limit/);
    expect(checkRule({ ...r, warn: 95, crit: 80 })).toMatch(/before the critical/);
    expect(checkRule({ ...r, op: "lt", warn: 10, crit: 20 })).toMatch(/before the critical/);
    expect(checkRule({ ...r, op: "lt", warn: 20, crit: 10 })).toBeNull();
    expect(checkRule({ ...r, attempts: 0 })).toMatch(/1 to 20/);
    expect(checkRule({ ...r, scope: { type: "tags", tags: [" "] } })).toMatch(/tag/);
    expect(checkRule({ ...r, warn: null })).toBeNull();
  });

  it("is described in words", () => {
    expect(describeLimits(ruleFromPreset("CPU use"))).toBe("> 80% warning, > 90% critical");
    expect(describeLimits(ruleFromPreset("Ping latency"))).toBe("> 200 ms warning, > 500 ms critical");
    expect(describeLimits({ ...ruleFromPreset("CPU use"), warn: null, op: "lt" })).toBe("< 90% critical");
    expect(describeLimits(ruleFromPreset("Port closed"))).toMatch(/port/);
    const name = (id: string) => `host ${id}`;
    expect(describeScope({ type: "all" }, name)).toBe("all hosts");
    expect(describeScope({ type: "tags", tags: ["prod", "db"] }, name)).toBe("hosts tagged prod, db");
    expect(describeScope({ type: "target", id: "7" }, name)).toBe("host 7");
  });
});

describe("what the status page shows", () => {
  it("picks the most serious state, and nothing when there are none", () => {
    expect(worst([])).toBeNull();
    expect(worst([{ status: "ok" }, { status: "warning" }])).toBe("warning");
    expect(worst([{ status: "warning" }, { status: "critical" }, { status: "ok" }])).toBe("critical");
    expect(worst([{ status: "ok" }, { status: "unknown" }])).toBe("unknown");
  });

  it("writes readings", () => {
    expect(formatValue("cpu", 12)).toBe("12%");
    expect(formatValue("cpu", 12.34)).toBe("12.3%");
    expect(formatValue("ping_latency", 0.4)).toBe("0.4 ms");
    expect(formatValue("port_closed", 1)).toBe("closed");
    expect(formatValue("port_closed", 0)).toBe("open");
    expect(formatValue("disk", null)).toBe("–");
  });

  it("says how long ago", () => {
    const now = 1_000_000_000;
    expect(ago(now / 1000 - 5, now)).toBe("5 s ago");
    expect(ago(now / 1000 - 300, now)).toBe("5 min ago");
    expect(ago(now / 1000 - 7300, now)).toBe("2 h ago");
    expect(ago(now / 1000 - 200000, now)).toBe("2 d ago");
    expect(ago(now / 1000 + 50, now)).toBe("0 s ago");
  });

  it("offers only the saved proxies an alert bot can use", () => {
    const rec = (id: string, data: Proxy | undefined): VaultRecord<Proxy> => ({ id, rev: 1, updated_at: 0, deleted: !data, device_id: "d", data });
    const list = [
      rec("c", { name: "Jump script", spec: { kind: "command", command: "nc %h %p", approved: true } }),
      rec("h", { name: "Office", spec: { kind: "http", host: "10.0.0.2", port: 3128 } }),
      rec("gone", undefined),
      rec("s", { name: "Home VPN", spec: { kind: "socks5", host: "127.0.0.1", port: 1080, username: "me" } }),
    ];
    expect(botProxyChoices(list)).toEqual([
      { id: "s", label: "Home VPN (SOCKS5 127.0.0.1:1080)" },
      { id: "h", label: "Office (HTTP 10.0.0.2:3128)" },
    ]);
    expect(botProxyChoices([])).toEqual([]);
    expect(emptyChannels().telegram.proxy_id).toBeNull();
    expect(emptyChannels().bale.proxy_id).toBeNull();
  });

  it("says who got an alert and who did not", () => {
    expect(describeDeliveries([])).toMatch(/no channel/);
    expect(describeDeliveries([{ channel: "telegram", error: null }, { channel: "email", error: null }])).toBe("Sent to Telegram, Email");
    expect(describeDeliveries([{ channel: "telegram", error: null }, { channel: "bale", error: "no answer" }])).toBe("Sent to Telegram. Failed: Bale (no answer)");
    expect(describeDeliveries([{ channel: "email", error: "Email: the server answered 535" }])).toBe("Failed: Email (the server answered 535)");
  });
});
