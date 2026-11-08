use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;

use super::derive::{baseline, delta, derive};
use super::manager::{BoxFuture, Config, EventSink, HostExec, HostMetricsManager, MetricsError};
use super::model::*;
use super::parse::{classify, parse, ParseError, Raw};
use super::script;

// -- a host in a string ----------------------------------------------------------------------------------------------

/// What the script prints for a pretend host. Change a field, print again: that is the next reading.
#[derive(Clone)]
pub(super) struct Fx {
    uptime: f64,
    cpu: [u64; 8],
    cores: Vec<[u64; 8]>,
    mem: Vec<(&'static str, u64)>,
    nets: Vec<(&'static str, u64, u64)>,
    ifs: Vec<String>,
    lower: Vec<(&'static str, &'static str)>,
    route: &'static str,
    disks: Vec<(&'static str, [u64; 7])>,
    block: Vec<&'static str>,
    df: &'static str,
    statics: bool,
    os: &'static str,
}

fn iface(name: &str, state: &str, ifindex: u32, dev: u8, devtype: &str, flags: &str) -> String {
    format!("IF|{name}|{state}|1500|-1|1|{ifindex}|{dev}|{devtype}|{flags}")
}

impl Fx {
    pub(super) fn host() -> Self {
        Fx {
            uptime: 1000.0,
            cpu: [1000, 0, 500, 8000, 100, 0, 0, 0],
            cores: vec![[500, 0, 250, 4000, 50, 0, 0, 0], [500, 0, 250, 4000, 50, 0, 0, 0]],
            mem: vec![("MemTotal", 16_000_000), ("MemFree", 2_000_000), ("MemAvailable", 8_000_000), ("Buffers", 500_000), ("Cached", 6_000_000), ("Shmem", 1_000_000), ("SReclaimable", 500_000), ("SwapTotal", 4_000_000), ("SwapFree", 3_000_000), ("Committed_AS", 9_000_000), ("Slab", 800_000)],
            nets: vec![("lo", 5000, 5000), ("eth0", 1_000_000, 500_000)],
            ifs: vec![iface("lo", "unknown", 1, 0, "", ""), iface("eth0", "up", 2, 1, "", "")],
            lower: vec![],
            route: "eth0",
            disks: vec![("sda", [100, 2000, 50, 200, 4000, 100, 120]), ("sda1", [90, 1800, 45, 190, 3900, 95, 110]), ("dm-0", [90, 1800, 45, 190, 3900, 95, 110])],
            block: vec!["sda|2000000000|0|0|Samsung SSD 860  ", "dm-0|1000000|0|1|", "loop0|100|1|1|"],
            df: "Filesystem     Type     1024-blocks     Used Available Capacity Mounted on\n/dev/sda2      ext4       123000000 92000000  26000000      78% /\ntmpfs          tmpfs        1000000        0   1000000       0% /dev/shm\n/dev/loop3     squashfs      100000   100000         0     100% /snap/core/1\n",
            statics: false,
            os: "Linux",
        }
    }

    pub(super) fn at(mut self, uptime: f64) -> Self {
        self.uptime = uptime;
        self
    }

    fn net(&mut self, name: &'static str, rx: u64, tx: u64) {
        if let Some(n) = self.nets.iter_mut().find(|n| n.0 == name) {
            n.1 = rx;
            n.2 = tx;
        } else {
            self.nets.push((name, rx, tx));
        }
    }

    pub(super) fn out(&self) -> String {
        if self.os != "Linux" {
            return format!("@@OS\n{}\n@@END\n", self.os);
        }
        let cpu = |name: &str, t: &[u64; 8]| format!("{name} {} {} {} {} {} {} {} {} 0 0\n", t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7]);
        let mut s = format!("@@OS\nLinux\n@@UPTIME\n{:.2} {:.2}\n@@STAT\n", self.uptime, self.uptime * 3.0);
        s += &cpu("cpu ", &self.cpu);
        for (i, c) in self.cores.iter().enumerate() {
            s += &cpu(&format!("cpu{i}"), c);
        }
        s += "intr 12345\nctxt 999\n@@MEMINFO\n";
        for (k, v) in &self.mem {
            s += &format!("{k}:{v:>16} kB\n");
        }
        s += "HugePages_Total:       0\n@@LOADAVG\n1.42 1.18 0.92 3/412 99999\n@@PROCS\n187\n@@CPUFREQ\n2400000\n3600000\n@@NETDEV\nInter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n";
        for (n, rx, tx) in &self.nets {
            s += &format!("{n:>6}:{rx} {} 1 2 0 0 0 0 {tx} {} 3 4 0 0 0 0\n", rx / 1500, tx / 1500);
        }
        s += "@@NETIF\n";
        for l in &self.ifs {
            s += &format!("{l}\n");
        }
        for (m, n) in &self.lower {
            s += &format!("LOWER|{m}|{n}\n");
        }
        s += &format!("@@ROUTE\n{}\n@@DISKSTATS\n", self.route);
        for (n, d) in &self.disks {
            s += &format!("   8       0 {n} {} 0 {} {} {} 0 {} {} 0 {} 999\n", d[0], d[1], d[2], d[3], d[4], d[5], d[6]);
        }
        s += "@@BLOCK\n";
        for b in &self.block {
            s += &format!("{b}\n");
        }
        s += &format!("@@DF\n{}@@TOOLS\nss\nip\nsystemctl\n", self.df);
        if self.statics {
            s += "@@STATIC\nhostname=claude\nkernel=6.2.0-39-generic\narch=x86_64\nos=\"Ubuntu 22.04 LTS\"\nmax_khz=3600000\nbase_khz=2400000\ncache=L1 Data|32K\ncache=L3 Unified|20480K\n@@CPUINFO\nmodel=Intel(R) Xeon(R) CPU E5-2680 v4\nlogical=12\ncores_per_socket=6\nsockets=1\nvirt=vmx\n@@LSCPU\nModel name:            Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz\nSocket(s):             1\nCore(s) per socket:    6\nThread(s) per core:    2\nCPU(s):                12\nVirtualization:        VT-x\nHypervisor vendor:     KVM\nL3 cache:              20 MiB (1 instance)\n";
        }
        s + "@@END\n"
    }

    fn raw(&self) -> Raw {
        parse(&self.out()).expect("fixture parses")
    }
}

pub(super) fn host_id() -> Uuid {
    Uuid::from_u128(7)
}

/// Two readings `dt` seconds of host time apart, the second after `change` has been applied to a copy of the first.
fn pair(change: impl FnOnce(&mut Fx), dt: f64) -> Sample {
    let a = Fx::host();
    let mut b = a.clone().at(a.uptime + dt);
    change(&mut b);
    derive(host_id(), 0, Some(&a.raw()), &b.raw(), &StaticInfo::default()).0
}

fn net_of<'a>(s: &'a Sample, name: &str) -> &'a NetIface {
    s.network.as_ref().unwrap().interfaces.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("no interface {name}"))
}

