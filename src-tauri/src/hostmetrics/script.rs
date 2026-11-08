//! The shell that reads a host. One run answers everything the frequent readings need, so a sample costs one channel on
//! the connection that is already open, not one per metric.
//!
//! It only reads: `/proc`, `/sys`, `uname` and `df`. It installs nothing and writes nothing. Output is plain text in
//! sections introduced by a line `@@NAME`; the arithmetic is done by the app (see `derive.rs`), never by the host, so a
//! host with a strange `awk` cannot skew a figure. A run ends with `@@END`: a run that was cut short has no end line and
//! is an error, not a short reading.
//!
//! Counters are read once per run (no `sleep`): a rate is the change since the *previous* run, over the host's own
//! uptime clock, so a sample is instant and the first one honestly has no rates.

/// Defines the helpers and opens the Linux branch. Other systems get only `@@OS` and `@@END`: they are not guessed at.
const HEAD: &str = r#"LC_ALL=C; export LC_ALL
SYS=${SYS:-/sys}
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
rd() { v=; { read -r v < "$1"; } 2>/dev/null || v=; }
os=$(uname -s 2>/dev/null)
echo "@@OS"; echo "$os"
if [ "$os" = Linux ]; then
"#;

/// Everything read at every sample.
const FREQUENT: &str = r#"echo "@@UPTIME"; cat /proc/uptime 2>/dev/null
echo "@@STAT"; grep '^cpu' /proc/stat 2>/dev/null
echo "@@MEMINFO"; cat /proc/meminfo 2>/dev/null
echo "@@LOADAVG"; cat /proc/loadavg 2>/dev/null
echo "@@PROCS"; set -- /proc/[0-9]*; echo $#
echo "@@CPUFREQ"; cat /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq 2>/dev/null
echo "@@NETDEV"; cat /proc/net/dev 2>/dev/null
echo "@@NETIF"
for d in /sys/class/net/*; do
  [ -e "$d" ] || continue
  n=${d##*/}
  rd "$d/operstate"; st=$v
  rd "$d/mtu"; mtu=$v
  rd "$d/speed"; sp=$v
  rd "$d/type"; ty=$v
  rd "$d/ifindex"; ix=$v
  dev=0; [ -e "$d/device" ] && dev=1
  dt=; { while IFS= read -r l; do case $l in DEVTYPE=*) dt=${l#DEVTYPE=};; esac; done < "$d/uevent"; } 2>/dev/null
  fl=
  [ -d "$d/bridge" ] && fl="$fl bridge"
  [ -d "$d/bonding" ] && fl="$fl bond"
  { [ -d "$d/wireless" ] || [ -d "$d/phy80211" ]; } && fl="$fl wireless"
  [ -e "$d/tun_flags" ] && fl="$fl tun"
  printf 'IF|%s|%s|%s|%s|%s|%s|%s|%s|%s\n' "$n" "$st" "$mtu" "$sp" "$ty" "$ix" "$dev" "$dt" "$fl"
  for l in "$d"/lower_*; do
    [ -e "$l" ] && printf 'LOWER|%s|%s\n' "$n" "${l##*/lower_}"
  done
done
echo "@@ROUTE"; awk '$2=="00000000" { if (!f || $7+0 < m) { m = $7+0; i = $1; f = 1 } } END { if (f) print i }' /proc/net/route 2>/dev/null
echo "@@DISKSTATS"; cat /proc/diskstats 2>/dev/null
echo "@@BLOCK"
for d in /sys/block/*; do
  [ -e "$d" ] || continue
  n=${d##*/}
  case $n in loop*|ram*) continue;; esac
  rd "$d/size"; sz=$v
  rd "$d/queue/rotational"; ro=$v
  rd "$d/device/model"; md=$v
  vt=0; [ -e "/sys/devices/virtual/block/$n" ] && vt=1
  printf '%s|%s|%s|%s|%s\n' "$n" "$sz" "$ro" "$vt" "$md"
