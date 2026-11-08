//! Temperatures, GPUs, processes, listening ports, logins and signals.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use super::derive::{derive, derive_with};
use super::logins;
use super::manager::{Config, HostMetricsManager, MetricsError};
use super::model::*;
use super::parse::{parse, temp_kind};
use super::ports;
use super::procs::{self, SignalError, SignalOutcome};
#[cfg(target_os = "linux")]
use super::script::{self, Options};
use super::tests::{host_id, mgr, Fake, Fx, SEC};

/// A pretend host's reading with extra sections added before its end line.
fn with(extra: &str) -> String {
    Fx::host().out().replace("@@END\n", &format!("{extra}@@END\n"))
}

fn sample_of(extra: &str) -> Sample {
    derive(host_id(), 0, None, &parse(&with(extra)).unwrap(), &StaticInfo::default()).0
}

// -- temperatures ----------------------------------------------------------------------------------------------------

#[test]
fn a_sensor_is_a_cpus_only_by_the_driver_that_owns_it() {
    assert_eq!(temp_kind("coretemp", "Package id 0"), TempKind::CpuPackage);
    assert_eq!(temp_kind("coretemp", "Core 3"), TempKind::CpuCore);
    assert_eq!(temp_kind("k10temp", "Tctl"), TempKind::CpuPackage);
    assert_eq!(temp_kind("k10temp", "Tccd1"), TempKind::CpuCore);
    assert_eq!(temp_kind("cpu_thermal", ""), TempKind::CpuPackage);
    // Not the processor's, however they are labelled.
    assert_eq!(temp_kind("acpitz", "CPU"), TempKind::System);
    assert_eq!(temp_kind("nvme", "Composite"), TempKind::Storage);
    assert_eq!(temp_kind("drivetemp", ""), TempKind::Storage);
    assert_eq!(temp_kind("amdgpu", "edge"), TempKind::Gpu);
    assert_eq!(temp_kind("pch_cannonlake", ""), TempKind::System);
    assert_eq!(temp_kind("iwlwifi_1", ""), TempKind::Network);
    assert_eq!(temp_kind("mystery", "CPU Temp"), TempKind::Other);
}

#[test]
fn temperatures_are_read_with_their_limits_and_bad_values_are_unavailable() {
    let s = sample_of(
        "@@TEMPS\nH|coretemp|temp1|Package id 0|48000|80000|100000\nH|coretemp|temp2|Core 0|45000||\nH|coretemp|temp3|Core 1|0||\nH|nvme|temp1|Composite|38850|83800|87800\nH|acpitz|temp1||-274000||\nH|weird|temp1||abc||\n",
    );
    assert_eq!(s.temperatures.len(), 6);
    let pkg = &s.temperatures[0];
    assert_eq!((pkg.chip.as_str(), pkg.label.as_str(), pkg.kind, pkg.celsius, pkg.high, pkg.critical), ("coretemp", "Package id 0", TempKind::CpuPackage, Some(48.0), Some(80.0), Some(100.0)));
    assert_eq!(s.temperatures[2].celsius, None, "0 is what an unconnected sensor reports");
    assert_eq!(s.temperatures[3].celsius, Some(38.85));
    assert_eq!(s.temperatures[4].celsius, None, "an impossible value is no value");
    assert_eq!(s.temperatures[4].label, "temp1", "a sensor without a label is named by its file");
    assert_eq!(s.temperatures[5].celsius, None);
    assert!(s.capabilities.sensors);
    assert_eq!(s.cpu_temperature(), Some(48.0));
}

#[test]
fn the_cpu_temperature_is_only_ever_from_a_cpu_sensor() {
    // Only a motherboard zone and a disk: no CPU figure, and no guess.
    let s = sample_of("@@TEMPS\nH|acpitz|temp1||61000||\nH|nvme|temp1|Composite|41000||\n");
    assert_eq!(s.cpu_temperature(), None);
    assert_eq!(HistoryPoint::of(&s).temp_cpu, None);
    // Cores but no package: the hottest core.
    let s = sample_of("@@TEMPS\nH|coretemp|temp2|Core 0|45000||\nH|coretemp|temp3|Core 1|52000||\n");
    assert_eq!(s.cpu_temperature(), Some(52.0));
    assert_eq!(HistoryPoint::of(&s).temp_cpu, Some(52.0));
    // None at all (a virtual machine): empty, flagged as no sensors, not zero.
    let s = sample_of("");
    assert!(s.temperatures.is_empty() && !s.capabilities.sensors);
    assert_eq!(s.cpu_temperature(), None);
}

#[test]
fn a_thermal_zone_that_repeats_a_hwmon_chip_is_not_listed_twice() {
    let s = sample_of("@@TEMPS\nH|coretemp|temp1|Package id 0|48000||\nH|acpitz|temp1||30000||\nZ|x86_pkg_temp|47000\nZ|acpitz|30000\nZ|INT3400 Thermal|20000\nZ|cpu-thermal|55000\n");
    let chips: Vec<_> = s.temperatures.iter().map(|t| (t.source.as_str(), t.chip.as_str())).collect();
    assert_eq!(chips, [("hwmon", "coretemp"), ("hwmon", "acpitz"), ("thermal", "INT3400 Thermal")]);
    // A Raspberry Pi has only the zone, and its CPU zone is the CPU's.
    let pi = sample_of("@@TEMPS\nZ|cpu-thermal|55000\n");
    assert_eq!(pi.cpu_temperature(), Some(55.0));
}

// -- GPUs ------------------------------------------------------------------------------------------------------------

#[test]
fn an_amd_gpu_is_read_from_the_drivers_own_files() {
    let s = sample_of("@@AMDGPU\nA|card0|AMD Radeon RX 6800|1002:73BF|amdgpu|37|2147483648|17163091968|61000|85000000|255000000|128|255\nA|card1||1002:1638|amdgpu||||||||\n");
    assert_eq!(s.gpus.len(), 2);
    let g = &s.gpus[0];
    assert_eq!((g.id.as_str(), g.vendor, g.name.as_str(), g.driver.as_deref()), ("amd0", GpuVendor::Amd, "AMD Radeon RX 6800", Some("amdgpu")));
    assert_eq!((g.util_pct, g.vram_used, g.vram_total, g.celsius), (Some(37.0), Some(2 << 30), Some(17_163_091_968), Some(61.0)));
    assert_eq!((g.power_watts, g.power_limit_watts), (Some(85.0), Some(255.0)));
    assert!((g.fan_pct.unwrap() - 50.2).abs() < 0.1, "pwm 128 of 255");
    assert!((g.vram_pct().unwrap() - 12.5).abs() < 0.1);
    // A card that says nothing has nothing, and is named by its PCI id rather than made up.
    let h = &s.gpus[1];
    assert_eq!(h.name, "AMD GPU 1002:1638");
    assert_eq!((h.util_pct, h.vram_used, h.vram_total, h.celsius, h.power_watts, h.fan_pct), (None, None, None, None, None, None));
    assert!(s.capabilities.amd_gpu);
}