// -- the first reading, and the baseline -----------------------------------------------------------------------------

#[test]
fn the_first_reading_has_values_but_no_rates_and_not_zeros() {
    let (s, keep) = derive(host_id(), 5, None, &Fx::host().raw(), &StaticInfo::default());
    assert_eq!(s.baseline, Baseline::First);
    assert!(keep, "it becomes the baseline");
    let cpu = s.cpu.as_ref().unwrap();
    assert_eq!(cpu.total_pct, None, "no baseline: unavailable, not 0%");
    assert!(cpu.cores.len() == 2 && cpu.cores.iter().all(|c| c.pct.is_none()));
    assert_eq!((cpu.iowait_pct, cpu.steal_pct), (None, None));
    let eth = net_of(&s, "eth0");
    assert_eq!((eth.rx_bps, eth.tx_bps, eth.rx_pps), (None, None, None));
    assert_eq!(s.network.as_ref().unwrap().total, None);
    // What needs no baseline is there.
    assert_eq!((eth.rx_bytes, eth.tx_bytes), (1_000_000, 500_000));
    assert!(s.memory.is_some() && s.load.is_some() && !s.filesystems.is_empty());
    let sda = s.disks.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!((sda.read_bps, sda.busy_pct, sda.await_ms), (None, None, None));
    assert_eq!(s.uptime_secs, Some(1000.0));
}

#[test]
fn baselines_are_told_apart_by_the_hosts_own_uptime() {
    let a = Fx::host().raw();
    let b = |u: f64| Fx::host().at(u).raw();
    assert_eq!(baseline(None, &a), Baseline::First);
    assert_eq!(baseline(Some(&a), &b(1002.0)), Baseline::Measured { elapsed_secs: 2.0 });
    assert_eq!(baseline(Some(&a), &b(500.0)), Baseline::Rebooted);
    assert!(matches!(baseline(Some(&a), &b(1000.05)), Baseline::TooClose));
    assert!(matches!(baseline(Some(&a), &b(2000.0)), Baseline::Stale { gap_secs } if gap_secs == 1000.0));
    let mut no_uptime = Fx::host().raw();
    no_uptime.uptime = None;
    assert_eq!(baseline(Some(&a), &no_uptime), Baseline::First, "without the host's clock there is nothing to measure against");
}

#[test]
fn a_reading_too_close_to_the_last_is_not_the_new_baseline() {
    let a = Fx::host().raw();
    let (s, keep) = derive(host_id(), 0, Some(&a), &Fx::host().at(1000.05).raw(), &StaticInfo::default());
    assert_eq!(s.baseline, Baseline::TooClose);
    assert!(!keep, "measuring 50 ms later would make every rate noise");
    assert_eq!(s.cpu.unwrap().total_pct, None);
}

#[test]
fn after_a_reboot_or_a_long_gap_nothing_is_a_rate() {
    for (uptime, want) in [(30.0, "rebooted"), (5000.0, "stale")] {
        let a = Fx::host().raw();
        let mut b = Fx::host().at(uptime);
        b.net("eth0", 10, 10); // would be a reset on its own
        let (s, keep) = derive(host_id(), 0, Some(&a), &b.raw(), &StaticInfo::default());
        match (&s.baseline, want) {
            (Baseline::Rebooted, "rebooted") | (Baseline::Stale { .. }, "stale") => {}
            other => panic!("{other:?}"),
        }
        assert!(keep, "the new reading starts the next baseline");
        assert_eq!(s.cpu.as_ref().unwrap().total_pct, None);
        assert_eq!(net_of(&s, "eth0").rx_bps, None);
        assert_eq!(net_of(&s, "eth0").rx_bytes, 10, "the counter itself is shown");
    }
}

// -- CPU -------------------------------------------------------------------------------------------------------------

#[test]
fn cpu_is_the_busy_share_of_the_change_in_the_counters() {
    // 2 s later: +250 busy (user 200, system 50), +750 idle: 25%.
    let s = pair(|b| {
        b.cpu = [1200, 0, 550, 8750, 100, 0, 0, 0];
        b.cores = vec![[600, 0, 275, 4500, 50, 0, 0, 0], [600, 0, 275, 4250, 50, 0, 0, 0]];
    }, 2.0);
    let cpu = s.cpu.unwrap();
    assert_eq!(cpu.total_pct, Some(25.0));
    // core 0: +125 busy, +500 idle => 20%; core 1: +125 busy, +250 idle => 33.3%
    assert_eq!(cpu.cores[0].pct, Some(20.0));
    assert!((cpu.cores[1].pct.unwrap() - 33.333).abs() < 0.01);
    assert_eq!(cpu.logical, 2);
    assert_eq!(cpu.freq_mhz, Some(3000.0), "the mean of the cores that report one");
}

#[test]
fn iowait_is_idle_and_steal_is_busy() {
    // +100 iowait, +100 idle, +50 steal, +50 user: busy = 100 of 300.
    let s = pair(|b| b.cpu = [1050, 0, 500, 8100, 200, 0, 0, 50], 2.0);
    let cpu = s.cpu.unwrap();
    assert!((cpu.total_pct.unwrap() - 100.0 * 100.0 / 300.0).abs() < 0.01);
    assert!((cpu.iowait_pct.unwrap() - 100.0 / 3.0).abs() < 0.01);
    assert!((cpu.steal_pct.unwrap() - 100.0 * 50.0 / 300.0).abs() < 0.01);
}

#[test]
fn iowait_going_backwards_does_not_invalidate_a_reading_but_other_counters_do() {
    // The kernel documents iowait going down.
    let s = pair(|b| b.cpu = [1100, 0, 500, 8400, 50, 0, 0, 0], 2.0);
    assert!(s.cpu.unwrap().total_pct.is_some());
    // user going down is a reset.
    let s = pair(|b| b.cpu = [10, 0, 500, 8400, 100, 0, 0, 0], 2.0);
    assert_eq!(s.cpu.unwrap().total_pct, None);
    // No ticks at all in the interval: nothing to divide by.
    let s = pair(|_| {}, 2.0);
    assert_eq!(s.cpu.unwrap().total_pct, None, "a frozen counter is unavailable, not an idle 0%");
}

#[test]
fn a_core_that_appeared_has_no_figure_yet() {
    let s = pair(|b| {
        b.cpu = [1200, 0, 550, 8750, 100, 0, 0, 0];
        b.cores.push([1, 0, 1, 1, 0, 0, 0, 0]);
    }, 2.0);
    let cores = s.cpu.unwrap().cores;
    assert_eq!(cores.len(), 3);
    assert_eq!(cores[2].pct, None);
}

// -- memory ----------------------------------------------------------------------------------------------------------

