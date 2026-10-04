// Parses the output of the small portable script that samples a host's
// CPU, memory, disk and load without anything installed beyond a POSIX
// shell, awk and df (present on essentially every Linux and BSD/macOS
// box). Kept separate from the polling store so the parsing — the part
// most likely to need a tweak for some shell's quirks — is unit-testable
// on its own.
//
// Linux: reads /proc/stat twice, a second apart, for a real CPU delta; the
// rest from /proc/meminfo, /proc/loadavg and /proc/uptime. BSD/macOS (no
// /proc): load from `sysctl vm.loadavg`, uptime from `kern.boottime`; CPU
// and memory percentage aren't sampled there (no single portable sysctl
// knob covers them as cleanly), so they come back null.
export const METRICS_SCRIPT = `if [ -f /proc/stat ]; then
  read -r _ u1 n1 s1 i1 w1 _ < /proc/stat
  sleep 1
  read -r _ u2 n2 s2 i2 w2 _ < /proc/stat
  busy1=$((u1+n1+s1)); idle1=$((i1+w1)); total1=$((busy1+idle1))
  busy2=$((u2+n2+s2)); idle2=$((i2+w2)); total2=$((busy2+idle2))
  dtotal=$((total2-total1)); dbusy=$((busy2-busy1))
  if [ "$dtotal" -gt 0 ]; then cpu=$((100*dbusy/dtotal)); else cpu=0; fi
  mem_total=$(awk '/MemTotal/{print $2}' /proc/meminfo)
  mem_avail=$(awk '/MemAvailable/{print $2}' /proc/meminfo)
  if [ -n "$mem_total" ] && [ "$mem_total" -gt 0 ]; then
    mem_used_pct=$(( (mem_total-mem_avail) * 100 / mem_total ))
  else
    mem_used_pct=""
  fi
  load=$(awk '{print $1,$2,$3}' /proc/loadavg)
  up=$(awk '{print int($1)}' /proc/uptime)
else
  cpu=""
  mem_used_pct=""
  load=$(sysctl -n vm.loadavg 2>/dev/null | tr -d '{}')
  boot=$(sysctl -n kern.boottime 2>/dev/null | awk -F= '{print $2+0}')
  now=$(date +%s)
  if [ -n "$boot" ]; then up=$((now-boot)); else up=""; fi
fi
disk_pct=$(df -P / 2>/dev/null | awk 'NR==2{gsub("%","",$5); print $5}')
echo "CPU=$cpu"
echo "MEM=$mem_used_pct"
echo "LOAD=$load"
echo "DISK=$disk_pct"
echo "UPTIME=$up"
`;

export interface HostMetrics {
  /** Percent busy over a 1-second sample; null on platforms without /proc. */
  cpuPct: number | null;
  memPct: number | null;
  diskPct: number | null;
  /** The three load-average numbers as shown by `uptime`, e.g. "0.12 0.09 0.05". */
  load: string | null;
  uptimeSecs: number | null;
}

function num(v: string | undefined): number | null {
  if (!v) return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

export function parseMetricsOutput(stdout: string): HostMetrics {
  const fields: Record<string, string> = {};
  for (const line of stdout.split("\n")) {
    const eq = line.indexOf("=");
    if (eq > 0) fields[line.slice(0, eq).trim()] = line.slice(eq + 1).trim();
  }
  return {
    cpuPct: num(fields.CPU),
    memPct: num(fields.MEM),
    diskPct: num(fields.DISK),
    load: fields.LOAD || null,
    uptimeSecs: num(fields.UPTIME),
  };
}

export function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}
