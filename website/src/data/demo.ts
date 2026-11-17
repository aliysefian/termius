/**
 * Fictional fixtures for the simulated demos. Addresses come from the documentation-only ranges
 * (RFC 5737: 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24). Nothing here is a real host, user or key.
 */
export type HostStatus = "connected" | "disconnected" | "unknown";

export interface DemoHost {
  id: string;
  name: string;
  hostname: string;
  username: string;
  port: number;
  status: HostStatus;
  group: string;
  env?: "PROD" | "STG" | "DEV";
  tags?: string[];
  favorite?: boolean;
}

export const demoHosts: DemoHost[] = [
  { id: "prod-api", name: "Production API", hostname: "192.0.2.10", username: "demo", port: 22, status: "connected", group: "Production", env: "PROD", tags: ["api", "eu"], favorite: true },
  { id: "web-01", name: "Web 01", hostname: "192.0.2.21", username: "demo", port: 22, status: "connected", group: "Production", env: "PROD", tags: ["web"] },
  { id: "db", name: "Database Server", hostname: "203.0.113.30", username: "admin", port: 22, status: "connected", group: "Production", env: "PROD", tags: ["postgres"] },
  { id: "dev", name: "Development Server", hostname: "198.51.100.20", username: "developer", port: 22, status: "disconnected", group: "Staging", env: "DEV", tags: ["dev"] },
  { id: "staging", name: "Staging Web", hostname: "198.51.100.44", username: "deploy", port: 22, status: "unknown", group: "Staging", env: "STG", tags: ["web"] },
  { id: "bastion", name: "Bastion", hostname: "203.0.113.5", username: "demo", port: 2222, status: "connected", group: "Infrastructure", tags: ["jump"] },
  { id: "build", name: "Build Runner", hostname: "198.51.100.77", username: "ci", port: 22, status: "disconnected", group: "Infrastructure", tags: ["ci"] },
];

export const hostById = (id: string) => demoHosts.find((h) => h.id === id);

export interface DemoFile {
  name: string;
  kind: "dir" | "file";
  size?: string;
  modified: string;
  mode?: string;
}

export const localFiles: DemoFile[] = [
  { name: "..", kind: "dir", modified: "" },
  { name: "Documents", kind: "dir", modified: "Nov 12 09:14" },
  { name: "Projects", kind: "dir", modified: "Nov 15 17:02" },
  { name: "release-notes.md", kind: "file", size: "4.2 KB", modified: "Nov 15 16:48" },
  { name: "backup.tar.gz", kind: "file", size: "18.6 MB", modified: "Nov 14 22:30" },
  { name: "deploy.sh", kind: "file", size: "1.1 KB", modified: "Nov 13 11:05" },
  { name: "report-q4.pdf", kind: "file", size: "612 KB", modified: "Nov 10 08:21" },
];

export const remoteRoot: DemoFile[] = [
  { name: "..", kind: "dir", modified: "" },
  { name: "backups", kind: "dir", modified: "Nov 16 03:00", mode: "drwxr-xr-x" },
  { name: "releases", kind: "dir", modified: "Nov 14 12:40", mode: "drwxr-xr-x" },
  { name: "logs", kind: "dir", modified: "Nov 16 12:01", mode: "drwxr-xr-x" },
  { name: "app.conf", kind: "file", size: "2.3 KB", modified: "Nov 12 10:22", mode: "-rw-r--r--" },
  { name: "README.txt", kind: "file", size: "812 B", modified: "Nov 01 09:00", mode: "-rw-r--r--" },
];

export const remoteReleases: DemoFile[] = [
  { name: "..", kind: "dir", modified: "" },
  { name: "v2.3.0", kind: "dir", modified: "Nov 14 12:40", mode: "drwxr-xr-x" },
  { name: "v2.2.4", kind: "dir", modified: "Oct 30 18:15", mode: "drwxr-xr-x" },
  { name: "current.txt", kind: "file", size: "7 B", modified: "Nov 14 12:41", mode: "-rw-r--r--" },
];

export interface DemoTmuxSession {
  id: string;
  name: string;
  attached: boolean;
  windows: { id: string; name: string; panes: { id: string; cmd: string; lines: string[] }[] }[];
}

export const demoTmux: DemoTmuxSession[] = [
  {
    id: "$0",
    name: "deploy",
    attached: false,
    windows: [
      {
        id: "@0",
        name: "editor",
        panes: [
          { id: "%0", cmd: "vim", lines: ["~", "~  app.conf", "~", "-- INSERT --"] },
          { id: "%1", cmd: "bash", lines: ["demo@prod-api:~$ git status", "On branch main", "nothing to commit"] },
        ],
      },
      {
        id: "@1",
        name: "logs",
        panes: [{ id: "%2", cmd: "tail", lines: ["12:01:07 GET /health 200 3ms", "12:01:09 GET /v1/hosts 200 18ms", "12:01:12 POST /v1/sync 202 41ms"] }],
      },
    ],
  },
  {
    id: "$1",
    name: "monitoring",
    attached: true,
    windows: [{ id: "@2", name: "htop", panes: [{ id: "%3", cmd: "htop", lines: ["CPU[|||||     ] 24.1%", "Mem[|||||||   ] 3.2G/8.0G", "Load average: 0.42 0.38 0.31"] }] }],
  },
  {
    id: "$2",
    name: "scratch",
    attached: false,
    windows: [{ id: "@3", name: "bash", panes: [{ id: "%4", cmd: "bash", lines: ["demo@prod-api:~$ "] }] }],
  },
];

export const demoSnippets = [
  { id: "uptime", name: "Uptime", command: "uptime", tags: ["system"] },
  { id: "disk", name: "Disk usage", command: "df -h /", tags: ["system"] },
  { id: "tail", name: "Tail service log", command: "journalctl -u {{service}} -n 20", tags: ["logs"] },
  { id: "restart", name: "Restart app", command: "sudo systemctl restart {{service}}", tags: ["ops"] },
];

export const demoKeys = [
  { id: "ed-main", name: "demo-laptop", type: "Ed25519", fingerprint: "SHA256:Zk3dEXAMPLEonly0000000000000000000000000fp1", used: 4 },
  { id: "ed-ci", name: "ci-deploy", type: "Ed25519", fingerprint: "SHA256:Qm8rEXAMPLEonly0000000000000000000000000fp2", used: 2 },
  { id: "rsa-old", name: "legacy-rsa", type: "RSA 4096", fingerprint: "SHA256:Vb1nEXAMPLEonly0000000000000000000000000fp3", used: 1 },
];

export const demoTunnels = [
  { id: "t-db", name: "Postgres (staging)", type: "Local", mapping: "127.0.0.1:5433 → db.internal:5432", host: "Bastion", state: "running", uptime: "2h 14m" },
  { id: "t-web", name: "Preview app", type: "Remote", mapping: "0.0.0.0:9000 ← localhost:3000", host: "Web 01", state: "stopped", uptime: "" },
];