#[test]
fn memory_uses_the_kernels_available_figure_and_does_not_count_shared_as_cache() {
    let m = Fx::host().raw();
    let m = derive(host_id(), 0, None, &m, &StaticInfo::default()).0.memory.unwrap();
    let kb = 1024;
    assert_eq!(m.total, 16_000_000 * kb);
    assert_eq!(m.available, 8_000_000 * kb);
    assert_eq!(m.used, 8_000_000 * kb, "total - available");
    assert_eq!(m.used_pct, 50.0);
    // Cached 6.0M + reclaimable slab 0.5M, less shared 1.0M that cannot be dropped.
    assert_eq!(m.cached, 5_500_000 * kb);
    assert_eq!(m.shared, 1_000_000 * kb);
    assert_eq!((m.swap_total, m.swap_used), (4_000_000 * kb, 1_000_000 * kb));
    assert_eq!((m.committed, m.slab), (Some(9_000_000 * kb), Some(800_000 * kb)));
    assert!(m.available_exact);
}

#[test]
fn memory_on_an_old_kernel_is_estimated_and_says_so() {
    let mut f = Fx::host();
    f.mem.retain(|(k, _)| *k != "MemAvailable");
    f.mem.retain(|(k, _)| *k != "Committed_AS");
    let m = derive(host_id(), 0, None, &f.raw(), &StaticInfo::default()).0.memory.unwrap();
    // free 2.0M + buffers 0.5M + cached(5.5M)
    assert_eq!(m.available, 8_000_000 * 1024);
    assert!(!m.available_exact);
    assert_eq!(m.committed, None, "an absent figure is unavailable, not 0");
}

#[test]
fn a_host_without_swap_has_zero_swap_and_without_meminfo_no_memory() {
    let mut f = Fx::host();
    f.mem.retain(|(k, _)| !k.starts_with("Swap"));
    let m = derive(host_id(), 0, None, &f.raw(), &StaticInfo::default()).0.memory.unwrap();
    assert_eq!((m.swap_total, m.swap_used), (0, 0));
    f.mem.clear();
    let s = derive(host_id(), 0, None, &f.raw(), &StaticInfo::default()).0;
    assert!(s.memory.is_none());
    assert!(!s.capabilities.meminfo);
}

// -- network ---------------------------------------------------------------------------------------------------------

#[test]
fn network_speed_is_bytes_over_the_hosts_clock_and_totals_stay_totals() {
    let s = pair(|b| b.net("eth0", 1_000_000 + 2_000_000, 500_000 + 600_000), 2.0);
    let eth = net_of(&s, "eth0");
    assert_eq!((eth.rx_bps, eth.tx_bps), (Some(1_000_000.0), Some(300_000.0)));
    assert_eq!((eth.rx_bytes, eth.tx_bytes), (3_000_000, 1_100_000), "since boot");
    assert!((eth.rx_pps.unwrap() - 2_000_000.0 / 1500.0 / 2.0).abs() < 1.0);
    assert_eq!((eth.rx_errors, eth.tx_drops), (1, 4));
    assert_eq!(eth.state, "up");
    let total = s.network.unwrap().total.unwrap();
    assert_eq!((total.rx_bps, total.tx_bps, total.from), (1_000_000.0, 300_000.0, vec!["eth0".to_string()]));
}

#[test]
fn an_idle_interface_is_a_real_zero_not_unavailable() {
    let s = pair(|_| {}, 2.0);
    let eth = net_of(&s, "eth0");
    assert_eq!((eth.rx_bps, eth.tx_bps), (Some(0.0), Some(0.0)));
    assert_eq!(s.network.unwrap().total.unwrap().rx_bps, 0.0);
}

#[test]
fn a_counter_that_went_backwards_has_no_rate_for_that_direction_only() {
    let s = pair(|b| b.net("eth0", 100, 500_000 + 1000), 2.0);
    let eth = net_of(&s, "eth0");
    assert_eq!(eth.rx_bps, None, "a reset is not a speed");
    assert_eq!(eth.tx_bps, Some(500.0));
    assert_eq!(s.network.unwrap().total, None, "a total with a missing direction would lie");
}

#[test]
fn a_32_bit_counter_that_wrapped_is_carried_over() {
    assert_eq!(delta(10, 15), Some(5));
    // Near 2^32 and now near zero: it wrapped.
    assert_eq!(delta(u64::from(u32::MAX) - 99, 900), Some(1000));
    // A big 64-bit counter dropping to a small one is a reset.
    assert_eq!(delta(50_000_000_000, 900), None);
    // Below the top by more than the window: also a reset.
    assert_eq!(delta(1_000_000, 10), None);
    let mut a = Fx::host();
    a.net("eth0", u64::from(u32::MAX) - 1000, 0);
    let mut b = a.clone().at(1002.0);
    b.net("eth0", 3000, 0);
    let s = derive(host_id(), 0, Some(&a.raw()), &b.raw(), &StaticInfo::default()).0;
    assert_eq!(net_of(&s, "eth0").rx_bps, Some(2000.5), "(4001 bytes) / 2 s");
}

#[test]
fn an_interface_that_was_recreated_or_renamed_has_no_rate_on_its_first_reading() {
    // Recreated under the same name: new index, counters back near zero (and here not even lower).
    let mut a = Fx::host();
    a.net("eth0", 10, 10);
    let mut b = a.clone().at(1002.0);
    b.net("eth0", 20, 20);
    b.ifs[1] = iface("eth0", "up", 9, 1, "", "");
    let s = derive(host_id(), 0, Some(&a.raw()), &b.raw(), &StaticInfo::default()).0;
    assert_eq!(net_of(&s, "eth0").rx_bps, None);
    // Renamed: the new name has no history, the old one is gone, and nothing is carried over by guessing.
    let mut c = Fx::host().at(1002.0);
    c.nets = vec![("lo", 5000, 5000), ("ens18", 2_000_000, 600_000)];
    c.ifs = vec![iface("lo", "unknown", 1, 0, "", ""), iface("ens18", "up", 2, 1, "", "")];
    c.route = "ens18";
    let s = derive(host_id(), 0, Some(&Fx::host().raw()), &c.raw(), &StaticInfo::default()).0;
    let n = s.network.unwrap();
    assert_eq!(n.interfaces.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["ens18"]);
    assert_eq!(n.interfaces[0].rx_bps, None);
    assert_eq!(n.default_iface.as_deref(), Some("ens18"));
}