#[test]
fn nvidia_gpus_come_from_nvidia_smi_with_not_available_as_unavailable() {
    let smi = "@@NVIDIA\n0, NVIDIA GeForce RTX 3060, 18, 7, 2150, 12288, 56, 62.40, 170.00, 35, 535.104.05, GPU-aaa\n1, Tesla T4, [N/A], [N/A], 0, 15360, 41, [Not Supported], 70.00, [N/A], 535.104.05, GPU-bbb\nrc=0\n@@NVAPPS\n4242, /usr/bin/python3, 1800, GPU-aaa\n77, ollama, 350, GPU-aaa\n9, ghost, 1, GPU-zzz\n";
    let s = sample_of(smi);
    assert_eq!(s.gpus.len(), 2);
    let a = &s.gpus[0];
    assert_eq!((a.id.as_str(), a.vendor, a.name.as_str()), ("nvidia0", GpuVendor::Nvidia, "NVIDIA GeForce RTX 3060"));
    assert_eq!((a.util_pct, a.mem_util_pct, a.celsius, a.power_watts, a.power_limit_watts, a.fan_pct), (Some(18.0), Some(7.0), Some(56.0), Some(62.4), Some(170.0), Some(35.0)));
    assert_eq!((a.vram_used, a.vram_total), (Some(2150 << 20), Some(12288 << 20)));
    assert_eq!(a.processes, vec![GpuProcess { pid: 4242, name: "python3".into(), vram: Some(1800 << 20) }, GpuProcess { pid: 77, name: "ollama".into(), vram: Some(350 << 20) }]);
    let b = &s.gpus[1];
    assert_eq!((b.util_pct, b.power_watts, b.fan_pct), (None, None, None), "[N/A] is unavailable, not 0");
    assert!(b.processes.is_empty(), "a process of an unknown card is not attached to another");
}

#[test]
fn a_failed_or_stuck_nvidia_smi_is_a_note_and_not_a_gpu() {
    let s = sample_of("@@NVIDIA\nrc=124\n@@NVAPPS\n");
    assert!(s.gpus.is_empty());
    assert_eq!(s.notes, vec!["nvidia-smi did not answer within 5 seconds".to_string()]);
    let s = sample_of("@@NVIDIA\nrc=9\n@@NVAPPS\n");
    assert!(s.notes[0].contains("failed (status 9)"));
    let s = sample_of("@@NVSKIP\nno timeout command\n");
    assert!(s.notes[0].contains("no `timeout` command"));
}

#[test]
fn nvidia_gpus_are_carried_between_asks_but_not_past_a_fresh_empty_answer() {
    let asked = parse(&with("@@NVIDIA\n0, A100, 90, 50, 1000, 40000, 60, 200.0, 400.0, [N/A], 535, GPU-a\nrc=0\n@@NVAPPS\n")).unwrap();
    let first = derive_with(host_id(), 0, None, &asked, &StaticInfo::default(), &[]).0;
    assert_eq!(first.gpus.len(), 1);
    // A reading that did not ask: the earlier answer stands in.
    let quiet = parse(&Fx::host().at(1002.0).out()).unwrap();
    let carried = derive_with(host_id(), 0, Some(&asked), &quiet, &StaticInfo::default(), &first.gpus).0;
    assert_eq!(carried.gpus, first.gpus);
    // A reading that asked and found none: none.
    let none = parse(&Fx::host().at(1004.0).out().replace("@@END\n", "@@NVIDIA\nrc=0\n@@NVAPPS\n@@END\n")).unwrap();
    assert!(derive_with(host_id(), 0, Some(&quiet), &none, &StaticInfo::default(), &first.gpus).0.gpus.is_empty());
}

#[test]
fn history_keeps_the_busiest_gpu_and_the_cpu_temperature() {
    let s = sample_of("@@TEMPS\nH|coretemp|temp1|Package id 0|48000||\n@@AMDGPU\nA|card0|X|1002:1|amdgpu|40|0|0|||||\n@@NVIDIA\n0, N, 77, 1, 1, 2, 3, 4, 5, 6, 7, GPU-a\nrc=0\n@@NVAPPS\n");
    let p = HistoryPoint::of(&s);
    assert_eq!((p.temp_cpu, p.gpu_util), (Some(48.0), Some(77.0)));
}

