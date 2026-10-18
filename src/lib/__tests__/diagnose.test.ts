import { describe, expect, it } from "vitest";
import { applyEvent, barWidths, emptyReport, formatAvg, formatLoss, lossClass, progress, verdictHeading, verdictTone } from "$lib/diagnose";
import type { DiagEvent, VerdictKind } from "$lib/types";

const dns = { name: "h", literal: false, addresses: [{ ip: "203.0.113.5", record: "A" as const, ptr: null }], resolve_ms: 3, error: null };
const tcp = { addr: "h:22", connect_ms: 12, failure: null, error: null };
const ssh = { banner: "SSH-2.0-OpenSSH_9.6", banner_ms: 9, kex_ms: 40, algorithm: "ssh-ed25519", fingerprint: "SHA256:x", error: null };
const route = { tool: "mtr", hops: [], reached: true, error: null };

describe("applyEvent", () => {
  it("folds each stage into its own slot, leaving the others alone", () => {
    let r = emptyReport();
    const events: DiagEvent[] = [
      { stage: "dns", report: dns },
      { stage: "tcp", report: tcp },
      { stage: "ssh", report: ssh },
    ];
    for (const e of events) r = applyEvent(r, e);
    expect(r.dns).toBe(dns);
    expect(r.tcp).toBe(tcp);
    expect(r.ssh).toBe(ssh);
    expect(r.route).toBeNull();
    expect(r.verdict).toBeNull();
  });

  it("marks the route as running until it reports", () => {
    let r = applyEvent(emptyReport(), { stage: "route_started" });
    expect(r.routeRunning).toBe(true);
    r = applyEvent(r, { stage: "route", report: route });
    expect(r.routeRunning).toBe(false);
    expect(r.route).toBe(route);
  });

  it("stops everything running when the verdict arrives", () => {
    const r = applyEvent(applyEvent(emptyReport(), { stage: "route_started" }), { stage: "verdict", verdict: { kind: "healthy", title: "Healthy", detail: "" } });
    expect(r.routeRunning).toBe(false);
    expect(r.verdict?.kind).toBe("healthy");
  });

  it("does not change the report it was given", () => {
    const r = emptyReport();
    applyEvent(r, { stage: "dns", report: dns });
    expect(r.dns).toBeNull();
  });
});

describe("verdicts", () => {
  it("are named as the brief names them", () => {
    expect(verdictHeading("local_network")).toBe("Local Network / ISP Drop");
    expect(verdictHeading("intermediate_hop")).toBe("Intermediate Hop / Firewall Timeout");
    expect(verdictHeading("bastion_unreachable")).toBe("Bastion Unreachable");
    expect(verdictHeading("target_port_closed")).toBe("Target Port Closed / SSH Daemon Down");
    expect(verdictHeading("auth_rejected")).toBe("Authentication / Key Rejected");
  });

  it("every kind has a heading and a tone", () => {
    const kinds: VerdictKind[] = ["healthy", "name_not_resolved", "local_network", "intermediate_hop", "bastion_unreachable", "target_port_closed", "auth_rejected", "host_key_problem", "unknown"];
    for (const k of kinds) {
      expect(verdictHeading(k)).toBeTruthy();
      expect(["ok", "warn", "bad"]).toContain(verdictTone(k));
    }
    expect(verdictTone("healthy")).toBe("ok");
    expect(verdictTone("host_key_problem")).toBe("warn");
    expect(verdictTone("local_network")).toBe("bad");
  });
});

describe("numbers", () => {
  it("scales two timings on one scale", () => {
    expect(barWidths(10, 40)).toEqual([25, 100]);
    expect(barWidths(40, 40)).toEqual([100, 100]);
    expect(barWidths(1, 400)).toEqual([3, 100]);
    expect(barWidths(null, 40)).toEqual([0, 100]);
    expect(barWidths(null, null)).toEqual([0, 0]);
    expect(barWidths(0, 0)).toEqual([0, 0]);
  });

  it("colours loss only when there is some", () => {
    expect(lossClass(0)).toBe("text-fg-muted");
    expect(lossClass(null)).toBe("text-fg-muted");
    expect(lossClass(33)).toBe("text-warning");
    expect(lossClass(100)).toBe("text-danger");
  });

  it("formats loss and averages", () => {
    expect(formatLoss(33.3)).toBe("33%");
    expect(formatLoss(null)).toBe("–");
    expect(formatAvg(0.6)).toBe("0.6");
    expect(formatAvg(12.54)).toBe("12.5");
    expect(formatAvg(128.4)).toBe("128");
    expect(formatAvg(null)).toBe("–");
  });

  it("counts the stages that apply", () => {
    expect(progress(emptyReport())).toEqual({ done: 0, total: 5 });
    const failed = { ...emptyReport(), dns, tcp: { ...tcp, connect_ms: null, failure: "timeout" as const } };
    expect(progress(failed)).toEqual({ done: 2, total: 4 });
    const full = { ...emptyReport(), dns, tcp, ssh, route, verdict: { kind: "healthy" as const, title: "", detail: "" } };
    expect(progress(full)).toEqual({ done: 5, total: 5 });
    const noRoute = { ...emptyReport(), dns, tcp, ssh, verdict: { kind: "healthy" as const, title: "", detail: "" } };
    expect(progress(noRoute)).toEqual({ done: 4, total: 4 });
  });
});