#[test]
fn a_removed_interface_just_disappears_and_a_new_one_has_no_rate_and_no_total_until_it_does() {
    let mut a = Fx::host();
    a.nets.push(("eth1", 1000, 1000));
    a.ifs.push(iface("eth1", "up", 3, 1, "", ""));
    let b = Fx::host().at(1002.0); // eth1 gone
    let s = derive(host_id(), 0, Some(&a.raw()), &b.raw(), &StaticInfo::default()).0;
    assert_eq!(s.network.as_ref().unwrap().interfaces.len(), 1);
    assert_eq!(s.network.unwrap().total.unwrap().from, ["eth0"]);
    // eth1 appears: it is counted, has no rate, so no total this time rather than a low one.
    let s = derive(host_id(), 0, Some(&Fx::host().raw()), &a.clone().at(1002.0).raw(), &StaticInfo::default()).0;
    assert_eq!(net_of(&s, "eth1").rx_bps, None);
    assert_eq!(s.network.unwrap().total, None);
}

#[test]
fn the_same_traffic_is_never_counted_twice() {
    let mut f = Fx::host();
    f.nets = vec![("lo", 1, 1), ("eth0", 1000, 1000), ("eth1", 2000, 2000), ("eth2", 3000, 3000), ("bond0", 5000, 5000), ("br0", 9000, 9000), ("eth3", 100, 100), ("docker0", 7000, 7000), ("veth1a", 7000, 7000), ("wg0", 400, 400), ("tailscale0", 400, 400), ("eth0.100", 4000, 4000)];
    f.ifs = vec![
        iface("lo", "unknown", 1, 0, "", ""),
        iface("eth0", "up", 2, 1, "", ""),
        iface("eth1", "up", 3, 1, "", ""),
        iface("eth2", "up", 4, 1, "", ""),
        iface("bond0", "up", 5, 0, "bond", "bond"),
        iface("br0", "up", 6, 0, "bridge", "bridge"),
        iface("eth3", "up", 7, 1, "", ""),
        iface("docker0", "up", 8, 0, "bridge", "bridge"),
        iface("veth1a", "up", 9, 0, "veth", ""),
        iface("wg0", "unknown", 10, 0, "wireguard", ""),
        iface("tailscale0", "unknown", 11, 0, "", "tun"),
        iface("eth0.100", "up", 12, 0, "vlan", ""),
    ];
    // eth1 and eth2 are in bond0; eth3 is a port of br0.
    f.lower = vec![("bond0", "eth1"), ("bond0", "eth2"), ("br0", "eth3"), ("docker0", "veth1a"), ("eth0", "eth0.100")];
    let mut g = f.clone().at(1002.0);
    for n in g.nets.iter_mut() {
        n.1 += 2000;
        n.2 += 2000;
    }
    let s = derive(host_id(), 0, Some(&f.raw()), &g.raw(), &StaticInfo::default()).0;
    let kind = |n: &str| net_of(&s, n).kind;
    assert_eq!((kind("eth0"), kind("bond0"), kind("br0"), kind("docker0"), kind("veth1a")), (IfaceKind::Physical, IfaceKind::Bond, IfaceKind::Bridge, IfaceKind::Bridge, IfaceKind::Container));
    assert_eq!((kind("wg0"), kind("tailscale0"), kind("eth0.100")), (IfaceKind::Tunnel, IfaceKind::Tunnel, IfaceKind::Vlan));
    assert_eq!(net_of(&s, "eth1").master.as_deref(), Some("bond0"));
    assert_eq!(net_of(&s, "eth3").master.as_deref(), Some("br0"));
    assert_eq!(net_of(&s, "eth0.100").master, None, "a VLAN's lower device is not a master");
    let total = s.network.unwrap().total.unwrap();
    // The physical NIC alone, the bond (not its members), and the physical port under the bridge (not the bridge).
    assert_eq!(total.from, ["bond0", "eth0", "eth3"]);
    assert_eq!(total.rx_bps, 3.0 * 1000.0);
}

#[test]
fn interface_kinds_come_from_the_kernel_first_and_the_name_only_as_a_fallback() {
    let m = |dev: bool, devtype: &str, flags: &[&str], t: Option<u32>| super::parse::IfMeta { state: "up".into(), mtu: None, speed_mbps: None, arp_type: t, ifindex: None, has_device: dev, devtype: devtype.into(), flags: flags.iter().map(|s| s.to_string()).collect() };
    assert_eq!(classify("lo", None), IfaceKind::Loopback);
    assert_eq!(classify("wlan0", Some(&m(true, "wlan", &["wireless"], Some(1)))), IfaceKind::Wireless);
    assert_eq!(classify("enp1s0", Some(&m(true, "", &[], Some(1)))), IfaceKind::Physical);
    // A real NIC with an unlucky name is still a NIC.
    assert_eq!(classify("wg-nic0", Some(&m(true, "", &[], Some(1)))), IfaceKind::Physical);
    assert_eq!(classify("br-1234", Some(&m(false, "bridge", &["bridge"], Some(1)))), IfaceKind::Bridge);
    assert_eq!(classify("tun0", Some(&m(false, "", &["tun"], Some(65534)))), IfaceKind::Tunnel);
    assert_eq!(classify("zt1", None), IfaceKind::Tunnel);
    assert_eq!(classify("veth0", None), IfaceKind::Container);
    assert_eq!(classify("weird0", Some(&m(false, "", &[], Some(1)))), IfaceKind::Other);
}

// -- disks and filesystems -------------------------------------------------------------------------------------------

#[test]
fn disk_activity_is_per_whole_device_and_busy_is_not_capacity() {
    // 2 s later on sda: +100 reads (+2000 sectors, 100 ms), +300 writes (+6000 sectors, 400 ms), 500 ms busy.
    let s = pair(|b| b.disks[0].1 = [200, 4000, 150, 500, 10000, 500, 620], 2.0);
    let names: Vec<_> = s.disks.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["dm-0", "sda"], "no partitions, no loop devices");
    let sda = s.disks.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!(sda.read_bps, Some(2000.0 * 512.0 / 2.0));
    assert_eq!(sda.write_bps, Some(6000.0 * 512.0 / 2.0));
    assert_eq!((sda.read_iops, sda.write_iops), (Some(50.0), Some(150.0)));
    assert_eq!(sda.busy_pct, Some(500.0 / 2000.0 * 100.0));
    assert_eq!(sda.await_ms, Some((100.0 + 400.0) / 400.0));
    assert_eq!((sda.model.as_deref(), sda.size, sda.rotational), (Some("Samsung SSD 860"), Some(2_000_000_000 * 512), Some(false)));
    assert!(!sda.virtual_device);
    assert!(s.disks.iter().find(|d| d.name == "dm-0").unwrap().virtual_device, "a layer above real disks, not summed with them");
    assert_eq!(sda.read_total, 4000 * 512);
}

#[test]
fn an_idle_disk_is_busy_zero_but_has_no_latency_to_report() {
    let s = pair(|_| {}, 2.0);
    let sda = s.disks.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!((sda.busy_pct, sda.read_bps, sda.write_iops), (Some(0.0), Some(0.0), Some(0.0)));
    assert_eq!(sda.await_ms, None);
}

