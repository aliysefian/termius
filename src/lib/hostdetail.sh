# Samples a host's network, processes, listening ports and interfaces for the
# detail view. Run with `sh -c`, so it works whatever the login shell is.
# Output is plain text in sections introduced by a line `@@NAME`; the app does
# the arithmetic (see hostdetail.ts). It only reads: it changes nothing.
#
# Linux uses /proc and awk alone for processes, so a host without `ps` is fine.
# Counters are read twice, a second apart, so each run stands on its own.
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin:/opt/homebrew/bin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
os=$(uname -s 2>/dev/null)
echo "@@OS"; echo "$os"

if [ "$os" = Linux ]; then
  echo "@@CLK"; echo "$(getconf CLK_TCK 2>/dev/null || echo 100)"

  # One line per process: pid, CPU ticks, resident kB, user, name. Built from
  # a stream (cat ignores a process that vanishes; awk would stop on one).
  psample() {
    cat /etc/passwd /proc/[0-9]*/stat /proc/[0-9]*/status 2>/dev/null | awk '
      /^[0-9]+ \(/ {
        pid = $1 + 0
        i = index($0, "("); j = 0
        for (k = length($0); k > 0; k--) if (substr($0, k, 1) == ")") { j = k; break }
        comm = substr($0, i + 1, j - i - 1)
        n = split(substr($0, j + 2), f, " ")
        ticks[pid] = f[12] + f[13]; cm[pid] = comm
        next
      }
      $1 == "Pid:" { cur = $2 + 0; next }
      $1 == "Uid:" { uid[cur] = $2; next }
      $1 == "VmRSS:" { rss[cur] = $2; next }
      /^[^ \t:]+:[^:]*:[0-9]+:/ { split($0, p, ":"); uname[p[3]] = p[1]; next }
      END {
        for (pid in ticks) {
          u = (uid[pid] in uname) ? uname[uid[pid]] : uid[pid]
          printf "%s\t%s\t%s\t%s\t%s\n", pid, ticks[pid], (rss[pid] == "" ? 0 : rss[pid]), u, cm[pid]
        }
      }'
  }

  echo "@@TA"; awk '{print $1}' /proc/uptime
  echo "@@CPUA"; head -n 1 /proc/stat
  echo "@@NETA"; cat /proc/net/dev
  echo "@@PROCA"; psample
  sleep 1
  echo "@@TB"; awk '{print $1}' /proc/uptime
  echo "@@CPUB"; head -n 1 /proc/stat
  echo "@@NETB"; cat /proc/net/dev
  echo "@@PROCB"; psample

  echo "@@MEM"; grep -E '^(MemTotal|MemFree|MemAvailable|Buffers|Cached|SwapTotal|SwapFree):' /proc/meminfo
  echo "@@LOAD"; cat /proc/loadavg
  echo "@@DISK"; df -P / 2>/dev/null | tail -n 1

  echo "@@PORTS"
  if have ss; then
    echo "tool ss"; ss -lntup 2>/dev/null
  elif have netstat; then
    echo "tool netstat"; netstat -lntup 2>/dev/null || netstat -lntu 2>/dev/null
  else
    echo "tool proc"
    for f in tcp tcp6 udp udp6; do echo "file $f"; cat "/proc/net/$f" 2>/dev/null; done
  fi

  echo "@@IPADDR"; if have ip; then ip -o addr show 2>/dev/null; fi
  echo "@@IPLINK"; if have ip; then ip -o link show 2>/dev/null; fi
  echo "@@IFCONFIG"; if ! have ip && have ifconfig; then ifconfig -a 2>/dev/null; fi
  echo "@@SYSNET"
  for d in /sys/class/net/*; do
    [ -e "$d" ] || continue
    printf '%s %s %s %s\n' "${d##*/}" "$(cat "$d/operstate" 2>/dev/null)" "$(cat "$d/address" 2>/dev/null)" "$(cat "$d/mtu" 2>/dev/null)"
  done
else
  # macOS and the BSDs: no /proc, so the system's own tools. CPU per process
  # is the system's own (decayed) figure; interface counters are sampled twice.
  echo "@@TA"; date +%s
  echo "@@NETA"; netstat -ibn 2>/dev/null
  sleep 1
  echo "@@TB"; date +%s
  echo "@@NETB"; netstat -ibn 2>/dev/null

  echo "@@PROCPS"; ps -axo pid=,user=,pcpu=,rss=,comm= 2>/dev/null
  echo "@@LOAD"; sysctl -n vm.loadavg 2>/dev/null | tr -d '{}'
  echo "@@DISK"; df -P / 2>/dev/null | tail -n 1

  echo "@@PORTS"
  if have lsof; then
    echo "tool lsof"; lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null; lsof -nP -iUDP 2>/dev/null
  elif have sockstat; then
    echo "tool sockstat"; sockstat -l 2>/dev/null
  else
    echo "tool none"
  fi

  echo "@@IFCONFIG"; ifconfig -a 2>/dev/null
fi
