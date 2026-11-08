// @ts-nocheck: runs in the browser under test, not in the app.
// A pretend host for the Host Metrics scripts: readings, processes, ports and logins, and a way to make things fail.

const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const host = (id, label) => rec(id, { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', environment: 'staging', identity_id: id === 'h3' ? null : 'i1', favorite: false });
window.__calls = { sample: 0, history: 0, maxAges: [], connects: 0 };
window.__fail = null;
window.__signalOutcome = { kind: 'sent' };
window.__signals = [];
window.__gone = new Set();
let n = 0;
const GB = 1024 * 1024 * 1024;
const info = { hostname: 'claude', os_name: 'Ubuntu 22.04 LTS', kernel: '6.2.0-39-generic', arch: 'x86_64', cpu_model: 'Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz', sockets: 1, physical_cores: 6, logical_cpus: 12, max_mhz: 3600, base_mhz: 2400, virtualization: 'VT-x', hypervisor: 'KVM', caches: [{ level: 'L2', size: '1.5 MiB' }, { level: 'L3', size: '20 MiB' }] };
const caps = { proc_stat: true, meminfo: true, loadavg: true, net_dev: true, diskstats: true, sys_block: true, sys_net: true, filesystems: true, cpufreq: true, sensors: false, amd_gpu: false, nvidia_smi: false, tools: ['ss', 'ip'] };
const iface = (name, kind, rx, tx, counted, over = {}) => ({ name, kind, state: 'up', master: null, mtu: 1500, link_mbps: kind === 'physical' ? 1000 : null, rx_bps: rx, tx_bps: tx, rx_pps: null, tx_pps: null, rx_bytes: 24.8e9 + n * 1e7, tx_bytes: 8.7e9 + n * 3e6, rx_errors: 0, tx_errors: 2, rx_drops: 0, tx_drops: 1, counted, ...over });
function reading(id) {
  n++;
  const first = n === 1;
  const wave = (a, b, k) => a + (b - a) * (0.5 + 0.5 * Math.sin(n / k));
  const rx = first ? null : wave(2e6, 12.8e6, 3), tx = first ? null : wave(1e6, 3.4e6, 4);
  return {
    host_id: id, taken_at_ms: Date.now(), platform: { kind: 'linux' },
    baseline: first ? { kind: 'first' } : { kind: 'measured', elapsed_secs: 2 }, uptime_secs: 12 * 86400 + 4 * 3600 + n * 2,
    cpu: { total_pct: first ? null : wave(15, 35, 5), iowait_pct: first ? null : 0.4, steal_pct: first ? null : 0, logical: 12, freq_mhz: 2650,
      cores: Array.from({ length: 12 }, (_, i) => ({ index: i, pct: first ? null : wave(10, 80, 3 + i) })) },
    memory: { total: 16 * GB, used: 9.9 * GB, available: 6.1 * GB, free: 3 * GB, buffers: 0.2 * GB, cached: 2.9 * GB, shared: 0.3 * GB, swap_total: 4 * GB, swap_used: 1.2 * GB, committed: 11 * GB, slab: 0.6 * GB, used_pct: 61.9 + Math.sin(n / 4), available_exact: true },
    load: { m1: 1.42, m5: 1.18, m15: 0.92 }, processes: { processes: 187, threads: 412, running: 3 },
    filesystems: [
      { device: '/dev/sda2', mount: '/', fstype: 'ext4', total: 120e9, used: 92e9, available: 28e9, used_pct: 76.7 },
      { device: '/dev/sda3', mount: '/home', fstype: 'ext4', total: 200e9, used: 64e9, available: 136e9, used_pct: 32 },
      { device: '/dev/sda4', mount: '/var', fstype: 'xfs', total: 50e9, used: 47e9, available: 3e9, used_pct: 94 },
    ],
    disks: [
      { name: 'sda', model: 'Samsung SSD 860', size: 480e9, rotational: false, virtual_device: false, read_bps: first ? null : 1.2e6, write_bps: first ? null : 3.4e6, read_iops: first ? null : 40, write_iops: first ? null : 120, busy_pct: first ? null : 12.5, await_ms: first ? null : 0.8, read_total: 1e12, write_total: 2e12 },
      { name: 'dm-0', model: null, size: 100e9, rotational: false, virtual_device: true, read_bps: null, write_bps: null, read_iops: null, write_iops: null, busy_pct: null, await_ms: null, read_total: 0, write_total: 0 },
    ],
    network: { default_iface: 'eth0', total: first ? null : { rx_bps: rx, tx_bps: tx, from: ['eth0'] },
      interfaces: [iface('eth0', 'physical', rx, tx, true), iface('docker0', 'bridge', first ? null : 5e5, first ? null : 5e5, false), iface('wg0', 'tunnel', first ? null : 1e4, first ? null : 2e4, false)] },
    temperatures: [
      { source: 'hwmon', chip: 'coretemp', label: 'Package id 0', kind: 'cpu_package', celsius: 48, high: 80, critical: 100 },
      { source: 'hwmon', chip: 'coretemp', label: 'Core 0', kind: 'cpu_core', celsius: 46, high: 80, critical: 100 },
      { source: 'hwmon', chip: 'nvme', label: 'Composite', kind: 'storage', celsius: 42, high: 83, critical: 88 },
      { source: 'hwmon', chip: 'acpitz', label: 'temp1', kind: 'system', celsius: 38, high: null, critical: null },
      { source: 'hwmon', chip: 'nct6775', label: 'SYSTIN', kind: 'system', celsius: null, high: null, critical: null },
    ],
    gpus: [{ id: 'nvidia0', vendor: 'nvidia', name: 'NVIDIA GeForce RTX 3060', driver: '535.104.05', util_pct: first ? null : wave(10, 30, 4), mem_util_pct: 7, vram_used: 2.1 * GB, vram_total: 12 * GB, celsius: 56, power_watts: 62, power_limit_watts: 170, fan_pct: 35, processes: [{ pid: 4242, name: 'python3', vram: 1.8 * GB }] }],
    notes: [], capabilities: { ...caps, sensors: true, nvidia_smi: true }, info,
  };
}
const procs = () => [
  { pid: 1, ppid: 0, user: 'root', state: 'S', name: 'systemd', command: '/sbin/init', cpu_pct: 0.1, rss: 12e6, mem_pct: 0.1, threads: 1, start_ticks: 10, age_secs: 1e6 },
  { pid: 812, ppid: 1, user: 'root', state: 'S', name: 'sshd', command: '/usr/sbin/sshd -D', cpu_pct: 0.2, rss: 9e6, mem_pct: 0.1, threads: 1, start_ticks: 200, age_secs: 9e5 },
  { pid: 34721, ppid: 1, user: 'root', state: 'R', name: 'nginx', command: 'nginx: worker process', cpu_pct: 18.4, rss: 1.2e9, mem_pct: 7.2, threads: 4, start_ticks: 900, age_secs: 5000 },
  { pid: 35218, ppid: 34721, user: 'www-data', state: 'S', name: 'php-fpm', command: 'php-fpm: pool www', cpu_pct: 6.1, rss: 7e8, mem_pct: 4.8, threads: 1, start_ticks: 950, age_secs: 4000 },
  { pid: 400, ppid: 1, user: 'postgres', state: 'S', name: 'postgres', command: 'postgres: main', cpu_pct: null, rss: 3e8, mem_pct: 1.9, threads: 2, start_ticks: 300, age_secs: 8e5 },
].filter((p) => !window.__gone.has(p.pid));
window.__invoke = async (cmd, args) => {
  if (cmd === 'ssh_connect') window.__calls.connects++;
  if (cmd === 'list_hosts') return [host('h1', 'claude'), host('h2', 'mac-mini'), host('h3', 'no-creds'), host('h4', 'web-02')];
  if (cmd === 'list_identities') return [rec('i1', { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' })];
  if (cmd === 'check_hosts') return [];
  if (cmd === 'hostmetrics_processes') { (window.__calls.proc = window.__calls.proc || []).push(args.maxAgeMs); return { host_id: args.hostId, taken_at_ms: Date.now(), total: 5, truncated: false, baseline: { kind: 'measured', elapsed_secs: 3 }, commands: true, items: procs() }; }
  if (cmd === 'hostmetrics_signal') { window.__signals.push(args); const o = window.__signalOutcome; if (o.kind === 'sent') window.__gone.add(args.pid); return o; }
  if (cmd === 'hostmetrics_ports') return { host_id: args.hostId, taken_at_ms: Date.now(), source: 'ss', owners_hidden: true, items: [
    { proto: 'tcp', address: '0.0.0.0', port: 22, ipv6: false, pid: 812, process: 'sshd', user: 'root', state: 'LISTEN', local_only: false },
    { proto: 'tcp', address: '127.0.0.1', port: 5432, ipv6: false, pid: 27630, process: 'postgres', user: 'postgres', state: 'LISTEN', local_only: true },
    { proto: 'tcp', address: '::', port: 443, ipv6: true, pid: 34721, process: 'nginx', user: null, state: 'LISTEN', local_only: false },
    { proto: 'udp', address: '0.0.0.0', port: 51820, ipv6: false, pid: null, process: null, user: null, state: 'UNCONN', local_only: false },
  ] };
  if (cmd === 'hostmetrics_logins') return { host_id: args.hostId, taken_at_ms: Date.now(), auth_source: 'journal',
    sessions: [{ user: 'ali', tty: 'pts/0', from: '147.79.115.80', started_ms: Date.now() - 18 * 60000, started: '2026-11-08 07:01', ended: 'still logged in' }],
    recent: [{ user: 'root', tty: 'pts/1', from: '185.211.57.152', started_ms: Date.now() - 2 * 60000, started: 'Sun Nov 8 07:01:12 2026', ended: 'still logged in' }, { user: 'deploy', tty: 'pts/2', from: '10.0.0.5', started_ms: Date.now() - 82 * 60000, started: 'Sun Nov 8 05:40:00 2026', ended: '- Sun Nov 8 05:45:00 2026 (00:05)' }],
    failed_logins: [],
    auth: [{ kind: 'failed', user: 'root', from: '185.211.57.152', method: 'password', time: '2026-11-08T07:02:00+0330' }, { kind: 'invalid_user', user: 'admin', from: '185.211.57.152', method: 'password', time: '2026-11-08T07:02:05+0330' }, { kind: 'accepted', user: 'ali', from: '147.79.115.80', method: 'publickey', time: '2026-11-08T07:01:12+0330' }],
    limits: [{ what: 'Failed login attempts', reason: 'Failed logins are recorded in /var/log/btmp, which only root can read.' }] };
  if (cmd === 'hostmetrics_history') { window.__calls.history++; return []; }
  if (cmd === 'hostmetrics_sample') {
    window.__calls.sample++; window.__calls.maxAges.push(args.maxAgeMs);
    if (window.__fail) throw window.__fail;
    if (args.hostId === 'h2') return { ...reading('h2'), temperatures: [], gpus: [], platform: { kind: 'unsupported', os: 'Darwin' }, cpu: null, memory: null, load: null, processes: null, filesystems: [], disks: [], network: null, uptime_secs: null };
    return reading(args.hostId);
  }
  return undefined;
};