#[test]
fn a_disk_counter_reset_leaves_that_disk_without_rates() {
    let s = pair(|b| b.disks[0].1 = [1, 1, 1, 1, 1, 1, 1], 2.0);
    let sda = s.disks.iter().find(|d| d.name == "sda").unwrap();
    assert_eq!((sda.read_bps, sda.busy_pct, sda.await_ms), (None, None, None));
    // The others are unaffected.
    assert_eq!(s.disks.iter().find(|d| d.name == "dm-0").unwrap().busy_pct, Some(0.0));
}

#[test]
fn filesystems_are_how_full_not_how_busy_and_pseudo_ones_are_left_out() {
    let s = Fx::host().raw();
    let fs = derive(host_id(), 0, None, &s, &StaticInfo::default()).0.filesystems;
    assert_eq!(fs.len(), 1, "tmpfs and a snap's squashfs are not storage: {fs:?}");
    let root = &fs[0];
    assert_eq!((root.device.as_str(), root.mount.as_str(), root.fstype.as_deref()), ("/dev/sda2", "/", Some("ext4")));
    assert_eq!((root.total, root.used, root.available), (123_000_000 * 1024, 92_000_000 * 1024, 26_000_000 * 1024));
    // df's own way: used / (used + available), so reserved blocks are not "free".
    assert!((root.used_pct - 92.0 * 100.0 / 118.0).abs() < 1e-9);
}

#[test]
fn df_without_a_type_column_mounts_with_spaces_and_bind_mounts() {
    let mut f = Fx::host();
    f.df = "Filesystem     1024-blocks    Used Available Capacity Mounted on\n/dev/sdb1          1000000  500000    500000      50% /mnt/my disk\n/dev/sdb1          1000000  500000    500000      50% /mnt/bind\n//nas/share        2000000  100000   1900000       5% /mnt/nas\nnone               1000      0     1000       0% /run/x\n/dev/loop4          100    100        0     100% /snap/y\n";
    let fs = derive(host_id(), 0, None, &f.raw(), &StaticInfo::default()).0.filesystems;
    assert_eq!(fs.iter().map(|x| x.mount.as_str()).collect::<Vec<_>>(), ["/mnt/my disk", "/mnt/nas"]);
    assert_eq!(fs[0].fstype, None);
    assert_eq!(fs[0].used_pct, 50.0);
    f.df = "";
    assert!(derive(host_id(), 0, None, &f.raw(), &StaticInfo::default()).0.filesystems.is_empty());
}

// -- load, processes, capabilities, unsupported ---------------------------------------------------------------------

#[test]
fn load_and_process_counts_and_capabilities_are_read_from_the_same_run() {
    let s = derive(host_id(), 0, None, &Fx::host().raw(), &StaticInfo::default()).0;
    assert_eq!(s.load, Some(Load { m1: 1.42, m5: 1.18, m15: 0.92 }));
    assert_eq!(s.processes, Some(ProcessCounts { processes: Some(187), threads: Some(412), running: Some(3) }));
    let c = &s.capabilities;
    assert!(c.proc_stat && c.meminfo && c.loadavg && c.net_dev && c.diskstats && c.sys_block && c.sys_net && c.filesystems && c.cpufreq);
    assert!(c.has_tool("ss") && c.has_tool("systemctl") && !c.has_tool("nvidia-smi"));
    assert_eq!(s.platform, Platform::Linux);
}

#[test]
fn a_host_missing_pieces_reports_what_it_has_and_what_it_lacks() {
    let mut f = Fx::host();
    f.disks.clear();
    f.nets.clear();
    let s = derive(host_id(), 0, None, &f.raw(), &StaticInfo::default()).0;
    assert!(s.disks.is_empty() && s.network.is_none());
    assert!(!s.capabilities.diskstats && !s.capabilities.net_dev);
    assert!(s.cpu.is_some() && s.memory.is_some());
}

#[test]
fn another_operating_system_is_unsupported_and_nothing_about_it_is_guessed() {
    let (s, keep) = derive(host_id(), 0, None, &Fx { os: "Darwin", ..Fx::host() }.raw(), &StaticInfo::default());
    assert_eq!(s.platform, Platform::Unsupported { os: "Darwin".into() });
    assert!(!keep);
    assert!(s.cpu.is_none() && s.memory.is_none() && s.network.is_none() && s.load.is_none() && s.filesystems.is_empty());
}

// -- reading the output ----------------------------------------------------------------------------------------------

#[test]
fn output_that_was_cut_short_or_is_empty_or_huge_is_an_error_not_a_short_reading() {
    let full = Fx::host().out();
    let cut = full.replace("@@END\n", "");
    assert_eq!(parse(&cut), Err(ParseError::Incomplete));
    assert_eq!(parse(""), Err(ParseError::Empty));
    assert_eq!(parse("  \n"), Err(ParseError::Empty));
    assert_eq!(parse(&"x".repeat(3 * 1024 * 1024)), Err(ParseError::TooLarge));
}

#[test]
fn junk_lines_and_bad_numbers_are_skipped() {
    let mut out = Fx::host().out();
    out = out.replace("@@NETDEV\n", "@@NETDEV\nwe!rd: 1 2 3\ngarbage line\n   eth9:notanumber 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1\n");
    out = out.replace("@@STAT\n", "@@STAT\ncpuX 1 2 3\ncpu99 a b c d\n");
    out.insert_str(0, "Welcome to the host!\nLast login: yesterday\n");
    let raw = parse(&out).unwrap();
    assert_eq!(raw.net.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["lo", "eth0"]);
    assert_eq!(raw.cpu_cores.len(), 2);
    assert!(raw.cpu_total.is_some());
}

#[test]
fn a_long_interface_name_that_touches_its_counters_is_read() {
    let mut f = Fx::host();
    f.nets.push(("enp0s31f6", 123, 456));
    let line = f.out().lines().find(|l| l.contains("enp0s31f6:")).unwrap().to_string();
    assert!(line.contains("enp0s31f6:123"), "the fixture prints it the way the kernel does: {line}");
    assert!(f.raw().net.iter().any(|n| n.0 == "enp0s31f6" && n.1.rx_bytes == 123));
}