/// The real script against a pretend /sys and a pretend `nvidia-smi`, so what the shell does with real files is tested too.
#[cfg(target_os = "linux")]
#[test]
fn the_real_script_reads_a_pretend_sysfs_and_a_pretend_nvidia_smi() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    if !std::path::Path::new("/proc/stat").exists() || Command::new("sh").args(["-c", "command -v timeout"]).output().map_or(true, |o| !o.status.success()) {
        return;
    }
    let root = std::env::temp_dir().join(format!("hm-sys-{}-{}", std::process::id(), Uuid::new_v4().simple()));
    let put = |rel: &str, text: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put("sys/class/hwmon/hwmon0/name", "coretemp\n");
    put("sys/class/hwmon/hwmon0/temp1_input", "51000\n");
    put("sys/class/hwmon/hwmon0/temp1_label", "Package id 0\n");
    put("sys/class/hwmon/hwmon0/temp1_max", "84000\n");
    put("sys/class/hwmon/hwmon0/temp1_crit", "100000\n");
    put("sys/class/hwmon/hwmon1/name", "nvme\n");
    put("sys/class/hwmon/hwmon1/temp1_input", "39000\n");
    put("sys/class/thermal/thermal_zone0/type", "x86_pkg_temp\n");
    put("sys/class/thermal/thermal_zone0/temp", "50000\n");
    put("sys/class/drm/card0/device/vendor", "0x1002\n");
    put("sys/class/drm/card0/device/gpu_busy_percent", "42\n");
    put("sys/class/drm/card0/device/mem_info_vram_used", "1073741824\n");
    put("sys/class/drm/card0/device/mem_info_vram_total", "8589934592\n");
    put("sys/class/drm/card0/device/uevent", "DRIVER=amdgpu\nPCI_ID=1002:73BF\n");
    put("sys/class/drm/card0/device/hwmon/hwmon3/temp1_input", "66000\n");
    put("sys/class/drm/card0/device/hwmon/hwmon3/power1_average", "90000000\n");
    put("sys/class/drm/card0/device/hwmon/hwmon3/pwm1", "255\n");
    // A connector and a non-AMD card are not GPUs here.
    put("sys/class/drm/card0-DP-1/device/vendor", "0x1002\n");
    put("sys/class/drm/card1/device/vendor", "0x8086\n");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let smi = bin.join("nvidia-smi");
    std::fs::write(&smi, "#!/bin/sh\ncase \"$*\" in\n*query-gpu*) echo '0, Fake GPU, 33, 10, 1024, 8192, 50, 80.00, 150.00, 40, 999.1, GPU-fake';;\n*query-compute-apps*) echo '321, /opt/x/trainer, 512, GPU-fake';;\nesac\n").unwrap();
    std::fs::set_permissions(&smi, std::fs::Permissions::from_mode(0o755)).unwrap();

    let run = |nvidia: bool| {
        let out = Command::new("sh")
            .arg("-c")
            .arg(script::script_with(Options { with_static: false, nvidia }))
            .env("SYS", root.join("sys"))
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
            .output()
            .expect("sh");
        assert!(out.stderr.is_empty(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        parse(&String::from_utf8_lossy(&out.stdout)).expect("parses")
    };
    let quiet = run(false);
    assert!(quiet.nvidia.is_none(), "not asked, so not read");
    let asked = run(true);
    let s = derive(host_id(), 0, None, &asked, &StaticInfo::default()).0;
    let chips: Vec<_> = s.temperatures.iter().map(|t| (t.chip.as_str(), t.kind, t.celsius)).collect();
    assert_eq!(chips, [("coretemp", TempKind::CpuPackage, Some(51.0)), ("nvme", TempKind::Storage, Some(39.0))], "the zone repeating the package is dropped");
    assert_eq!(s.temperatures[0].high, Some(84.0));
    assert_eq!(s.cpu_temperature(), Some(51.0));
    assert_eq!(s.gpus.len(), 2, "{:?}", s.gpus);
    let amd = &s.gpus[0];
    assert_eq!((amd.name.as_str(), amd.driver.as_deref(), amd.util_pct, amd.celsius, amd.power_watts, amd.fan_pct), ("AMD GPU 1002:73BF", Some("amdgpu"), Some(42.0), Some(66.0), Some(90.0), Some(100.0)));
    assert_eq!((amd.vram_used, amd.vram_total), (Some(1 << 30), Some(8 << 30)));
    let nv = &s.gpus[1];
    assert_eq!((nv.name.as_str(), nv.util_pct, nv.vram_total, nv.fan_pct), ("Fake GPU", Some(33.0), Some(8192 << 20), Some(40.0)));
    assert_eq!(nv.processes, vec![GpuProcess { pid: 321, name: "trainer".into(), vram: Some(512 << 20) }]);
    assert!(s.capabilities.sensors && s.capabilities.amd_gpu && s.capabilities.nvidia_smi);
    std::fs::remove_dir_all(&root).ok();
}

// -- processes -------------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn stat(pid: u32, name: &str, state: &str, ppid: u32, ticks: u64, threads: u32, start: u64, rss: u64) -> String {
    format!("{pid} ({name}) {state} {ppid} 0 0 0 -1 0 0 0 0 0 {ticks} 0 0 0 20 0 {threads} 0 {start} 0 {rss} 0")
}

fn proc_out(a: Option<(f64, &[String])>, b: (f64, &[String]), extra: &str) -> String {
    let mut s = String::from("@@OS\nLinux\n@@CLK\n100\n@@PAGE\n4096\n");
    if let Some((up, lines)) = a {
        s += &format!("@@UPA\n{up:.2} 1.0\n@@STATA\n{}\n", lines.join("\n"));
        s += &format!("@@UPB\n{:.2} 1.0\n@@STATB\n{}\n", b.0, b.1.join("\n"));
    } else {
        s += &format!("@@UPA\n{:.2} 1.0\n@@STATA\n{}\n", b.0, b.1.join("\n"));
    }
    s + "@@MEMTOTAL\nMemTotal:       16000000 kB\n@@PASSWD\nroot:x:0:0:root:/root:/bin/bash\nwww-data:x:33:33::/var/www:/usr/sbin/nologin\n@@UIDS\n1 0\n812 0\n900 33\n901 4242\n@@ARGS\n  812 /usr/sbin/sshd -D\n  900 nginx: worker process\n" + extra + "@@END\n"
}

#[test]
fn a_stat_line_is_read_even_when_the_name_has_spaces_and_parentheses() {
    let raw = procs::parse(&proc_out(None, (500.0, &[stat(7, "Web Content (x)", "S", 1, 50, 4, 1000, 100)]), "")).unwrap();
    let p = &raw.now.procs[&7];
    assert_eq!((p.name.as_str(), p.state.as_str(), p.ppid, p.ticks, p.threads, p.start, p.rss_pages), ("Web Content (x)", "S", 1, 50, 4, 1000, 100));
}

#[test]
fn cpu_use_is_ticks_over_the_hosts_clock_and_a_reused_pid_has_none() {
    let a = vec![stat(812, "sshd", "S", 1, 1000, 1, 100, 500), stat(900, "nginx", "S", 1, 200, 2, 200, 3000), stat(5, "old", "S", 1, 10, 1, 50, 10)];
    // 2 s later at 100 ticks/s: sshd +50 ticks = 25%; nginx +400 = 200% (two cores); pid 5 is a different program (new start).
    let b = vec![stat(812, "sshd", "S", 1, 1050, 1, 100, 500), stat(900, "nginx", "R", 1, 600, 2, 200, 3000), stat(5, "other", "S", 1, 9999, 1, 1999, 10), stat(950, "fresh", "S", 1, 30, 1, 1990, 20)];
    let raw = procs::parse(&proc_out(Some((1000.0, &a)), (1002.0, &b), "")).unwrap();
    let list = procs::derive(host_id(), 0, None, &raw);
    assert_eq!(list.baseline, Baseline::Measured { elapsed_secs: 2.0 });
    let get = |pid: u32| list.items.iter().find(|p| p.pid == pid).unwrap();
    assert_eq!(get(812).cpu_pct, Some(25.0));
    assert_eq!(get(900).cpu_pct, Some(200.0));
    assert_eq!((get(5).cpu_pct, get(950).cpu_pct), (None, None), "a reused or new pid has no history, so no figure");
    // Busiest first, those with no figure after.
    assert_eq!(list.items.iter().map(|p| p.pid).collect::<Vec<_>>(), [900, 812, 950, 5]);
    assert_eq!(get(900).state, "R");
}

