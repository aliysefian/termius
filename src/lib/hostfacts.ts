// Facts about a host that a person asked to collect: what it runs, how big it is, which tools it has. One read-only
// script, run when someone presses the button (never in the background), and the answer is stamped with when it was
// observed, so it is never mistaken for live state. Pure, so the parsing is tested without a connection.

/**
 * Prints `KEY=value` lines. It only reads: files under /proc, /etc and /sys, `uname`, `df`, `nproc`, and `--version`
 * of tools that are there. Nothing is installed, started or changed, and nothing scans a network.
 */
export const FACTS_SCRIPT = `LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin:/opt/homebrew/bin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
echo "HOSTNAME=$(hostname 2>/dev/null)"
echo "KERNEL=$(uname -sr 2>/dev/null)"
echo "ARCH=$(uname -m 2>/dev/null)"
if [ -r /etc/os-release ]; then echo "OS=$(. /etc/os-release 2>/dev/null; echo "$PRETTY_NAME")"; else echo "OS=$(uname -s 2>/dev/null)"; fi
if [ -r /proc/cpuinfo ]; then echo "CPU=$(grep -m1 'model name' /proc/cpuinfo | sed 's/.*: *//')"; fi
echo "CORES=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null)"
if [ -r /proc/meminfo ]; then
  echo "MEM_KB=$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
  echo "SWAP_KB=$(awk '/^SwapTotal:/ {print $2}' /proc/meminfo)"
else
  echo "MEM_KB=$(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1024 ))"
fi
if [ -r /proc/uptime ]; then echo "UPTIME=$(awk '{print int($1)}' /proc/uptime)"; fi
df -P -k 2>/dev/null | awk 'NR > 1 && $1 !~ /^(tmpfs|devtmpfs|overlay|shm|udev|none)$/ && $6 !~ /^\\/(proc|sys|dev|run|snap)/ { printf "DISK=%s|%s|%s|%s\\n", $6, $2, $3, $5 }' | head -n 12
if [ -d /run/systemd/system ]; then echo "INIT=systemd"; elif [ -f /.dockerenv ]; then echo "INIT=container"; fi
if have systemd-detect-virt; then echo "VIRT=$(systemd-detect-virt 2>/dev/null)"; fi
for t in docker podman nerdctl kubectl helm git python3 node; do
  if have "$t"; then echo "TOOL=$t|$("$t" --version 2>/dev/null | head -n 1 | cut -c1-80)"; fi
done
`;

export interface Disk {
  mount: string;
  sizeKb: number;
  usedKb: number;
  usedPct: number | null;
}

export interface Tool {
  name: string;
  version: string;
}

export interface HostFacts {
  hostname: string | null;
  os: string | null;
  kernel: string | null;
  arch: string | null;
  cpu: string | null;
  cores: number | null;
  memKb: number | null;
  swapKb: number | null;
  uptimeSecs: number | null;
  disks: Disk[];
  /** "systemd", "container", or null when the script could not tell. */
  init: string | null;
  virtualization: string | null;
  tools: Tool[];
}

const num = (v: string | undefined): number | null => {
  if (v === undefined || v.trim() === "") return null;
  const n = Number(v);
  return Number.isFinite(n) && n >= 0 ? n : null;
};
const text = (v: string | undefined): string | null => (v && v.trim() ? v.trim().slice(0, 200) : null);

export function parseFacts(stdout: string): HostFacts {
  const one: Record<string, string> = {};
  const disks: Disk[] = [];
  const tools: Tool[] = [];
  for (const raw of stdout.split("\n")) {
    const line = raw.replace(/\r$/, "");
    const eq = line.indexOf("=");
    if (eq <= 0) continue;
    const key = line.slice(0, eq);
    const value = line.slice(eq + 1);
    if (key === "DISK") {
      const [mount, size, used, pct] = value.split("|");
      const s = num(size);
      if (mount && s !== null) disks.push({ mount: mount.slice(0, 200), sizeKb: s, usedKb: num(used) ?? 0, usedPct: num((pct ?? "").replace("%", "")) });
    } else if (key === "TOOL") {
      const [name, ...rest] = value.split("|");
      if (/^[a-z0-9]+$/.test(name ?? "")) tools.push({ name, version: rest.join("|").trim().slice(0, 80) });
    } else {
      one[key] = value;
    }
  }
  return {
    hostname: text(one.HOSTNAME),
    os: text(one.OS),
    kernel: text(one.KERNEL),
    arch: text(one.ARCH),
    cpu: text(one.CPU),
    cores: num(one.CORES),
    memKb: num(one.MEM_KB),
    swapKb: num(one.SWAP_KB),
    uptimeSecs: num(one.UPTIME),
    disks,
    init: text(one.INIT),
    virtualization: text(one.VIRT),
    tools,
  };
}

/** "3.8 GiB" from kibibytes. */
export function sizeText(kb: number | null): string {
  if (kb === null) return "—";
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let v = kb;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/** The facts as plain text, to copy into a ticket or a document. */
export function factsText(f: HostFacts, observedAt: number, hostLabel: string): string {
  const row = (k: string, v: string | null) => (v ? [`${k}: ${v}`] : []);
  return [
    `${hostLabel}: facts observed ${new Date(observedAt).toLocaleString()}`,
    ...row("Hostname", f.hostname),
    ...row("OS", f.os),
    ...row("Kernel", f.kernel),
    ...row("Architecture", f.arch),
    ...row("CPU", f.cpu ? `${f.cpu}${f.cores ? ` (${f.cores} cores)` : ""}` : f.cores ? `${f.cores} cores` : null),
    ...row("Memory", f.memKb !== null ? `${sizeText(f.memKb)}${f.swapKb ? `, swap ${sizeText(f.swapKb)}` : ""}` : null),
    ...row("Init", f.init),
    ...row("Virtualization", f.virtualization),
    ...f.disks.map((d) => `Disk ${d.mount}: ${sizeText(d.sizeKb)}${d.usedPct !== null ? `, ${d.usedPct}% used` : ""}`),
    ...f.tools.map((t) => `Tool ${t.name}: ${t.version || "present"}`),
  ].join("\n");
}