#[test]
fn static_facts_are_read_from_lscpu_over_cpuinfo() {
    let mut f = Fx::host();
    f.statics = true;
    let info = f.raw().info.unwrap();
    assert_eq!(info.hostname.as_deref(), Some("claude"));
    assert_eq!(info.os_name.as_deref(), Some("Ubuntu 22.04 LTS"));
    assert_eq!((info.kernel.as_deref(), info.arch.as_deref()), (Some("6.2.0-39-generic"), Some("x86_64")));
    assert_eq!(info.cpu_model.as_deref(), Some("Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz"));
    assert_eq!((info.sockets, info.physical_cores, info.logical_cpus), (Some(1), Some(6), Some(12)));
    assert_eq!((info.max_mhz, info.base_mhz), (Some(3600.0), Some(2400.0)));
    assert_eq!((info.virtualization.as_deref(), info.hypervisor.as_deref()), (Some("VT-x"), Some("KVM")));
    assert_eq!(info.caches, vec![CpuCache { level: "L3".into(), size: "20 MiB".into() }]);
    // Without lscpu, /proc/cpuinfo still gives the model and counts.
    let cut = f.out().replace("Model name:", "Nope:").replace("Socket(s):", "X:").replace("Core(s) per socket:", "Y:").replace("CPU(s):", "Z:").replace("Virtualization:        VT-x\n", "");
    let info = parse(&cut).unwrap().info.unwrap();
    assert_eq!(info.cpu_model.as_deref(), Some("Intel(R) Xeon(R) CPU E5-2680 v4"));
    assert_eq!((info.sockets, info.physical_cores, info.logical_cpus), (Some(1), Some(6), Some(12)));
    assert_eq!(info.virtualization.as_deref(), Some("vmx"));
    // A reading without the static part has none.
    assert!(Fx::host().raw().info.is_none());
}

#[test]
fn the_script_only_reads() {
    for with_static in [false, true] {
        let s = script::script(with_static);
        assert!(s.contains("@@END") && s.starts_with("LC_ALL=C"));
        // Every `>` is a redirect to /dev/null or to another descriptor: nothing is written anywhere.
        for (i, _) in s.match_indices('>') {
            let rest = s[i + 1..].trim_start();
            assert!(rest.starts_with("/dev/null") || rest.starts_with('&'), "a write in the script near {:?}", &s[i.saturating_sub(30)..(i + 30).min(s.len())]);
        }
        for bad in ["rm ", "mv ", "chmod", "sudo", "mkdir", "tee ", " dd ", "kill ", "systemctl restart", "curl", "wget"] {
            assert!(!s.contains(bad), "{bad} in the script");
        }
    }
    assert!(!script::script(false).contains("@@STATIC"));
    assert!(script::script(true).contains("@@STATIC"));
}

// -- this machine ----------------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn the_real_script_runs_here_and_two_readings_give_real_rates() {
    use std::process::Command;
    if !std::path::Path::new("/proc/stat").exists() {
        return;
    }
    let run = |with_static: bool| {
        let out = Command::new("sh").arg("-c").arg(script::script(with_static)).output().expect("sh");
        assert!(out.stderr.is_empty(), "the script wrote to stderr: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let first = parse(&run(true)).expect("this machine's first reading parses");
    assert!(first.linux && first.uptime.is_some());
    let info = first.info.clone().expect("static facts were asked for");
    std::thread::sleep(Duration::from_millis(1200));
    let second = parse(&run(false)).expect("and the second");
    let (s1, keep1) = derive(host_id(), 0, None, &first, &info);
    assert!(keep1 && s1.baseline == Baseline::First);
    let (s2, _) = derive(host_id(), 0, Some(&first), &second, &info);
    assert!(matches!(s2.baseline, Baseline::Measured { elapsed_secs } if (1.0..10.0).contains(&elapsed_secs)), "{:?}", s2.baseline);
    let cpu = s2.cpu.as_ref().unwrap();
    assert!(cpu.total_pct.is_some_and(|p| (0.0..=100.0).contains(&p)), "{cpu:?}");
    assert!(!cpu.cores.is_empty() && cpu.cores.iter().all(|c| c.pct.is_some()));
    let mem = s2.memory.as_ref().unwrap();
    assert!(mem.total > 0 && mem.used <= mem.total && mem.available <= mem.total);
    assert!(s2.load.is_some() && s2.processes.as_ref().is_some_and(|p| p.processes.unwrap_or(0) > 0));
    assert!(!s2.filesystems.is_empty(), "this machine has a root filesystem");
    assert!(s2.filesystems.iter().all(|f| f.used_pct >= 0.0 && f.used_pct <= 100.0 && f.total >= f.used));
    if let Some(n) = &s2.network {
        assert!(n.interfaces.iter().all(|i| i.name != "lo"));
        if let Some(t) = &n.total {
            assert!(t.rx_bps >= 0.0 && t.tx_bps >= 0.0, "{t:?}");
        }
    }
    // Whole devices only: no partition shows up beside its disk.
    for d in &s2.disks {
        assert!(!d.name.starts_with("loop"), "{}", d.name);
    }
    assert!(info.cpu_model.is_some() || info.logical_cpus.is_some(), "{info:?}");
    assert!(info.kernel.is_some());
    assert!(s2.capabilities.proc_stat && s2.capabilities.meminfo);
}

// -- the manager -----------------------------------------------------------------------------------------------------

pub(super) type Handler = Box<dyn Fn(Uuid, &str) -> BoxFuture<'static, Result<String, String>> + Send + Sync>;

pub(super) struct Fake {
    handler: Handler,
    calls: AtomicUsize,
    pub(super) scripts: Mutex<Vec<String>>,
}

impl Fake {
    pub(super) fn new(handler: impl Fn(Uuid, &str) -> BoxFuture<'static, Result<String, String>> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Fake { handler: Box::new(handler), calls: AtomicUsize::new(0), scripts: Mutex::new(Vec::new()) })
    }

    /// A host that answers with these readings, one per call, then keeps repeating the last.
    pub(super) fn scripted(readings: Vec<Result<String, String>>) -> Arc<Self> {
        let q = Mutex::new(VecDeque::from(readings));
        Fake::new(move |_, _| {
            let r = {
                let mut q = q.lock().unwrap();
                if q.len() > 1 {
                    q.pop_front().unwrap()
                } else {
                    q.front().cloned().unwrap()
                }
            };
            Box::pin(async move { r })
        })
    }

    pub(super) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl HostExec for Fake {
    fn exec<'a>(&'a self, host: Uuid, script: &'a str, _limit: Duration) -> BoxFuture<'a, Result<String, String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.scripts.lock().unwrap().push(script.to_string());
        (self.handler)(host, script)
    }
}

pub(super) fn mgr(fake: &Arc<Fake>) -> HostMetricsManager<Fake> {
    HostMetricsManager::new(Arc::clone(fake), Config::default(), None)
}

pub(super) const SEC: Duration = Duration::from_secs(1);

#[tokio::test(start_paused = true)]
async fn callers_asking_at_once_share_one_collection_and_recent_samples_are_reused() {
    let fake = Fake::new(|_, _| Box::pin(async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(Fx::host().out())
    }));
    let m = Arc::new(mgr(&fake));
    let id = host_id();
    // The dashboard, the host window and the terminal panel all ask in the same moment.
    let asks: Vec<_> = (0..10).map(|_| { let m = Arc::clone(&m); tokio::spawn(async move { m.sample(id, Duration::from_secs(2)).await }) }).collect();
    let mut got = Vec::new();
    for a in asks {
        got.push(a.await.unwrap().unwrap());
    }
    assert_eq!(fake.calls(), 1, "ten views, one SSH round trip");
    assert!(got.iter().all(|s| Arc::ptr_eq(s, &got[0])), "they all hold the same sample");
    assert_eq!(m.collections(), 1);

    // Within the caller's max_age: no new collection. Asking for something fresher than the minimum interval is not
    // possible: it is answered from the cache too.
    tokio::time::advance(Duration::from_millis(500)).await;
    m.sample(id, Duration::ZERO).await.unwrap();
    tokio::time::advance(Duration::from_millis(1000)).await;
    m.sample(id, Duration::from_secs(2)).await.unwrap();
    assert_eq!(fake.calls(), 1);
    // Past it: a new one.
    tokio::time::advance(Duration::from_secs(2)).await;
    m.sample(id, Duration::from_secs(2)).await.unwrap();
    assert_eq!(fake.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn the_second_sample_has_rates_computed_from_the_first_and_a_reboot_restarts_them() {
    let mut a = Fx::host();
    a.net("eth0", 1_000_000, 0);
    let mut b = a.clone().at(1002.0);
    b.net("eth0", 3_000_000, 0);
    let rebooted = Fx::host().at(20.0);
    let mut after = rebooted.clone().at(22.0);
    after.net("eth0", 1_000_000 + 4_000_000, 500_000);
    let fake = Fake::scripted(vec![Ok(a.out()), Ok(b.out()), Ok(rebooted.out()), Ok(after.out())]);
    let m = mgr(&fake);
    let id = host_id();
    let s1 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s1.baseline, Baseline::First);
    assert_eq!(s1.network.as_ref().unwrap().total, None);
    tokio::time::advance(Duration::from_secs(2)).await;
    let s2 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s2.baseline, Baseline::Measured { elapsed_secs: 2.0 });
    assert_eq!(s2.network.as_ref().unwrap().total.as_ref().unwrap().rx_bps, 1_000_000.0);
    tokio::time::advance(Duration::from_secs(2)).await;
    let s3 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s3.baseline, Baseline::Rebooted);
    assert_eq!(s3.network.as_ref().unwrap().total, None);
    assert_eq!(s3.cpu.as_ref().unwrap().total_pct, None);
    tokio::time::advance(Duration::from_secs(2)).await;
    let s4 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s4.baseline, Baseline::Measured { elapsed_secs: 2.0 }, "the post-reboot reading is the new baseline");
    assert_eq!(s4.network.as_ref().unwrap().total.as_ref().unwrap().rx_bps, 2_000_000.0);
    // The kept history has a point per reading, with the gaps as gaps.
    let h = m.history(id, 0);
    assert_eq!(h.len(), 4);
    assert_eq!((h[0].rx_bps, h[1].rx_bps, h[2].rx_bps, h[3].rx_bps), (None, Some(1_000_000.0), None, Some(2_000_000.0)));
}

