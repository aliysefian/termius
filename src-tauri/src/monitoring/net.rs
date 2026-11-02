//! Network traffic of a host: how fast each interface is receiving and sending right now, and how much it has carried.
//!
//! The numbers come from the host's own interface counters (`/proc/net/dev`), read twice about a second apart; the
//! speed is the change in the counter divided by the change in the host's uptime between the two reads, so a slow SSH
//! connection doesn't skew it. Nothing is estimated: where the counters can't be read (anything without `/proc`, a
//! counter that went backwards) there is no number, and the window shows none.
//!
//! RX is what the interface received and TX what it sent, on the monitored host.

use serde::Serialize;

/// The shell that reads the counters, shared by the stand-alone sample and the metrics check (which reads them either
/// side of the second it already waits for the CPU figure).
macro_rules! read_counters {
    () => {
        r#"netread() { awk 'NR>2 { sub(/:/, " "); print $1, $2, $3, $4, $5, $10, $11, $12, $13 }' /proc/net/dev 2>/dev/null; }
"#
    };
}

/// What each interface is: its link state, whether it is a real device (not a bridge, a veth or a tunnel), and its
/// link speed in Mbit/s when the driver says.
macro_rules! read_meta {
    () => {
        r#"netmeta() {
  for d in /sys/class/net/*; do
    [ -e "$d" ] || continue
    n=${d##*/}
    st=$(cat "$d/operstate" 2>/dev/null)
    ph=0; [ -e "$d/device" ] && ph=1
    sp=$(cat "$d/speed" 2>/dev/null)
    echo "$n ${st:-unknown} $ph ${sp:--1}"
  done
}
"#
    };
}

/// The part of a script that takes the first reading.
macro_rules! net_first {
    () => {
        r#"if [ -r /proc/net/dev ]; then
  echo "@@NETA $(awk '{print $1}' /proc/uptime 2>/dev/null)"; netread
fi
"#
    };
}

/// The part that takes the second reading and prints what the interfaces are.
macro_rules! net_second {
    () => {
        r#"if [ -r /proc/net/dev ]; then
  echo "@@NETB $(awk '{print $1}' /proc/uptime 2>/dev/null)"; netread
  echo "@@NETIF"; netmeta
  echo "@@PRIMARY"; awk '$2=="00000000"{print $1; exit}' /proc/net/route 2>/dev/null
fi
"#
    };
}

pub(crate) use {net_first, net_second, read_counters, read_meta};

/// A stand-alone reading: the two reads with a second between them.
pub const SCRIPT: &str = concat!(read_counters!(), read_meta!(), net_first!(), "sleep 1\n", net_second!());

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetInterface {
    pub name: String,
    /// `up`, `down`, or `unknown` (the kernel's operstate).
    pub state: String,
    /// A real network device, not a bridge, a virtual pair, a tunnel or a loopback.
    pub physical: bool,
    /// What the driver reports, in megabits per second.
    pub link_mbps: Option<u32>,
    /// Bytes per second over the sample; `None` when the counters can't say.
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    pub rx_pps: Option<f64>,
    pub tx_pps: Option<f64>,
    /// Counters since the host started (or the interface appeared).
    pub rx_total: u64,
    pub tx_total: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_drops: u64,
    pub tx_drops: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetReading {
    pub interfaces: Vec<NetInterface>,
    /// The interface the default route leaves by, when the host has one.
    pub primary: Option<String>,
    /// How long the two reads were apart, in seconds.
    pub window_secs: f64,
}

#[derive(Clone, Copy, Default)]
struct Counters {
    rx_bytes: u64,
    rx_packets: u64,
    rx_errors: u64,
    rx_drops: u64,
    tx_bytes: u64,
    tx_packets: u64,
    tx_errors: u64,
    tx_drops: u64,
}

fn counters(line: &str) -> Option<(String, Counters)> {
    let mut f = line.split_whitespace();
    let name = f.next()?.to_string();
    let n: Vec<u64> = f.map(|v| v.parse().ok()).collect::<Option<Vec<_>>>()?;
    if n.len() < 8 {
        return None;
    }
    Some((name, Counters { rx_bytes: n[0], rx_packets: n[1], rx_errors: n[2], rx_drops: n[3], tx_bytes: n[4], tx_packets: n[5], tx_errors: n[6], tx_drops: n[7] }))
}

/// An interface name is plain text from the host, shown in the window and used as a key: only the usual characters
/// are accepted.
fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':'))
}

