// Parses the output of the small portable script that samples a host's
// CPU, memory, disk and load without anything installed beyond a POSIX
// shell, awk and df (present on essentially every Linux and BSD/macOS
// box). Kept separate from the polling store so the parsing — the part
// most likely to need a tweak for some shell's quirks — is unit-testable
// on its own.
//
// Linux: reads /proc/stat twice, a second apart, for a real CPU delta; the
// rest from /proc/meminfo, /proc/loadavg and /proc/uptime. BSD/macOS (no
// /proc): load from `sysctl vm.loadavg`, uptime from `kern.boottime`. CPU and
// memory come from top and vm_stat on macOS and from top and sysctl on FreeBSD
// (written from the man pages, not yet run on either); on other BSDs they
// come back null.
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
  # CPU and memory where the system's own tools give them. Anything that can't
  # be read stays empty, which the app shows as "not available", never as 0.
  case "$(uname -s 2>/dev/null)" in
    Darwin)
      # The second display of top covers a real interval; its "CPU usage" line ends in "% idle".
      cpu=$(top -l 2 -n 0 -s 1 2>/dev/null | awk '/^CPU usage/ { l = $0 } END { if (match(l, /[0-9.]+% idle/)) { v = substr(l, RSTART, RLENGTH); sub(/%.*/, "", v); printf "%d", 100 - v + 0.5 } }')
      # Used = active + wired + compressed pages, out of the installed memory (what Activity Monitor adds up).
      mem_used_pct=$(vm_stat 2>/dev/null | awk -v total="$(sysctl -n hw.memsize 2>/dev/null)" '
        function n(s) { gsub(/[^0-9]/, "", s); return s + 0 }
        /page size of/ { match($0, /[0-9]+ bytes/); ps = substr($0, RSTART, RLENGTH - 6) + 0 }
        /^Pages active/ { act = n($3) }
        /^Pages wired down/ { wired = n($4) }
        /^Pages occupied by compressor/ { comp = n($5) }
        END { if (total > 0 && ps > 0) printf "%d", 100 * (act + wired + comp) * ps / total }')
      ;;
    FreeBSD)
      cpu=$(top -b -d 2 -s 1 0 2>/dev/null | awk '/^CPU:/ { l = $0 } END { if (match(l, /[0-9.]+% idle/)) { v = substr(l, RSTART, RLENGTH); sub(/%.*/, "", v); printf "%d", 100 - v + 0.5 } }')
      # Used = everything except free and inactive pages.
      mem_used_pct=$(sysctl -n hw.physmem hw.pagesize vm.stats.vm.v_free_count vm.stats.vm.v_inactive_count 2>/dev/null | awk 'NR == 1 { t = $1 } NR == 2 { p = $1 } NR == 3 { f = $1 } NR == 4 { i = $1 } END { if (t > 0 && p > 0) printf "%d", 100 * (1 - (f + i) * p / t) }')
      ;;
  esac
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