#[tokio::test(start_paused = true)]
async fn only_so_many_hosts_are_read_at_once() {
    let live = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (l, p) = (Arc::clone(&live), Arc::clone(&peak));
    let fake = Fake::new(move |_, _| {
        let (l, p) = (Arc::clone(&l), Arc::clone(&p));
        Box::pin(async move {
            let now = l.fetch_add(1, Ordering::SeqCst) + 1;
            p.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100)).await;
            l.fetch_sub(1, Ordering::SeqCst);
            Ok(Fx::host().out())
        })
    });
    let m = Arc::new(HostMetricsManager::new(Arc::clone(&fake), Config { max_concurrent: 2, ..Config::default() }, None));
    let tasks: Vec<_> = (0..8u128).map(|i| { let m = Arc::clone(&m); tokio::spawn(async move { m.sample(Uuid::from_u128(i + 1), SEC).await }) }).collect();
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(fake.calls(), 8);
    assert_eq!(peak.load(Ordering::SeqCst), 2, "never more than the limit, and the limit is used");
    assert_eq!(m.host_count(), 8);
}

#[tokio::test(start_paused = true)]
async fn a_host_that_fails_is_left_alone_for_a_growing_time() {
    let fake = Fake::scripted(vec![Err("connection refused".into()), Err("connection refused".into()), Ok(Fx::host().out())]);
    let m = mgr(&fake);
    let id = host_id();
    let e = m.sample(id, SEC).await.unwrap_err();
    assert_eq!(e, MetricsError::Exec { message: "connection refused".into() });
    assert_eq!(fake.calls(), 1);
    // Asking again straight away does not hit the host again.
    let e = m.sample(id, SEC).await.unwrap_err();
    assert!(matches!(&e, MetricsError::Cooling { last, .. } if last == "connection refused"), "{e:?}");
    assert_eq!(fake.calls(), 1);
    // After the first back-off (2 s) it tries; it fails again, so the next wait is twice as long.
    tokio::time::advance(Duration::from_millis(2100)).await;
    m.sample(id, SEC).await.unwrap_err();
    assert_eq!(fake.calls(), 2);
    tokio::time::advance(Duration::from_millis(2100)).await;
    assert!(matches!(m.sample(id, SEC).await.unwrap_err(), MetricsError::Cooling { .. }), "4 s this time");
    assert_eq!(fake.calls(), 2);
    tokio::time::advance(Duration::from_millis(2100)).await;
    let ok = m.sample(id, SEC).await.unwrap();
    assert_eq!(fake.calls(), 3);
    assert_eq!(ok.baseline, Baseline::First, "nothing carried over from before the failures");
    // And a success clears the back-off.
    tokio::time::advance(Duration::from_secs(2)).await;
    m.sample(id, SEC).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_host_that_never_answers_times_out_and_garbage_is_unreadable() {
    let fake = Fake::new(|_, _| Box::pin(std::future::pending()));
    let m = HostMetricsManager::new(Arc::clone(&fake), Config { timeout: Duration::from_secs(5), ..Config::default() }, None);
    assert_eq!(m.sample(host_id(), SEC).await.unwrap_err(), MetricsError::Timeout);
    let fake = Fake::scripted(vec![Ok("<html>not a reading</html>".into())]);
    let m = mgr(&fake);
    assert!(matches!(m.sample(host_id(), SEC).await.unwrap_err(), MetricsError::Unreadable { .. }));
    let fake = Fake::scripted(vec![Ok(Fx::host().out().replace("@@END\n", ""))]);
    let e = mgr(&fake).sample(host_id(), SEC).await.unwrap_err();
    assert_eq!(e, MetricsError::Unreadable { message: "the host's answer was cut short".into() });
}

#[tokio::test(start_paused = true)]
async fn a_collection_can_be_cancelled_and_the_host_can_be_asked_again() {
    let hang = Arc::new(AtomicUsize::new(1));
    let h = Arc::clone(&hang);
    let fake = Fake::new(move |_, _| {
        let hang = h.load(Ordering::SeqCst) == 1;
        Box::pin(async move {
            if hang {
                std::future::pending::<()>().await;
            }
            Ok(Fx::host().out())
        })
    });
    let m = Arc::new(mgr(&fake));
    let id = host_id();
    let pending = { let m = Arc::clone(&m); tokio::spawn(async move { m.sample(id, SEC).await }) };
    tokio::time::sleep(Duration::from_millis(50)).await;
    m.cancel(id);
    assert_eq!(pending.await.unwrap().unwrap_err(), MetricsError::Cancelled);
    // A cancel is not the host's fault: no back-off.
    hang.store(0, Ordering::SeqCst);
    assert!(m.sample(id, SEC).await.is_ok());
    assert_eq!(fake.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn dropping_the_caller_stops_the_collection_and_frees_the_host() {
    let fake = Fake::new(|_, _| Box::pin(std::future::pending()));
    let m = Arc::new(mgr(&fake));
    let id = host_id();
    let t = { let m = Arc::clone(&m); tokio::spawn(async move { m.sample(id, SEC).await }) };
    tokio::time::sleep(Duration::from_millis(50)).await;
    t.abort();
    let _ = t.await;
    // The gate and the permit were released with the dropped future: the next caller gets its turn (and here times out).
    let m2 = HostMetricsManager::new(Arc::clone(&fake), Config { timeout: Duration::from_secs(1), ..Config::default() }, None);
    assert_eq!(m2.sample(id, SEC).await.unwrap_err(), MetricsError::Timeout);
    let r = tokio::time::timeout(Duration::from_secs(20), m.sample(id, SEC)).await;
    assert!(matches!(r, Ok(Err(MetricsError::Timeout))), "{r:?}");
}

#[tokio::test(start_paused = true)]
async fn clearing_forgets_everything_so_a_lock_leaves_nothing_behind() {
    let fake = Fake::scripted(vec![Ok(Fx::host().out())]);
    let m = mgr(&fake);
    let id = host_id();
    m.sample(id, SEC).await.unwrap();
    assert!(m.latest(id).is_some() && m.history(id, 0).len() == 1 && m.all_latest().len() == 1);
    m.clear();
    assert!(m.latest(id).is_none() && m.history(id, 0).is_empty() && m.all_latest().is_empty());
    assert_eq!(m.host_count(), 0);
    // The baseline went too: the next reading is a first one.
    assert_eq!(m.sample(id, SEC).await.unwrap().baseline, Baseline::First);
    m.forget(id);
    assert!(m.latest(id).is_none());
}

#[tokio::test(start_paused = true)]
async fn when_the_host_cannot_be_logged_in_to_nothing_is_cached_or_invented() {
    // What the app's connection says when the vault is locked or the host has no saved credentials.
    let fake = Fake::scripted(vec![Err("the vault is locked".into())]);
    let m = mgr(&fake);
    let e = m.sample(host_id(), SEC).await.unwrap_err();
    assert_eq!(e.to_string(), "the vault is locked");
    assert!(m.latest(host_id()).is_none() && m.history(host_id(), 0).is_empty());
}

#[tokio::test(start_paused = true)]
async fn static_facts_ride_along_once_then_hourly_and_survive_between() {
    let mut with = Fx::host();
    with.statics = true;
    let fake = Fake::scripted(vec![Ok(with.out()), Ok(Fx::host().at(1002.0).out()), Ok(Fx::host().at(5000.0).out())]);
    let m = mgr(&fake);
    let id = host_id();
    let s1 = m.sample(id, SEC).await.unwrap();
    assert!(fake.scripts.lock().unwrap()[0].contains("@@STATIC"));
    assert_eq!(s1.info.hostname.as_deref(), Some("claude"));
    tokio::time::advance(Duration::from_secs(2)).await;
    let s2 = m.sample(id, SEC).await.unwrap();
    assert!(!fake.scripts.lock().unwrap()[1].contains("@@STATIC"), "no need to ask again");
    assert_eq!(s2.info.cpu_model, s1.info.cpu_model, "kept from the earlier reading");
    tokio::time::advance(Duration::from_secs(3601)).await;
    m.sample(id, SEC).await.unwrap();
    assert!(fake.scripts.lock().unwrap()[2].contains("@@STATIC"), "an hour later it is read again");
}

#[tokio::test(start_paused = true)]
async fn history_is_bounded() {
    let fake = Fake::scripted(vec![Ok(Fx::host().out())]);
    let m = HostMetricsManager::new(Arc::clone(&fake), Config { history_len: 5, ..Config::default() }, None);
    for _ in 0..12 {
        m.sample(host_id(), SEC).await.unwrap();
        tokio::time::advance(Duration::from_secs(2)).await;
    }
    assert_eq!(m.history(host_id(), 0).len(), 5);
    assert_eq!(m.history(host_id(), u64::MAX).len(), 0);
}

#[tokio::test(start_paused = true)]
async fn the_event_sink_hears_of_real_collections_only() {
    #[derive(Default)]
    struct Sink {
        sampled: AtomicUsize,
        failed: AtomicUsize,
    }
    impl EventSink for Sink {
        fn sampled(&self, _: &Arc<Sample>) {
            self.sampled.fetch_add(1, Ordering::SeqCst);
        }
        fn failed(&self, _: Uuid, _: &MetricsError) {
            self.failed.fetch_add(1, Ordering::SeqCst);
        }
    }
    let sink = Arc::new(Sink::default());
    let fake = Fake::scripted(vec![Ok(Fx::host().out()), Err("boom".into())]);
    let m = HostMetricsManager::new(Arc::clone(&fake), Config::default(), Some(Arc::clone(&sink) as Arc<dyn EventSink>));
    m.sample(host_id(), SEC).await.unwrap();
    m.sample(host_id(), SEC).await.unwrap(); // cache hit
    assert_eq!(sink.sampled.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_secs(2)).await;
    m.sample(host_id(), SEC).await.unwrap_err();
    assert_eq!((sink.sampled.load(Ordering::SeqCst), sink.failed.load(Ordering::SeqCst)), (1, 1));
}

#[tokio::test(start_paused = true)]
async fn another_os_is_collected_as_unsupported_and_leaves_no_history() {
    let fake = Fake::scripted(vec![Ok(Fx { os: "FreeBSD", ..Fx::host() }.out())]);
    let m = mgr(&fake);
    let s = m.sample(host_id(), SEC).await.unwrap();
    assert_eq!(s.platform, Platform::Unsupported { os: "FreeBSD".into() });
    assert!(m.history(host_id(), 0).is_empty());
}