struct Section<'a> {
    uptime: Option<f64>,
    lines: Vec<&'a str>,
}

fn sections(out: &str) -> std::collections::HashMap<&str, Section<'_>> {
    let mut map = std::collections::HashMap::new();
    let mut cur: Option<&str> = None;
    for line in out.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("@@") {
            let mut it = rest.split_whitespace();
            let key = it.next().unwrap_or("");
            let uptime = it.next().and_then(|v| v.parse::<f64>().ok());
            map.insert(key, Section { uptime, lines: Vec::new() });
            cur = Some(key);
        } else if let Some(k) = cur {
            if let Some(s) = map.get_mut(k) {
                s.lines.push(line);
            }
        }
    }
    map
}

/// Read what [`SCRIPT`] (or the metrics script) printed. `None` when the host had no counters to read.
pub fn parse(out: &str) -> Option<NetReading> {
    let s = sections(out);
    let (a, b) = (s.get("NETA")?, s.get("NETB")?);
    let window = match (a.uptime, b.uptime) {
        (Some(x), Some(y)) if y - x >= 0.2 => y - x,
        _ => return None,
    };
    let first: std::collections::HashMap<String, Counters> = a.lines.iter().filter_map(|l| counters(l)).collect();
    let meta: std::collections::HashMap<&str, (&str, bool, Option<u32>)> = s
        .get("NETIF")
        .map(|m| {
            m.lines
                .iter()
                .filter_map(|l| {
                    let mut f = l.split_whitespace();
                    let name = f.next()?;
                    let state = f.next()?;
                    let physical = f.next()? == "1";
                    let mbps = f.next().and_then(|v| v.parse::<i64>().ok()).filter(|v| *v > 0).map(|v| v as u32);
                    Some((name, (state, physical, mbps)))
                })
                .collect()
        })
        .unwrap_or_default();
    let rate = |before: u64, after: u64| -> Option<f64> { (after >= before).then(|| (after - before) as f64 / window) };
    let mut interfaces = Vec::new();
    for line in &b.lines {
        let Some((name, now)) = counters(line) else { continue };
        if name == "lo" || !name_ok(&name) {
            continue;
        }
        let before = first.get(&name);
        let m = meta.get(name.as_str());
        interfaces.push(NetInterface {
            state: m.map_or("unknown", |m| m.0).to_string(),
            physical: m.is_some_and(|m| m.1),
            link_mbps: m.and_then(|m| m.2),
            rx_bps: before.and_then(|p| rate(p.rx_bytes, now.rx_bytes)),
            tx_bps: before.and_then(|p| rate(p.tx_bytes, now.tx_bytes)),
            rx_pps: before.and_then(|p| rate(p.rx_packets, now.rx_packets)),
            tx_pps: before.and_then(|p| rate(p.tx_packets, now.tx_packets)),
            rx_total: now.rx_bytes,
            tx_total: now.tx_bytes,
            rx_errors: now.rx_errors,
            tx_errors: now.tx_errors,
            rx_drops: now.rx_drops,
            tx_drops: now.tx_drops,
            name,
        });
    }
    if interfaces.is_empty() {
        return None;
    }
    let primary = s.get("PRIMARY").and_then(|p| p.lines.iter().map(|l| l.trim()).find(|l| name_ok(l))).map(str::to_string).filter(|p| interfaces.iter().any(|i| &i.name == p));
    Some(NetReading { interfaces, primary, window_secs: window })
}