#[test]
fn the_first_list_without_two_readings_has_no_cpu_figures_and_says_why() {
    let b = vec![stat(812, "sshd", "S", 1, 1000, 1, 100, 500)];
    let list = procs::derive(host_id(), 0, None, &procs::parse(&proc_out(None, (1000.0, &b), "")).unwrap());
    assert_eq!(list.baseline, Baseline::First);
    assert_eq!(list.items[0].cpu_pct, None);
    // A later list is compared with the stored snapshot.
    let raw1 = procs::parse(&proc_out(None, (1000.0, &b), "")).unwrap();
    let c = vec![stat(812, "sshd", "S", 1, 1200, 1, 100, 500)];
    let raw2 = procs::parse(&proc_out(None, (1004.0, &c), "")).unwrap();
    let list = procs::derive(host_id(), 0, Some(&raw1.now), &raw2);
    assert_eq!(list.items[0].cpu_pct, Some(50.0));
    // And a host that restarted between them is not compared at all.
    let list = procs::derive(host_id(), 0, Some(&raw2.now), &raw1);
    assert_eq!((list.baseline, list.items[0].cpu_pct), (Baseline::Rebooted, None));
}

#[test]
fn users_commands_memory_and_age_are_filled_in_from_what_the_host_said() {
    let b = vec![stat(812, "sshd", "S", 1, 1000, 1, 100, 1000), stat(900, "nginx", "S", 1, 0, 2, 200, 0), stat(901, "mystery", "S", 1, 0, 1, 300, 0)];
    let list = procs::derive(host_id(), 0, None, &procs::parse(&proc_out(None, (1000.0, &b), "")).unwrap());
    let get = |pid: u32| list.items.iter().find(|p| p.pid == pid).unwrap();
    assert_eq!((get(812).user.as_str(), get(900).user.as_str(), get(901).user.as_str()), ("root", "www-data", "4242"));
    assert_eq!(get(812).command, "/usr/sbin/sshd -D");
    assert_eq!(get(901).command, "", "no command line for a process ps did not list");
    assert!(list.commands);
    assert_eq!(get(812).rss, 1000 * 4096);
    assert_eq!(get(812).mem_pct, Some(0.0), "4 MB of 16 GB rounds to 0.0 at one decimal");
    assert_eq!(get(812).age_secs, Some(1000.0 - 1.0));
    assert_eq!(get(812).start_ticks, 100);
}

#[test]
fn a_big_list_is_cut_to_the_busiest_and_says_so() {
    let a: Vec<String> = (2..=700u32).map(|p| stat(p, "p", "S", 1, 0, 1, 10, 1)).collect();
    // Every process gains ticks equal to its pid, so the highest pids are the busiest.
    let b: Vec<String> = (2..=700u32).map(|p| stat(p, "p", "S", 1, u64::from(p), 1, 10, 1)).collect();
    let raw = procs::parse(&proc_out(Some((1000.0, &a)), (1001.0, &b), "")).unwrap();
    let list = procs::derive(host_id(), 0, None, &raw);
    assert_eq!((list.total, list.truncated, list.items.len()), (699, true, procs::MAX_ITEMS));
    assert_eq!(list.items[0].pid, 700);
    assert_eq!(list.items.last().unwrap().pid, 201, "the 500 busiest, down to the 500th");
}

#[test]
fn a_process_listing_that_was_cut_short_is_refused() {
    assert!(procs::parse("@@OS\nLinux\n@@UPA\n1 1\n").is_err());
    assert!(procs::parse("").is_err());
}

#[test]
fn the_signal_command_only_ever_holds_checked_things() {
    let s = procs::signal_script(812, 100, "sshd", "TERM").unwrap();
    assert!(s.contains("pid=812") && s.contains("kill -TERM \"$pid\"") && s.contains("'sshd'") && s.contains("\"$start\" != 100"));
    assert_eq!(procs::signal_script(1, 1, "init", "TERM").unwrap_err(), SignalError::BadPid);
    assert_eq!(procs::signal_script(0, 1, "x", "TERM").unwrap_err(), SignalError::BadPid);
    assert_eq!(procs::signal_script(5, 1, "x", "9").unwrap_err(), SignalError::BadSignal);
    assert_eq!(procs::signal_script(5, 1, "x", "TERM; rm -rf /").unwrap_err(), SignalError::BadSignal);
    for bad in ["a'b", "$(id)", "a;b", "a`b", "", "a\nb", &"x".repeat(41)] {
        assert_eq!(procs::signal_script(5, 1, bad, "KILL").unwrap_err(), SignalError::BadName, "{bad:?}");
    }
    for sig in procs::SIGNALS {
        assert!(procs::signal_script(5, 1, "x", sig).is_ok());
    }
}

#[test]
fn the_signal_answers_are_read() {
    assert_eq!(procs::parse_signal("RESULT=ok\n@@END\n"), SignalOutcome::Sent);
    assert_eq!(procs::parse_signal("RESULT=gone\n"), SignalOutcome::Gone);
    assert_eq!(procs::parse_signal("RESULT=changed\n"), SignalOutcome::Changed);
    assert_eq!(procs::parse_signal("RESULT=refused:sh: 1: kill: Operation not permitted\n"), SignalOutcome::Refused { reason: "sh: 1: kill: Operation not permitted".into() });
    assert!(matches!(procs::parse_signal("garbage"), SignalOutcome::Refused { .. }));
}