done
echo "@@DF"
T=; have timeout && T="timeout 5"
L=; [ -z "$T" ] && L=l
o=$($T df -Pk${L}T 2>/dev/null); [ -n "$o" ] || o=$($T df -Pk$L 2>/dev/null)
echo "$o"
echo "@@TOOLS"
for t in ss ip lsblk lscpu systemctl journalctl sensors nvidia-smi rocm-smi timeout ufw nft iptables last lastb who ps; do have "$t" && echo "$t"; done
echo "@@TEMPS"
for h in "$SYS"/class/hwmon/hwmon*; do
  [ -d "$h" ] || continue
  rd "$h/name"; chip=$v
  for f in "$h"/temp*_input; do
    [ -e "$f" ] || continue
    b=${f##*/}; b=${b%_input}
    rd "$f"; val=$v
    rd "$h/${b}_label"; lab=$v
    rd "$h/${b}_max"; hi=$v
    rd "$h/${b}_crit"; cr=$v
    printf 'H|%s|%s|%s|%s|%s|%s\n' "$chip" "$b" "$lab" "$val" "$hi" "$cr"
  done
done
for z in "$SYS"/class/thermal/thermal_zone*; do
  [ -d "$z" ] || continue
  rd "$z/type"; ty=$v
  rd "$z/temp"; val=$v
  printf 'Z|%s|%s\n' "$ty" "$val"
done
echo "@@AMDGPU"
for d in "$SYS"/class/drm/card[0-9]*; do
  n=${d##*/}
  case $n in *-*) continue;; esac
  rd "$d/device/vendor"; [ "$v" = 0x1002 ] || continue
  rd "$d/device/gpu_busy_percent"; busy=$v
  rd "$d/device/mem_info_vram_used"; vu=$v
  rd "$d/device/mem_info_vram_total"; vt=$v
  rd "$d/device/product_name"; pn=$v
  pci=; drv=; { while IFS= read -r l; do case $l in PCI_ID=*) pci=${l#PCI_ID=};; DRIVER=*) drv=${l#DRIVER=};; esac; done < "$d/device/uevent"; } 2>/dev/null
  t=; pw=; pl=; fan=; fmax=
  for hm in "$d"/device/hwmon/hwmon*; do
    [ -d "$hm" ] || continue
    rd "$hm/temp1_input"; t=$v
    rd "$hm/power1_average"; pw=$v
    [ -n "$pw" ] || { rd "$hm/power1_input"; pw=$v; }
    rd "$hm/power1_cap"; pl=$v
    rd "$hm/pwm1"; fan=$v
    rd "$hm/pwm1_max"; fmax=$v
    break
  done
  printf 'A|%s|%s|%s|%s|%s|%s|%s|%s|%s|%s|%s|%s\n' "$n" "$pn" "$pci" "$drv" "$busy" "$vu" "$vt" "$t" "$pw" "$pl" "$fan" "$fmax"
done
if [ "$NV" = 1 ]; then
  if have nvidia-smi && have timeout; then
    echo "@@NVIDIA"
    timeout 5 nvidia-smi --query-gpu=index,name,utilization.gpu,utilization.memory,memory.used,memory.total,temperature.gpu,power.draw,power.limit,fan.speed,driver_version,uuid --format=csv,noheader,nounits 2>/dev/null
    echo "rc=$?"
    echo "@@NVAPPS"
    timeout 5 nvidia-smi --query-compute-apps=pid,process_name,used_memory,gpu_uuid --format=csv,noheader,nounits 2>/dev/null
  elif have nvidia-smi; then
    echo "@@NVSKIP"; echo "no timeout command"
  fi
fi
"#;

/// Facts that do not change, appended only now and then.
const STATIC: &str = r#"echo "@@STATIC"
rd /proc/sys/kernel/hostname; echo "hostname=$v"
rd /proc/sys/kernel/osrelease; echo "kernel=$v"
echo "arch=$(uname -m 2>/dev/null)"
if [ -r /etc/os-release ]; then { while IFS= read -r l; do case $l in PRETTY_NAME=*) echo "os=${l#PRETTY_NAME=}";; esac; done < /etc/os-release; } 2>/dev/null; fi
rd /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq; echo "max_khz=$v"
rd /sys/devices/system/cpu/cpu0/cpufreq/base_frequency; echo "base_khz=$v"
for c in /sys/devices/system/cpu/cpu0/cache/index*; do
  [ -d "$c" ] || continue
  rd "$c/level"; lv=$v; rd "$c/type"; ty=$v; rd "$c/size"; sz=$v
  echo "cache=L$lv $ty|$sz"
done
echo "@@CPUINFO"
awk -F: '
  /^processor/ { n++ }
  /^model name|^Model Name|^cpu model|^Hardware/ { if (!m) { m = $2; sub(/^ +/, "", m) } }
  /^physical id/ { p[$2] = 1 }
  /^cpu cores/ { c = $2 + 0 }
  /^flags|^Features/ { if (index($0, " vmx")) v = "vmx"; else if (index($0, " svm")) v = "svm" }
  END { s = 0; for (k in p) s++; printf "model=%s\nlogical=%d\ncores_per_socket=%d\nsockets=%d\nvirt=%s\n", m, n, c, s, v }
' /proc/cpuinfo 2>/dev/null
echo "@@LSCPU"; have lscpu && lscpu 2>/dev/null
"#;

const TAIL: &str = r#"fi
echo "@@END"
"#;

/// What one reading should include beyond the cheap, always-present part.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The facts that change rarely (CPU model, kernel, caches).
    pub with_static: bool,
    /// Ask `nvidia-smi` (a program start, so not on every reading). Only sensible when it is known to be installed.
    pub nvidia: bool,
}

/// The script for one reading. `with_static` adds the facts that change rarely (CPU model, kernel, caches).
pub fn script(with_static: bool) -> String {
    script_with(Options { with_static, nvidia: false })
}

pub fn script_with(o: Options) -> String {
    let mut s = String::with_capacity(HEAD.len() + FREQUENT.len() + STATIC.len() + TAIL.len() + 8);
    if o.nvidia {
        s.push_str("NV=1; export NV\n");
    }
    s.push_str(HEAD);
    s.push_str(FREQUENT);
    if o.with_static {
        s.push_str(STATIC);
    }
    s.push_str(TAIL);
    s
}
