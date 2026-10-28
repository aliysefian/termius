//! CPU, memory and disk use read over SSH.
//!
//! The script needs only a POSIX shell, `awk` and `df`. CPU and memory come from `/proc`, so they are filled in on
//! Linux; elsewhere they stay empty and a rule on them simply has nothing to judge (the state shows as unknown, never
//! as 0). The disk figure is for `/`.

/// Takes about a second: CPU use is the change in `/proc/stat` over that time.
pub const SCRIPT: &str = r#"if [ -r /proc/stat ]; then
  read -r _ u1 n1 s1 i1 w1 _ < /proc/stat
  sleep 1
  read -r _ u2 n2 s2 i2 w2 _ < /proc/stat
  busy=$((u2+n2+s2-u1-n1-s1))
  total=$((u2+n2+s2+i2+w2-u1-n1-s1-i1-w1))
  if [ "$total" -gt 0 ]; then cpu=$((100*busy/total)); else cpu=0; fi
  mt=$(awk '/MemTotal/{print $2}' /proc/meminfo)
  ma=$(awk '/MemAvailable/{print $2}' /proc/meminfo)
  if [ -n "$mt" ] && [ -n "$ma" ] && [ "$mt" -gt 0 ]; then mem=$(( (mt-ma)*100/mt )); fi
fi
disk=$(df -P / 2>/dev/null | awk 'NR==2{gsub("%","",$5); print $5}')
echo "CPU=$cpu"
echo "MEM=$mem"
echo "DISK=$disk"
"#;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Metrics {
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    pub disk: Option<f64>,
}

/// Read the script's `KEY=value` lines. A value that is empty or not a percentage is `None`.
pub fn parse(out: &str) -> Metrics {
    let mut m = Metrics::default();
    for line in out.lines() {
        let Some((key, value)) = line.trim().split_once('=') else { continue };
        let v = value.trim().parse::<f64>().ok().filter(|v| (0.0..=100.0).contains(v));
        match key.trim() {
            "CPU" => m.cpu = v,
            "MEM" => m.mem = v,
            "DISK" => m.disk = v,
            _ => {}
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_lines_and_leaves_gaps_empty() {
        assert_eq!(parse("CPU=12\nMEM=47\nDISK=81\n"), Metrics { cpu: Some(12.0), mem: Some(47.0), disk: Some(81.0) });
        assert_eq!(parse("CPU=\nMEM=\nDISK=55\n"), Metrics { cpu: None, mem: None, disk: Some(55.0) });
        // Noise from a login banner, out-of-range numbers and junk are not readings.
        assert_eq!(parse("Welcome!\nCPU=250\nMEM=abc\nDISK=9\r\n"), Metrics { cpu: None, mem: None, disk: Some(9.0) });
        assert_eq!(parse(""), Metrics::default());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_script_runs_and_reports_this_machine() {
        let out = std::process::Command::new("sh").arg("-c").arg(SCRIPT).output().expect("sh");
        let m = parse(&String::from_utf8_lossy(&out.stdout));
        assert!(m.cpu.is_some() && m.mem.is_some() && m.disk.is_some(), "{m:?}");
    }
}