/// Real processes on this machine: a signal reaches the process it was meant for, and only that one.
#[cfg(target_os = "linux")]
#[test]
fn a_signal_checks_the_process_identity_on_a_real_host() {
    use std::process::{Command, Stdio};
    if !std::path::Path::new("/proc/self/stat").exists() {
        return;
    }
    let mut child = Command::new("sleep").arg("60").stdout(Stdio::null()).spawn().expect("sleep");
    let pid = child.id();
    // `spawn` can return before the new process has become `sleep` (until then it carries its parent's name), and a
    // process's identity is only worth checking once it has settled.
    let mut line = String::new();
    for _ in 0..200 {
        line = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        if line.contains("(sleep)") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(line.contains("(sleep)"), "{line}");
    let (_, st) = {
        let raw = procs::parse(&proc_out(None, (1.0, &[line.trim().to_string()]), "")).unwrap();
        (pid, raw.now.procs[&pid].clone())
    };
    let run = |pid: u32, start: u64, name: &str, sig: &str| {
        let script = procs::signal_script(pid, start, name, sig).unwrap();
        let out = Command::new("sh").arg("-c").arg(script).output().unwrap();
        procs::parse_signal(&String::from_utf8_lossy(&out.stdout))
    };
    // The wrong start time (a pid that was reused) or the wrong name: nothing is sent.
    assert_eq!(run(pid, st.start + 1, &st.name, "TERM"), SignalOutcome::Changed);
    assert_eq!(run(pid, st.start, "something-else", "TERM"), SignalOutcome::Changed);
    assert!(child.try_wait().unwrap().is_none(), "the process is still there after the refused attempts");
    // A pid that does not exist.
    assert_eq!(run(4_000_000, 1, "x", "TERM"), SignalOutcome::Gone);
    // The right identity: delivered, and the process ends.
    assert_eq!(run(pid, st.start, &st.name, "TERM"), SignalOutcome::Sent);
    let status = child.wait().unwrap();
    assert!(!status.success(), "it was terminated: {status:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn the_real_process_script_lists_this_machines_processes_with_cpu_figures() {
    use std::process::Command;
    if !std::path::Path::new("/proc/stat").exists() {
        return;
    }
    let out = Command::new("sh").arg("-c").arg(procs::script(true)).output().expect("sh");
    let raw = procs::parse(&String::from_utf8_lossy(&out.stdout)).expect("parses");
    let list = procs::derive(host_id(), 0, None, &raw);
    assert!(matches!(list.baseline, Baseline::Measured { elapsed_secs } if (0.9..5.0).contains(&elapsed_secs)), "{:?}", list.baseline);
    assert!(list.total > 3 && !list.items.is_empty());
    let me = list.items.iter().find(|p| p.pid == std::process::id()).or_else(|| list.items.iter().find(|p| p.pid > 1));
    assert!(me.is_some());
    assert!(list.items.iter().any(|p| p.cpu_pct.is_some()));
    assert!(list.items.iter().all(|p| p.rss < (1 << 50)));
    assert!(list.items.iter().any(|p| !p.user.is_empty()), "users are named");
    // A second list, one reading, compared with the first (a moment later: two readings in the same instant measure nothing).
    std::thread::sleep(Duration::from_millis(500));
    let out = Command::new("sh").arg("-c").arg(procs::script(false)).output().expect("sh");
    let raw2 = procs::parse(&String::from_utf8_lossy(&out.stdout)).expect("parses");
    let list2 = procs::derive(host_id(), 0, Some(&raw.now), &raw2);
    assert!(matches!(list2.baseline, Baseline::Measured { .. }), "{:?}", list2.baseline);
    if Command::new("sh").args(["-c", "command -v ps"]).output().is_ok_and(|o| o.status.success()) {
        assert!(list.commands && list.items.iter().any(|p| p.command.contains('/') || !p.command.is_empty()));
    }
}

// -- listening ports ---------------------------------------------------------------------------------------------------

fn port_out(tool: &str, body: &str) -> String {
    format!("@@OS\nLinux\n@@PORTS\ntool {tool}\n{body}@@PASSWD\nroot:x:0:0::/root:/bin/sh\npostgres:x:114:120::/var/lib/postgresql:/bin/bash\n@@END\n")
}

#[test]
fn ss_output_becomes_listeners_with_their_owners_and_scope() {
    let ss = "tcp   LISTEN 0 128  0.0.0.0:22     0.0.0.0:*   users:((\"sshd\",pid=812,fd=3)) ino:1 sk:1\n\
tcp   LISTEN 0 128     [::]:22        [::]:*   users:((\"sshd\",pid=812,fd=4)) ino:1 sk:1\n\
tcp   LISTEN 0 244  127.0.0.1:5432  0.0.0.0:*  users:((\"postgres\",pid=27630,fd=5)) uid:114 ino:2 sk:2\n\
tcp   LISTEN 0 4096 127.0.0.53%lo:53 0.0.0.0:*  uid:101 ino:3 sk:3\n\
udp   UNCONN 0 0       *:51820        *:*      users:((\"wg\",pid=1867,fd=3)) uid:0 ino:4 sk:4\n\
tcp   LISTEN 0 511  0.0.0.0:80     0.0.0.0:*   users:((\"nginx\",pid=100,fd=6),(\"nginx\",pid=101,fd=6))\n";
    let l = ports::parse(host_id(), 0, &port_out("ss", ss)).unwrap().unwrap();
    assert_eq!(l.source, "ss");
    let key = |p: &ListeningPort| (p.proto, p.port, p.address.clone());
    let by = |port: u16, proto: Proto| l.items.iter().find(|p| p.port == port && p.proto == proto).unwrap();
    assert_eq!(l.items.len(), 6);
    let ssh4 = l.items.iter().find(|p| p.port == 22 && !p.ipv6).unwrap();
    assert_eq!((ssh4.address.as_str(), ssh4.pid, ssh4.process.as_deref(), ssh4.local_only, ssh4.state.as_str()), ("0.0.0.0", Some(812), Some("sshd"), false, "LISTEN"));
    let ssh6 = l.items.iter().find(|p| p.port == 22 && p.ipv6).unwrap();
    assert_eq!(ssh6.address, "::");
    let pg = by(5432, Proto::Tcp);
    assert_eq!((pg.local_only, pg.user.as_deref(), pg.process.as_deref()), (true, Some("postgres"), Some("postgres")));
    let dns = by(53, Proto::Tcp);
    assert_eq!((dns.address.as_str(), dns.local_only, dns.process.clone(), dns.user.clone()), ("127.0.0.53", true, None, Some("101".into())));
    let wg = by(51820, Proto::Udp);
    assert_eq!((wg.address.as_str(), wg.user.as_deref(), wg.state.as_str()), ("*", Some("root"), "UNCONN"));
    assert_eq!(by(80, Proto::Tcp).pid, Some(100), "the first process named");
    assert!(l.owners_hidden, "one listener has no owner shown");
    // Sorted by port, then protocol.
    let order: Vec<u16> = l.items.iter().map(|p| p.port).collect();
    assert_eq!(order, [22, 22, 53, 80, 5432, 51820]);
    let _ = key;
}

#[test]
fn the_same_listener_seen_twice_is_one_row() {
    let ss = "tcp LISTEN 0 1 0.0.0.0:22 0.0.0.0:* users:((\"sshd\",pid=1,fd=3))\ntcp LISTEN 0 1 0.0.0.0:22 0.0.0.0:* users:((\"sshd\",pid=2,fd=3))\nNetid State Recv-Q Send-Q Local Address:Port Peer Address:Port\n";
    assert_eq!(ports::parse(host_id(), 0, &port_out("ss", ss)).unwrap().unwrap().items.len(), 1);
}

#[test]
fn netstat_output_is_read_too() {
    let ns = "Active Internet connections (only servers)\nProto Recv-Q Send-Q Local Address           Foreign Address         State       PID/Program name\ntcp        0      0 0.0.0.0:22              0.0.0.0:*               LISTEN      812/sshd\ntcp6       0      0 :::443                  :::*                    LISTEN      -\ntcp        0      0 10.0.0.5:44444          10.0.0.9:80             ESTABLISHED 1/x\nudp        0      0 0.0.0.0:68              0.0.0.0:*                           845/dhclient\n";
    let l = ports::parse(host_id(), 0, &port_out("netstat", ns)).unwrap().unwrap();
    assert_eq!(l.source, "netstat");
    assert_eq!(l.items.iter().map(|p| (p.port, p.proto)).collect::<Vec<_>>(), [(22, Proto::Tcp), (68, Proto::Udp), (443, Proto::Tcp)]);
    assert_eq!(l.items[0].process.as_deref(), Some("sshd"));
    assert!(l.items[2].ipv6 && l.items[2].process.is_none() && l.items[2].address == "::");
    assert!(l.owners_hidden);
}

#[test]
fn the_kernels_own_tables_work_without_ss_and_say_who_owns_nothing() {
    let proc = "file tcp\n  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000 100 0 0 10 0\n   1: 0100007F:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000   114        0 12346 1 0000 100 0 0 10 0\n   2: 0100007F:9999 0100007F:1538 01 00000000:00000000 00:00000000 00000000   114        0 12347 1 0000 100 0 0 10 0\nfile tcp6\n  sl  local_address                         rem_address                            st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000001000000:01BB 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22222 1 0 100 0 0 10 0\n   1: 00000000000000000000000000000000:0050 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22223 1 0 100 0 0 10 0\nfile udp\n  sl  local_address rem_address   st\n   0: 00000000:0044 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 33333 2 0 0\n";
    let l = ports::parse(host_id(), 0, &port_out("proc", proc)).unwrap().unwrap();
    assert_eq!(l.source, "proc");
    let got: Vec<_> = l.items.iter().map(|p| (p.port, p.address.as_str(), p.local_only, p.user.as_deref())).collect();
    assert_eq!(got, [(22, "0.0.0.0", false, Some("root")), (68, "0.0.0.0", false, Some("root")), (80, "::", false, Some("root")), (443, "::1", true, Some("root")), (5432, "127.0.0.1", true, Some("postgres"))]);
    assert!(l.items.iter().all(|p| p.process.is_none()));
    assert!(!l.owners_hidden, "the tables name no process, and nothing is claimed about owners");
}

#[test]
fn a_listing_for_another_system_or_cut_short_is_not_made_up() {
    assert_eq!(ports::parse(host_id(), 0, "@@OS\nDarwin\n@@END\n").unwrap(), None);
    assert!(ports::parse(host_id(), 0, "@@OS\nLinux\n").is_err());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_real_port_script_sees_a_listener_opened_by_this_test() {
    use std::process::Command;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let out = Command::new("sh").arg("-c").arg(ports::SCRIPT).output().expect("sh");
    let list = ports::parse(host_id(), 0, &String::from_utf8_lossy(&out.stdout)).expect("parses").expect("Linux");
    let mine = list.items.iter().find(|p| p.port == port && p.proto == Proto::Tcp).unwrap_or_else(|| panic!("port {port} not in {:?}", list.items.iter().map(|p| p.port).collect::<Vec<_>>()));
    assert!(mine.local_only && mine.address == "127.0.0.1" && !mine.ipv6);
    if list.source != "proc" {
        assert_eq!(mine.pid, Some(std::process::id()), "ss names this very process ({})", list.source);
    }
    drop(l);
}

// -- logins ------------------------------------------------------------------------------------------------------------

fn login_out(who: &str, last: &str, lastb: &str, auth: &str) -> String {
    format!("@@OS\nLinux\n@@NOW\n1794121000\n@@TZ\n+0330\n@@WHO\n{who}@@LAST\n{last}@@LASTB\n{lastb}@@AUTH\n{auth}@@END\n")
}

#[test]
fn recent_logins_carry_their_source_and_a_time_in_the_hosts_own_zone() {
    let last = "root     pts/0        185.211.57.152   Sun Nov  8 07:01:12 2026   still logged in\n\
ali      pts/1        147.79.115.80    Sat Nov  7 18:20:00 2026 - Sat Nov  7 18:25:30 2026  (00:05)\n\
deploy   pts/2        0.0.0.0          Sat Nov  7 10:00:00 2026 - down   (00:30)\n\
reboot   system boot  6.2.0-39-generic Sat Nov  7 09:59:50 2026   still running\n\
\n\
wtmp begins Fri Apr 26 07:34:20 2024\n";
    let r = logins::parse(host_id(), 0, &login_out("", last, "", "none\n")).unwrap().unwrap();
    assert_eq!(r.recent.len(), 3, "reboot and the wtmp line are not logins");
    let a = &r.recent[0];
    assert_eq!((a.user.as_str(), a.tty.as_str(), a.from.as_deref(), a.ended.as_str()), ("root", "pts/0", Some("185.211.57.152"), "still logged in"));
    assert_eq!(a.started_ms, Some(1_794_108_672_000), "07:01:12 at +03:30 is 03:31:12 UTC");
    assert_eq!(a.started, "Sun Nov 8 07:01:12 2026");
    let b = &r.recent[1];
    assert!(b.ended.starts_with("- Sat Nov 7 18:25:30 2026") && b.ended.contains("(00:05)"));
    assert_eq!(r.recent[2].from, None, "0.0.0.0 means no address was recorded");
}

#[test]
fn current_sessions_are_read_in_both_who_formats() {
    let who = "ali pts/0 2026-11-08 07:01 (65.109.190.229)\nali pts/1 2026-11-03 07:03 (tmux(3725).%0)\nroot tty1 2026-11-08 06:00\nold pts/9 Nov  8 07:01 (1.2.3.4)\n";
    let r = logins::parse(host_id(), 0, &login_out(who, "", "", "none\n")).unwrap().unwrap();
    assert_eq!(r.sessions.len(), 4);
    assert_eq!((r.sessions[0].from.as_deref(), r.sessions[0].started_ms), (Some("65.109.190.229"), Some(1_794_108_660_000)));
    assert_eq!(r.sessions[1].from.as_deref(), Some("tmux(3725).%0"));
    assert_eq!(r.sessions[2].from, None);
    assert_eq!((r.sessions[3].started_ms, r.sessions[3].started.as_str()), (None, "Nov 8 07:01"));
}

#[test]
fn ssh_events_are_read_from_the_journal_and_from_a_syslog_file_without_calling_any_suspicious() {
    let journal = "source journal\n\
2026-11-08T07:01:12+0330 web sshd[1234]: Accepted publickey for ali from 147.79.115.80 port 51000 ssh2: ED25519 SHA256:abc\n\
2026-11-08T07:02:00+0330 web sshd[1240]: Failed password for root from 185.211.57.152 port 40000 ssh2\n\
2026-11-08T07:02:05+0330 web sshd[1241]: Failed password for invalid user admin from 185.211.57.152 port 40001 ssh2\n\
2026-11-08T07:02:09+0330 web sshd[1242]: Invalid user test from 5.34.214.88 port 3333\n\
2026-11-08T07:03:00+0330 web sshd[1243]: pam_unix(sshd:auth): authentication failure; logname= uid=0 rhost=185.211.57.152\n";
    let r = logins::parse(host_id(), 0, &login_out("", "", "", journal)).unwrap().unwrap();
    assert_eq!(r.auth_source.as_deref(), Some("journal"));
    assert_eq!(r.auth.len(), 4, "a PAM line repeats a failure already listed and is not an event of its own");
    // Newest first.
    let kinds: Vec<_> = r.auth.iter().map(|e| (e.kind, e.user.as_deref(), e.from.as_deref())).collect();
    assert_eq!(kinds, [(AuthKind::InvalidUser, Some("test"), Some("5.34.214.88")), (AuthKind::InvalidUser, Some("admin"), Some("185.211.57.152")), (AuthKind::Failed, Some("root"), Some("185.211.57.152")), (AuthKind::Accepted, Some("ali"), Some("147.79.115.80"))]);
    assert_eq!(r.auth[3].method.as_deref(), Some("publickey"));
    assert_eq!(r.auth[2].method.as_deref(), Some("password"));
    assert_eq!(r.auth[3].time, "2026-11-08T07:01:12+0330");
    let file = "source /var/log/auth.log\nNov  8 07:01:12 web sshd[1234]: Accepted password for deploy from 10.0.0.5 port 22 ssh2\n";
    let r = logins::parse(host_id(), 0, &login_out("", "", "", file)).unwrap().unwrap();
    assert_eq!((r.auth[0].kind, r.auth[0].user.as_deref(), r.auth[0].time.as_str()), (AuthKind::Accepted, Some("deploy"), "Nov 8 07:01:12"));
}

#[test]
fn what_could_not_be_read_is_listed_with_the_reason() {
    let r = logins::parse(host_id(), 0, &login_out("ali pts/0 2026-11-08 07:01\n", "ali pts/0 1.2.3.4 Sun Nov  8 07:01:12 2026   still logged in\n", "lastb: cannot open /var/log/btmp: Permission denied\n", "denied journal\nnone\n")).unwrap().unwrap();
    assert_eq!(r.sessions.len(), 1);
    assert_eq!(r.recent.len(), 1);
    assert!(r.failed_logins.is_empty());
    let whats: Vec<_> = r.limits.iter().map(|l| l.what.as_str()).collect();
    assert_eq!(whats, ["Failed login attempts", "SSH authentication events", "SSH authentication events"]);
    assert!(r.limits[0].reason.contains("/var/log/btmp") && r.limits[0].reason.contains("root"));
    assert!(r.limits[1].reason.contains("journal") && r.limits[1].reason.contains("cannot be read"));
    // A journal that was denied but an auth log that could be read: the events are shown, the limit is gone.
    let r = logins::parse(host_id(), 0, &login_out("", "", "", "denied journal\nsource /var/log/auth.log\nNov  8 07:01:12 web sshd[1]: Failed password for bob from 1.1.1.1 port 1 ssh2\n")).unwrap().unwrap();
    assert!(r.limits.is_empty() && r.auth.len() == 1);
    // A missing tool is said to be missing.
    let r = logins::parse(host_id(), 0, &login_out("missing who\n", "missing last\n", "missing lastb\n", "none\n")).unwrap().unwrap();
    assert!(r.limits.iter().any(|l| l.what == "Recent logins" && l.reason.contains("not installed")));
    assert!(r.recent.is_empty() && r.sessions.is_empty());
}

#[test]
fn failed_logins_from_lastb_are_listed_when_readable() {
    let lastb = "admin    ssh:notty    185.211.57.152   Sun Nov  8 06:59:00 2026 - Sun Nov  8 06:59:00 2026  (00:00)\nroot     ssh:notty    5.34.214.88      Sun Nov  8 06:58:00 2026 - Sun Nov  8 06:58:00 2026  (00:00)\n\nbtmp begins Sat Nov  1 00:03:12 2026\n";
    let r = logins::parse(host_id(), 0, &login_out("", "", lastb, "none\n")).unwrap().unwrap();
    assert_eq!(r.failed_logins.iter().map(|e| (e.user.as_str(), e.from.as_deref())).collect::<Vec<_>>(), [("admin", Some("185.211.57.152")), ("root", Some("5.34.214.88"))]);
}

#[test]
fn another_system_has_no_login_report() {
    assert_eq!(logins::parse(host_id(), 0, "@@OS\nFreeBSD\n@@END\n").unwrap(), None);
}

#[cfg(target_os = "linux")]
#[test]
fn the_real_login_script_runs_here_and_parses_whatever_this_machine_allows() {
    use std::process::Command;
    let out = Command::new("sh").arg("-c").arg(logins::SCRIPT).output().expect("sh");
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    let r = logins::parse(host_id(), 0, &String::from_utf8_lossy(&out.stdout)).expect("parses").expect("Linux");
    // Whatever it could read is well formed; whatever it could not has a reason.
    for e in r.sessions.iter().chain(&r.recent) {
        assert!(!e.user.is_empty() && !e.tty.is_empty(), "{e:?}");
    }
    assert!(r.limits.iter().all(|l| !l.what.is_empty() && !l.reason.is_empty()));
}

// -- the manager's on-demand views -------------------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn nvidia_is_asked_only_once_known_and_not_more_often_than_every_five_seconds() {
    let tools = |f: &Fx| f.out().replace("@@TOOLS\nss\n", "@@TOOLS\nnvidia-smi\nss\n");
    let nv = |n: u64| format!("@@NVIDIA\n0, GPU {n}, {n}, 1, 1, 2, 3, 4, 5, 6, 7, GPU-a\nrc=0\n@@NVAPPS\n");
    let fake = Fake::scripted(vec![
        Ok(tools(&Fx::host())),
        Ok(tools(&Fx::host().at(1002.0)).replace("@@END\n", &format!("{}@@END\n", nv(10)))),
        Ok(tools(&Fx::host().at(1004.0))),
        Ok(tools(&Fx::host().at(1006.0))),
        Ok(tools(&Fx::host().at(1008.0)).replace("@@END\n", &format!("{}@@END\n", nv(20)))),
    ]);
    let m = mgr(&fake);
    let id = host_id();
    let s1 = m.sample(id, SEC).await.unwrap();
    assert!(s1.capabilities.nvidia_smi && s1.gpus.is_empty(), "the first reading only learns that the tool is there");
    tokio::time::advance(Duration::from_secs(2)).await;
    let s2 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s2.gpus.len(), 1);
    tokio::time::advance(Duration::from_secs(2)).await;
    let s3 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s3.gpus, s2.gpus, "between asks the earlier answer stands");
    tokio::time::advance(Duration::from_secs(2)).await;
    let s4 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s4.gpus, s2.gpus);
    tokio::time::advance(Duration::from_secs(2)).await;
    let s5 = m.sample(id, SEC).await.unwrap();
    assert_eq!(s5.gpus[0].name, "GPU 20", "asked again after five seconds");
    let asked: Vec<bool> = fake.scripts.lock().unwrap().iter().map(|s| s.starts_with("NV=1")).collect();
    assert_eq!(asked, [false, true, false, false, true]);
}

