// A host's packages, read-only: what is installed, what has an update waiting, whether a restart is due. Package managers
// differ, so each is read in its own way and brought to one shape; what a manager does not say stays unknown (null).
import SCRIPT from "./packages.sh?raw";
import { complete, hostNow, nonEmpty, sections, type Limit } from "./sections";

export const PACKAGES_SCRIPT: string = SCRIPT.replace(/\r\n/g, "\n");

export type Manager = "apt" | "dnf" | "yum" | "zypper" | "pacman" | "apk" | "none";

export interface Package {
  name: string;
  version: string;
  arch: string | null;
  held: boolean;
}

export interface Update {
  name: string;
  /** The version on offer. */
  available: string;
  /** The version installed, when the manager says. */
  current: string | null;
  /** The repository or suite it comes from, when the manager says. */
  source: string | null;
}

export interface PackageReport {
  manager: Manager;
  packages: Package[];
  updates: Update[];
  /** A restart is waiting (apt's flag); null where the manager has none. */
  rebootRequired: boolean | null;
  /** Seconds since the host last refreshed its package lists, when it says. */
  cacheAgeSecs: number | null;
  limits: Limit[];
  truncated: boolean;
}

const MANAGERS: Manager[] = ["apt", "dnf", "yum", "zypper", "pacman", "apk", "none"];

export function parseApt(lines: string[]): Package[] {
  const out: Package[] = [];
  for (const l of nonEmpty(lines)) {
    const [name, version, arch, status] = l.split("\t");
    // "ii " is installed, "hi " installed and held; anything else (rc: removed with config left) is not installed.
    if (!name || !version || !status || !/^[ih]i/.test(status)) continue;
    out.push({ name, version, arch: arch || null, held: status.startsWith("h") });
  }
  return out;
}

export function parseRpm(lines: string[]): Package[] {
  return nonEmpty(lines).flatMap((l) => {
    const [name, version, arch] = l.split("\t");
    return name && version ? [{ name, version, arch: arch || null, held: false }] : [];
  });
}

export function parsePacman(lines: string[]): Package[] {
  return nonEmpty(lines).flatMap((l) => {
    const [name, version] = l.trim().split(/\s+/);
    return name && version ? [{ name, version, arch: null, held: false }] : [];
  });
}

/** `busybox-1.35.0-r1` is a name and a version; the version starts at the last "-<digit>". */
export function splitApk(token: string): { name: string; version: string } | null {
  const m = /^(.+)-(\d[\w.+~]*-r\d+)$/.exec(token);
  return m ? { name: m[1], version: m[2] } : null;
}

export function parseApk(lines: string[]): Package[] {
  return nonEmpty(lines).flatMap((l) => {
    const [token, arch] = l.trim().split(/\s+/);
    const nv = token ? splitApk(token) : null;
    return nv ? [{ ...nv, arch: arch && !arch.startsWith("{") ? arch : null, held: false }] : [];
  });
}

export function parseAptUpdates(lines: string[]): Update[] {
  const out: Update[] = [];
  for (const l of lines) {
    // curl/jammy-updates,jammy-security 7.81.0-1ubuntu1.18 amd64 [upgradable from: 7.81.0-1ubuntu1.15]
    const m = /^([^/\s]+)\/(\S+)\s+(\S+)\s+\S+(?:\s+\[upgradable from: (\S+)\])?/.exec(l.trim());
    if (m) out.push({ name: m[1], available: m[3], current: m[4] ?? null, source: m[2].split(",")[0] });
  }
  return out;
}

export function parseDnfUpdates(lines: string[]): Update[] {
  const out: Update[] = [];
  for (const l of lines) {
    // kernel.x86_64    5.14.0-362.el9    baseos
    if (/^(Obsoleting|Security:|Last metadata)/i.test(l.trim())) continue;
    const t = l.trim().split(/\s+/);
    if (t.length !== 3 || !/\.[\w]+$/.test(t[0]) || !/\d/.test(t[1])) continue;
    out.push({ name: t[0].replace(/\.[\w]+$/, ""), available: t[1], current: null, source: t[2] });
  }
  return out;
}

