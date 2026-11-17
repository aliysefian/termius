import { ArrowLeftRight, Boxes, Code, Database, FolderSync, KeyRound, Network, Server, ShieldCheck, SquareTerminal, Terminal, Workflow, type LucideIcon } from "lucide-react";
import type { FeatureCategory } from "@/tutorials/types";

export type FeatureStatus = "Implemented" | "Partial" | "Experimental" | "Planned";

export interface DemoFeature {
  id: string;
  name: string;
  category: FeatureCategory;
  icon: LucideIcon;
  summary: string;
  useCase: string;
  status: FeatureStatus;
  docs: { slug: string; label: string };
  tutorial: string;
  /** The slice of that tutorial this feature loops. */
  clip: { tutorial: string; chapter: string };
}

/** Features with an animated demonstration. Every claim traces to the README or docs/USAGE.md. */
export const demoFeatures: DemoFeature[] = [
  {
    id: "connections",
    name: "SSH connections",
    category: "SSH & Connections",
    icon: Server,
    summary: "Password, SSH key, shared credential or ask-every-time login. Server fingerprints are trusted explicitly and pinned. Jump-host chains, proxies and agent forwarding sit on the same form.",
    useCase: "Reach a server behind a bastion without touching ~/.ssh/config.",
    status: "Implemented",
    docs: { slug: "connecting", label: "Connecting" },
    tutorial: "create-ssh-connection",
    clip: { tutorial: "create-ssh-connection", chapter: "Select it and connect" },
  },
  {
    id: "hosts",
    name: "Host management",
    category: "Host Management",
    icon: Boxes,
    summary: "Nested groups with default credentials, favorites, tags, notes, search and bulk edit. Import from ~/.ssh/config, Ansible, PuTTY, MobaXterm or CSV.",
    useCase: "Keep fifty servers findable by name, address or tag.",
    status: "Implemented",
    docs: { slug: "hosts", label: "Hosts and credentials" },
    tutorial: "host-management",
    clip: { tutorial: "host-management", chapter: "Search" },
  },
  {
    id: "terminal",
    name: "Tabbed terminal",
    category: "Terminal",
    icon: Terminal,
    summary: "Tabs stay connected in the background. Split any tab into up to six panes, search, record a session and use themes, with WebGL rendering.",
    useCase: "Watch logs in one pane while you restart a service in another.",
    status: "Implemented",
    docs: { slug: "terminal", label: "Terminals" },
    tutorial: "terminal-workflow",
    clip: { tutorial: "terminal-workflow", chapter: "Split the pane" },
  },
  {
    id: "tmux",
    name: "tmux manager",
    category: "tmux",
    icon: SquareTerminal,
    summary: "See sessions, windows and panes on a host with live read-only previews, then attach in one click. The app never types into your shell to do it.",
    useCase: "Find the long-running job you left in a session last week.",
    status: "Implemented",
    docs: { slug: "tmux", label: "tmux sessions" },
    tutorial: "tmux-sessions",
    clip: { tutorial: "tmux-sessions", chapter: "Pick a session" },
  },
  {
    id: "files",
    name: "Dual-pane file manager",
    category: "File Management",
    icon: FolderSync,
    summary: "SFTP, SCP and FTP/FTPS in one two-pane browser with drag and drop, a transfer queue, replace/skip/keep-both choices and editing remote files in your local editor.",
    useCase: "Ship a build to a server and check permissions without leaving the app.",
    status: "Implemented",
    docs: { slug: "sftp", label: "Files: SFTP, SCP and FTP" },
    tutorial: "sftp-file-transfer",
    clip: { tutorial: "sftp-file-transfer", chapter: "Upload with progress" },
  },
  {
    id: "tunnels",
    name: "Port forwarding",
    category: "Networking",
    icon: ArrowLeftRight,
    summary: "Local, remote and SOCKS5 tunnels with state, uptime and logs per rule. Create and start are separate; rules can start automatically after unlock.",
    useCase: "Reach a database that is not exposed to the internet.",
    status: "Implemented",
    docs: { slug: "port-forwarding", label: "Port forwarding" },
    tutorial: "port-forwarding",
    clip: { tutorial: "port-forwarding", chapter: "Create a tunnel" },
  },
  {
    id: "keys",
    name: "Key Manager and SSH agent",
    category: "Security",
    icon: KeyRound,
    summary: "Generate or import Ed25519, ECDSA and RSA keys. A built-in agent serves vault keys to ssh and git with per-key approval, and nothing is written to disk.",
    useCase: "Use one key for git and ssh without a private key file on the laptop.",
    status: "Implemented",
    docs: { slug: "keys-and-agent", label: "Keys and the SSH agent" },
    tutorial: "ssh-agent-keys",
    clip: { tutorial: "ssh-agent-keys", chapter: "Turn on the SSH agent" },
  },
  {
    id: "snippets",
    name: "Snippets on many hosts",
    category: "DevOps Tools",
    icon: Code,
    summary: "Saved commands with {{variables}}, run on up to eight hosts at a time with per-host exit code, duration and output.",
    useCase: "Check disk space across a fleet and compare the answers.",
    status: "Implemented",
    docs: { slug: "snippets", label: "Snippets" },
    tutorial: "run-on-many-hosts",
    clip: { tutorial: "run-on-many-hosts", chapter: "Pick the hosts" },
  },
];

