// The state behind the diagnostics drawer, as pure functions: how stage results fold into one report, what each
// verdict is called, how the timing bars scale, and when a hop's loss is worth a colour.

import type { DiagAuth, DiagDns, DiagEvent, DiagRoute, DiagSsh, DiagTcp, DiagVerdict, VerdictKind } from "$lib/types";

export interface DiagReport {
  dns: DiagDns | null;
  tcp: DiagTcp | null;
  ssh: DiagSsh | null;
  route: DiagRoute | null;
  /** The route trace has been asked for and has not answered yet. */
  routeRunning: boolean;
  auth: DiagAuth | null;
  verdict: DiagVerdict | null;
}

export const emptyReport = (): DiagReport => ({ dns: null, tcp: null, ssh: null, route: null, routeRunning: false, auth: null, verdict: null });

/** Fold one stage's result into the report. */
export function applyEvent(r: DiagReport, e: DiagEvent): DiagReport {
  switch (e.stage) {
    case "dns":
      return { ...r, dns: e.report };
    case "tcp":
      return { ...r, tcp: e.report };
    case "ssh":
      return { ...r, ssh: e.report };
    case "route_started":
      return { ...r, routeRunning: true };
    case "route":
      return { ...r, route: e.report, routeRunning: false };
    case "auth":
      return { ...r, auth: e.report };
    case "verdict":
      // Nothing is running once the verdict is in, whatever stage never reported.
      return { ...r, verdict: e.verdict, routeRunning: false };
  }
}

/** The names the verdicts go by in the banner. */
const HEADINGS: Record<VerdictKind, string> = {
  healthy: "Healthy",
  name_not_resolved: "Name Not Resolved",
  local_network: "Local Network / ISP Drop",
  intermediate_hop: "Intermediate Hop / Firewall Timeout",
  bastion_unreachable: "Bastion Unreachable",
  target_port_closed: "Target Port Closed / SSH Daemon Down",
  auth_rejected: "Authentication / Key Rejected",
  host_key_problem: "Host Key Problem",
  unknown: "Not Enough Information",
};

export const verdictHeading = (kind: VerdictKind) => HEADINGS[kind];

export type Tone = "ok" | "warn" | "bad";

/** Healthy is green; a host key that needs a decision is amber; everything else is a failure. */
export function verdictTone(kind: VerdictKind): Tone {
  if (kind === "healthy") return "ok";
  if (kind === "host_key_problem" || kind === "unknown") return "warn";
  return "bad";
}

/**
 * Widths (0–100) for two timing bars drawn on one scale, so the longer is full and the other is in proportion; a
 * missing time draws nothing. Each non-zero time keeps a sliver so it is still visible beside a much longer one.
 */
export function barWidths(a: number | null | undefined, b: number | null | undefined): [number, number] {
  const top = Math.max(a ?? 0, b ?? 0);
  if (top <= 0) return [0, 0];
  const w = (v: number | null | undefined) => (v === null || v === undefined || v <= 0 ? 0 : Math.max(3, Math.round((v / top) * 100)));
  return [w(a), w(b)];
}

/** The colour class for a hop's loss: none for a clean hop, amber for some, red for most. */
export function lossClass(loss: number | null | undefined): string {
  if (loss === null || loss === undefined || loss <= 0) return "text-fg-muted";
  return loss >= 50 ? "text-danger" : "text-warning";
}

/** "0%", "33%", "100%"; a dash where the tool does not report loss. */
export const formatLoss = (loss: number | null | undefined) => (loss === null || loss === undefined ? "–" : `${Math.round(loss)}%`);

/** "0.6", "12.5", "128": one decimal below 100 ms, none above; a dash for no answer. */
export const formatAvg = (ms: number | null | undefined) => (ms === null || ms === undefined ? "–" : ms < 100 ? ms.toFixed(1) : String(Math.round(ms)));

/** How many of the stages that apply have reported, for a progress line. */
export function progress(r: DiagReport): { done: number; total: number } {
  const stages = [r.dns, r.tcp, r.ssh, r.route, r.verdict];
  // The SSH stage does not run when the TCP connection failed, and the route may be absent when it was not asked for.
  const skipped = (r.tcp && r.tcp.connect_ms === null ? 1 : 0) + (r.verdict && !r.route ? 1 : 0);
  return { done: stages.filter(Boolean).length, total: stages.length - skipped };
}