#[tokio::test(start_paused = true)]
async fn processes_are_read_twice_the_first_time_then_once_and_shared() {
    let a = vec![stat(812, "sshd", "S", 1, 1000, 1, 100, 500)];
    let b = vec![stat(812, "sshd", "S", 1, 1050, 1, 100, 500)];
    let c = vec![stat(812, "sshd", "S", 1, 1150, 1, 100, 500)];
    let fake = Fake::scripted(vec![Ok(proc_out(Some((1000.0, &a)), (1002.0, &b), "")), Ok(proc_out(None, (1006.0, &c), ""))]);
    let m = Arc::new(mgr(&fake));
    let id = host_id();
    let first = m.processes(id, Duration::from_secs(2)).await.unwrap();
    assert_eq!(first.items[0].cpu_pct, Some(25.0), "figures at once");
    assert!(fake.scripts.lock().unwrap()[0].starts_with("TWICE=1"));
    // Ten views asking together share one reading.
    tokio::time::advance(Duration::from_secs(3)).await;
    let asks: Vec<_> = (0..10).map(|_| { let m = Arc::clone(&m); tokio::spawn(async move { m.processes(id, Duration::from_secs(2)).await }) }).collect();
    let mut lists = Vec::new();
    for t in asks {
        lists.push(t.await.unwrap().unwrap());
    }
    assert!(lists.iter().all(|l| Arc::ptr_eq(l, &lists[0])));
    assert_eq!(fake.calls(), 2);
    assert!(!fake.scripts.lock().unwrap()[1].contains("TWICE=1"), "later lists are compared with the one before");
    // 1150 - 1050 ticks over the 4 s between the two stored snapshots (1002 -> 1006) = 25%.
    assert_eq!(lists[0].items[0].cpu_pct, Some(25.0));
}