export function parseZypperUpdates(lines: string[]): Update[] {
  const out: Update[] = [];
  for (const l of lines) {
    // v | repo | name | current | available | arch
    const c = l.split("|").map((x) => x.trim());
    if (c.length >= 6 && c[0] !== "S" && !/^-+$/.test(c[0]) && c[2]) out.push({ name: c[2], current: c[3] || null, available: c[4], source: c[1] || null });
  }
  return out;
}

export function parsePacmanUpdates(lines: string[]): Update[] {
  const out: Update[] = [];
  for (const l of lines) {
    const m = /^(\S+)\s+(\S+)\s+->\s+(\S+)/.exec(l.trim());
    if (m) out.push({ name: m[1], current: m[2], available: m[3], source: null });
  }
  return out;
}

export function parseApkUpdates(lines: string[]): Update[] {
  const out: Update[] = [];
  for (const l of lines) {
    const m = /^(\S+)\s+<\s+(\S+)/.exec(l.trim());
    const nv = m ? splitApk(m[1]) : null;
    if (m && nv) out.push({ name: nv.name, current: nv.version, available: m[2], source: null });
  }
  return out;
}

export function parsePackages(out: string): PackageReport | null {
  const s = sections(out);
  if (!complete(s)) return null;
  const manager = (nonEmpty(s.get("MANAGER"))[0]?.trim() ?? "none") as Manager;
  const m: Manager = MANAGERS.includes(manager) ? manager : "none";
  const installedLines = s.get("INSTALLED") ?? [];
  const updateLines = s.get("UPDATES") ?? [];
  const packages = m === "apt" ? parseApt(installedLines) : m === "dnf" || m === "yum" || m === "zypper" ? parseRpm(installedLines) : m === "pacman" ? parsePacman(installedLines) : m === "apk" ? parseApk(installedLines) : [];
  const updates = m === "apt" ? parseAptUpdates(updateLines) : m === "dnf" || m === "yum" ? parseDnfUpdates(updateLines) : m === "zypper" ? parseZypperUpdates(updateLines) : m === "pacman" ? parsePacmanUpdates(updateLines) : m === "apk" ? parseApkUpdates(updateLines) : [];
  const held = new Set(nonEmpty(s.get("HELD")).map((x) => x.trim()));
  for (const p of packages) if (held.has(p.name)) p.held = true;

  const state = new Map(nonEmpty(s.get("STATE")).map((l) => l.split("=", 2) as [string, string]));
  const now = hostNow(s);
  const stamp = Number(state.get("cache"));
  const limits: Limit[] = [];
  if (m === "none") limits.push({ what: "Packages", reason: "No supported package manager was found (apt, dnf, yum, zypper, pacman or apk)." });
  else if (packages.length === 0) limits.push({ what: "Installed packages", reason: "The host listed no installed packages." });
  if ((m === "dnf" || m === "yum") && updates.length === 0) limits.push({ what: "Updates", reason: `${m} is asked about updates from its own cache only; with no cached metadata it lists none. Refreshing it needs root and the network, so an empty list here is not proof that nothing is waiting.` });
  if (m === "zypper" && updates.length === 0) limits.push({ what: "Updates", reason: "zypper is asked without refreshing its repositories, so this is as current as its last refresh." });
  if (m === "pacman" || m === "apk") limits.push({ what: "Updates", reason: `${m} lists updates against what it last synchronised; the host may know of newer ones.` });
  return {
    manager: m,
    packages,
    updates,
    rebootRequired: state.has("reboot") ? state.get("reboot") === "yes" : null,
    cacheAgeSecs: Number.isFinite(stamp) && stamp > 0 ? Math.max(0, now - stamp) : null,
    limits,
    truncated: installedLines.filter((l) => l.trim()).length >= 20000,
  };
}

export interface PackageRow extends Package {
  update: Update | null;
}

/** Installed packages with the update waiting for each, matched by name. */
export function withUpdates(r: PackageReport): PackageRow[] {
  const by = new Map(r.updates.map((u) => [u.name, u]));
  return r.packages.map((p) => ({ ...p, update: by.get(p.name) ?? null }));
}

export function ageText(secs: number | null): string {
  if (secs === null) return "unknown";
  if (secs < 3600) return `${Math.max(1, Math.round(secs / 60))} min ago`;
  if (secs < 172_800) return `${Math.round(secs / 3600)} h ago`;
  return `${Math.round(secs / 86_400)} days ago`;
}