export interface OtherFeature {
  name: string;
  category: FeatureCategory;
  icon: LucideIcon;
  text: string;
  status: FeatureStatus;
  note?: string;
}

/** Capabilities without a demonstration yet. Status words follow README, ROADMAP and docs/engineering/FEATURE_INVENTORY.md. */
export const otherFeatures: OtherFeature[] = [
  { name: "Encrypted, self-synced vault", category: "Security", icon: ShieldCheck, status: "Implemented", text: "Per-record XChaCha20-Poly1305, a random 256-bit vault key unlocked by Argon2id or a recovery key. Sync is a folder you already use." },
  { name: "Jump hosts, proxies, X11", category: "SSH & Connections", icon: Network, status: "Implemented", text: "ProxyJump chains, SOCKS5/HTTP proxies, ProxyCommand, agent and X11 forwarding, keep-alive and auto-reconnect." },
  { name: "Quick connect and command palette", category: "SSH & Connections", icon: Workflow, status: "Implemented", text: "Type user@host:port or an ssh -J command; search hosts, snippets and actions with Ctrl+Shift+P." },
  { name: "Databases", category: "DevOps Tools", icon: Database, status: "Implemented", text: "Browse and query MySQL, MariaDB, PostgreSQL, SQL Server, Oracle, rqlite, Redis, MongoDB and Elasticsearch, optionally through an SSH host." },
  { name: "Containers", category: "DevOps Tools", icon: Boxes, status: "Implemented", text: "Docker, Podman and nerdctl on this computer or any saved host, with nothing installed on the host." },
  { name: "Kubernetes", category: "DevOps Tools", icon: Boxes, status: "Partial", text: "Pods across contexts and namespaces, logs, describe, a shell in a pod and port-forwards using your kubectl.", note: "No deployments, services or events views yet." },
  { name: "Runbooks", category: "DevOps Tools", icon: Workflow, status: "Implemented", text: "Saved, checked sequences of steps run on many hosts with a dry run, live results, typed approval for production and a history." },
  { name: "Monitoring and alerts", category: "DevOps Tools", icon: Server, status: "Partial", text: "Fleet tiles, CPU, memory, disk, load and uptime over SSH; alerts when hosts stop answering or run hot.", note: "Polls only while the app is open. No severity levels or persisted alert history." },
  { name: "Mosh", category: "SSH & Connections", icon: Terminal, status: "Partial", text: "Roaming, sleep-proof sessions through the system mosh-client after logging in with vault keys." },
  { name: "Hardware keys (FIDO2, PKCS#11)", category: "Security", icon: KeyRound, status: "Experimental", text: "Logging in through your computer's ssh-agent.", note: "Tested against a real sshd with a stand-in for the device, not yet with a physical key." },
  { name: "Serial consoles", category: "SSH & Connections", icon: Terminal, status: "Experimental", text: "Serial consoles with port discovery and line settings, plus Telnet marked UNENCRYPTED.", note: "Unit-tested; not yet tried against real hardware." },
  { name: "Scripting CLI", category: "DevOps Tools", icon: Code, status: "Implemented", text: "sshvault status, list, connect and run, talking to the running app so no secret passes through the CLI." },
  { name: "Unlock the vault with a hardware key", category: "Security", icon: KeyRound, status: "Planned", text: "Not available yet. Listed on the roadmap." },
  { name: "Team sharing", category: "Security", icon: ShieldCheck, status: "Planned", text: "A design proposal only: a vault shared between people without a shared master password." },
  { name: "Notarized macOS app and signed installers", category: "Security", icon: ShieldCheck, status: "Planned", text: "Signing is built and switches on when keys are added; today installers are unsigned." },
];