#[tokio::test(start_paused = true)]
async fn views_of_a_system_without_a_collector_say_so_and_a_failing_host_is_not_asked() {
    let fake = Fake::scripted(vec![Ok("@@OS\nDarwin\n@@END\n".to_string())]);
    let m = mgr(&fake);
    let e = m.processes(host_id(), SEC).await.unwrap_err();
    assert!(e.to_string().contains("only Linux"), "{e}");
    assert!(m.ports(host_id(), SEC).await.unwrap_err().to_string().contains("Linux"));
    assert!(m.logins(host_id(), SEC).await.unwrap_err().to_string().contains("Linux"));
    // A host that just failed to give a reading is not asked for a view straight after.
    let fake = Fake::scripted(vec![Err("connection refused".into())]);
    let m = mgr(&fake);
    m.sample(host_id(), SEC).await.unwrap_err();
    let before = fake.calls();
    assert!(matches!(m.ports(host_id(), SEC).await.unwrap_err(), MetricsError::Cooling { .. }));
    assert_eq!(fake.calls(), before);
}

#[tokio::test(start_paused = true)]
async fn a_signal_is_one_attempt_whose_outcome_is_reported_and_whose_timeout_is_unknown() {
    let fake = Fake::scripted(vec![Ok("RESULT=ok\n@@END\n".into())]);
    let m = mgr(&fake);
    assert_eq!(m.signal(host_id(), 812, 100, "sshd", "TERM").await.unwrap(), SignalOutcome::Sent);
    assert!(fake.scripts.lock().unwrap()[0].contains("kill -TERM"));
    // Bad input never reaches the host.
    assert_eq!(m.signal(host_id(), 1, 100, "init", "KILL").await.unwrap_err(), SignalError::BadPid);
    assert_eq!(m.signal(host_id(), 5, 100, "a'b", "KILL").await.unwrap_err(), SignalError::BadName);
    assert_eq!(fake.calls(), 1);
    // The host's refusal is passed on in its words.
    let fake = Fake::scripted(vec![Ok("RESULT=refused:kill: Operation not permitted\n".into())]);
    assert_eq!(mgr(&fake).signal(host_id(), 5, 1, "x", "KILL").await.unwrap(), SignalOutcome::Refused { reason: "kill: Operation not permitted".into() });
    // No answer: unknown, and not sent again.
    let hang = Fake::new(|_, _| Box::pin(std::future::pending()));
    let m = HostMetricsManager::new(Arc::clone(&hang), Config { timeout: Duration::from_secs(5), ..Config::default() }, None);
    assert_eq!(m.signal(host_id(), 5, 1, "x", "KILL").await.unwrap(), SignalOutcome::Unknown);
    assert_eq!(hang.calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn after_a_signal_the_cached_process_list_is_not_served_again() {
    let a = vec![stat(812, "sshd", "S", 1, 1000, 1, 100, 500)];
    let fake = Fake::scripted(vec![Ok(proc_out(Some((1000.0, &a)), (1002.0, &a), "")), Ok("RESULT=ok\n".into()), Ok(proc_out(None, (1004.0, &a), ""))]);
    let m = mgr(&fake);
    m.processes(host_id(), Duration::from_secs(60)).await.unwrap();
    m.signal(host_id(), 812, 100, "sshd", "HUP").await.unwrap();
    m.processes(host_id(), Duration::from_secs(60)).await.unwrap();
    assert_eq!(fake.calls(), 3, "the list after the signal was read again, not taken from the cache");
}

#[allow(dead_code)]
fn unused(_: Uuid) {}
