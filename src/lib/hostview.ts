// Small pure decisions the Host Metrics views make about a reading, kept out of the components so they can be tested.
import type { Filesystem, HostSample, IfaceKind, Memory, NetIface } from "./hostsample";

/** The filesystem to put on the overview: the root one, else the fullest. */
export function mainFilesystem(fs: Filesystem[]): Filesystem | null {
  return fs.find((f) => f.mount === "/") ?? [...fs].sort((a, b) => b.used_pct - a.used_pct)[0] ?? null;
}

export interface MemoryBar {
  used: number;
  /** Page cache and buffers the kernel can give back: what is available beyond what is simply free. */
  reclaimable: number;
  free: number;
}

/**
 * The three parts of the memory bar, as shares of the total. They add up to the whole by construction (in use = total -
 * available; free; and what is available but not free), so cache is never counted twice and the bar agrees with the
 * percentage shown beside it.
 */
export function memoryBar(m: Memory): MemoryBar {
  const free = Math.min(m.free, m.available);
  return { used: m.used / m.total, reclaimable: Math.max(0, m.available - free) / m.total, free: free / m.total };
}

export const KIND_LABEL: Record<IfaceKind, string> = {
  physical: "Ethernet",
  wireless: "Wi-Fi",
  bond: "Bond",
  bridge: "Bridge",
  vlan: "VLAN",
  tunnel: "Tunnel / VPN",
  container: "Container",
  loopback: "Loopback",
  other: "Virtual",
};

/** Why an interface is, or is not, in the total: said once, in words, so nobody wonders about double counting. */
export function countedNote(i: NetIface): string {
  if (i.counted) return "Counted in the host total.";
  if (i.kind === "bridge" || i.kind === "vlan" || i.kind === "tunnel" || i.kind === "container") return "Not in the total: its traffic also crosses a real interface.";
  if (i.master) return `Not in the total: it is part of ${i.master}, which is counted.`;
  return "Not in the total.";
}

/** Load average against the number of CPUs: 1.0 per CPU means every CPU is busy all the time. */
export function loadPerCpu(load1: number, logical: number): number | null {
  return logical > 0 ? load1 / logical : null;
}

export type Tone = "ok" | "warn" | "bad" | "none";

/** How worried a use percentage should look. Colour is never the only signal: the figure is always printed too. */
export function usageTone(pct: number | null | undefined, warn = 75, bad = 90): Tone {
  if (pct === null || pct === undefined) return "none";
  return pct >= bad ? "bad" : pct >= warn ? "warn" : "ok";
}

/** Whether a reading has anything to show, and if not, why: the Linux collector is the only one so far. */
export function unsupportedReason(s: HostSample): string | null {
  return s.platform.kind === "unsupported" ? `Host metrics are not available for ${s.platform.os || "this operating system"} yet. Only Linux hosts are read; nothing is guessed for others.` : null;
}

/** The interface rows the Network view lists: real traffic first, then bridges, tunnels and containers. */
export function sortedInterfaces(list: NetIface[]): NetIface[] {
  const rank = (i: NetIface) => (i.counted ? 0 : i.kind === "bridge" || i.kind === "vlan" ? 1 : i.kind === "tunnel" ? 2 : 3);
  return [...list].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, undefined, { numeric: true }));
}