/// The interface the headline speed is read from: the one the default route leaves by, else the busiest real device.
/// (Adding every interface together would count bridged traffic twice, once on the bridge and once on the device.)
pub fn headline(r: &NetReading) -> Option<&NetInterface> {
    if let Some(p) = &r.primary {
        if let Some(i) = r.interfaces.iter().find(|i| &i.name == p) {
            return Some(i);
        }
    }
    r.interfaces
        .iter()
        .filter(|i| i.physical)
        .max_by(|a, b| (a.rx_bps.unwrap_or(0.0) + a.tx_bps.unwrap_or(0.0)).total_cmp(&(b.rx_bps.unwrap_or(0.0) + b.tx_bps.unwrap_or(0.0))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines as the script's awk prints them: name, rx bytes, packets, errs, drop, tx bytes, packets, errs, drop.
    const SCRIPT_OUT: &str = "@@NETA 1000.00
eth0 5000000 4000 1 2 1000000 3000 0 0
@@NETB 1002.00
eth0 9000000 6000 1 2 1600000 4000 0 0
@@NETIF
eth0 up 1 -1
@@PRIMARY
eth0
";

    #[test]
    fn reads_speed_from_the_change_in_the_counters_over_the_hosts_own_clock() {
        let r = parse(SCRIPT_OUT).unwrap();
        assert_eq!(r.window_secs, 2.0);
        let i = &r.interfaces[0];
        assert_eq!((i.rx_bps, i.tx_bps), (Some(2_000_000.0), Some(300_000.0)));
        assert_eq!((i.rx_pps, i.tx_pps), (Some(1000.0), Some(500.0)));
        assert_eq!((i.rx_total, i.tx_total), (9_000_000, 1_600_000));
        assert_eq!(i.link_mbps, None, "-1 means the driver doesn't say");
        assert!(i.physical && i.state == "up");
    }

    #[test]
    fn drops_the_loopback_and_picks_the_default_route_as_the_headline() {
        let with_lo = "@@NETA 1.0\nlo 1 1 0 0 1 1 0 0\neth0 1 1 0 0 1 1 0 0\n@@NETB 2.0\nlo 9 9 0 0 9 9 0 0\neth0 9 9 0 0 9 9 0 0\n@@NETIF\n@@PRIMARY\neth0\n";
        assert_eq!(parse(with_lo).unwrap().interfaces.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["eth0"]);
        let r = parse(SCRIPT_OUT).unwrap();
        assert_eq!(headline(&r).unwrap().name, "eth0");
        assert_eq!(r.primary.as_deref(), Some("eth0"));
    }

    #[test]
    fn a_counter_that_went_backwards_has_no_speed_and_a_new_interface_has_none_yet() {
        let out = "@@NETA 10.0\neth0 500 5 0 0 500 5 0 0\n@@NETB 11.0\neth0 100 6 0 0 900 6 0 0\nwg0 70 1 0 0 70 1 0 0\n@@NETIF\n@@PRIMARY\n";
        let r = parse(out).unwrap();
        let eth = r.interfaces.iter().find(|i| i.name == "eth0").unwrap();
        assert_eq!((eth.rx_bps, eth.tx_bps), (None, Some(400.0)), "a reset isn't a speed");
        let wg = r.interfaces.iter().find(|i| i.name == "wg0").unwrap();
        assert_eq!((wg.rx_bps, wg.tx_bps), (None, None));
        assert!(headline(&r).is_none(), "nothing says which is the real device, so no headline is guessed");
    }

    #[test]
    fn says_nothing_when_there_is_nothing_to_say() {
        assert!(parse("").is_none());
        assert!(parse("@@NETA 5.0\n@@NETB 5.0\neth0 1 1 0 0 1 1 0 0\n").is_none(), "no time passed");
        assert!(parse("@@NETA x\n@@NETB y\n").is_none());
        // Junk and odd names are not taken for an interface.
        let r = parse("@@NETA 1.0\n@@NETB 2.0\nwe!rd 1 1 0 0 1 1 0 0\nbad line\neth1 10 1 0 0 20 1 0 0\n").unwrap();
        assert_eq!(r.interfaces.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["eth1"]);
    }

    #[test]
    fn without_a_default_route_the_busiest_real_device_is_the_headline() {
        let out = "@@NETA 1.0\na 0 0 0 0 0 0 0 0\nb 0 0 0 0 0 0 0 0\nc 0 0 0 0 0 0 0 0\n@@NETB 2.0\na 100 1 0 0 100 1 0 0\nb 9000 1 0 0 9000 1 0 0\nc 99999 1 0 0 99999 1 0 0\n@@NETIF\na up 1 -1\nb up 1 -1\nc up 0 -1\n@@PRIMARY\n";
        assert_eq!(headline(&parse(out).unwrap()).unwrap().name, "b", "a virtual device doesn't count");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_script_runs_here_and_reports_this_machines_interfaces() {
        let out = std::process::Command::new("sh").arg("-c").arg(SCRIPT).output().expect("sh");
        let r = parse(&String::from_utf8_lossy(&out.stdout)).expect("this machine has network counters");
        assert!(r.window_secs >= 0.9 && r.window_secs < 5.0, "{}", r.window_secs);
        assert!(r.interfaces.iter().all(|i| i.name != "lo"));
        assert!(r.interfaces.iter().any(|i| i.rx_bps.is_some()));
    }
}
