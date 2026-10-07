// Mirrors the Rust types in src-tauri/src/{vault,models,session,commands}.rs.

export type Uuid = string;

export interface VaultRecord<T> {
  id: Uuid;
  /** Revision; saves send it back so edits from other devices aren't overwritten. */
  rev: number;
  updated_at: number;
  deleted: boolean;
  /** Device that wrote this revision. */
  device_id: Uuid;
  data?: T;
}

export type AuthMethod =
  | { type: "password"; password: string }
  | { type: "private_key"; private_key: string; passphrase?: string; certificate?: string }
  /** A key from the Key Manager. */
  | { type: "key"; key_id: Uuid }
  /** A key file on this computer, referenced by path. */
  | { type: "key_file"; path: string; passphrase?: string }
  | { type: "agent" };

export interface Identity {
  label: string;
  username: string;
  auth: AuthMethod;
  notes: string;
  /** Set when the credentials were entered in that host's own form. */
  for_host?: Uuid;
}

/** An identity's secrets, shown after the master password is confirmed. */
export interface Revealed {
  password: string | null;
  private_key: string | null;
  passphrase: string | null;
}

/** Secret-free auth sent from the host form. Empty secrets keep the stored ones. */
export type InlineAuth =
  | { type: "password"; password: string | null }
  | { type: "private_key"; private_key: string | null; passphrase: string | null }
  | { type: "key_file"; path: string; passphrase: string | null }
  | { type: "generate_key" }
  | { type: "key"; key_id: Uuid }
  | { type: "agent" };

export type HostCredentials =
  | { mode: "keep" }
  | { mode: "ask" }
  | { mode: "identity"; identity_id: Uuid }
  | { mode: "inline"; username: string; auth: InlineAuth; save_to_keychain: string | null };

export interface TerminalProfile {
  /** Id of a built-in or imported terminal theme. */
  theme?: string;
  font_size?: number;
  scrollback?: number;
}

export interface Host {
  label: string;
  hostname: string;
  port: number;
  identity_id?: Uuid;
  /** Host to tunnel through first, like OpenSSH ProxyJump. May itself have a jump. */
  jump_host_id?: Uuid;
  /** Forward this computer's ssh-agent (ssh -A). */
  forward_agent?: boolean;
  /** Show the host's graphical programs on this computer (ssh -X). */
  forward_x11?: boolean;
  /** "production", "staging", "development", or empty. */
  environment?: string;
  /** Typed into the shell right after connecting. */
  startup_command?: string;
  /** How this host's terminal looks; anything missing follows the settings. */
  profile?: TerminalProfile;
  /** Slash-separated group path, e.g. "Production/Databases". */
  group: string;
  tags: string[];
  color?: string;
  notes: string;
  favorite?: boolean;
  /** Proxy (SOCKS5, HTTP or ProxyCommand) used to reach this host or its first jump. */
  proxy_id?: Uuid;
  /** Connect without the group's default jump host. */
  no_group_jump?: boolean;
  /** Connect without the group's default proxy. */
  no_group_proxy?: boolean;
  /** Keep-alive interval in seconds (ServerAliveInterval); 0 disables. */
  keepalive_secs?: number;
  /** Free-form metadata, e.g. owner or ticket. */
  custom?: Record<string, string>;
  /** "telnet" for Telnet hosts; absent or empty means SSH. */
  protocol?: string;
  /** Smart completion on this host: absent follows the settings (production hosts: history only). */
  completion?: HostCompletion;
  /** Connect with Mosh (needs `mosh-client` here and `mosh-server` on the host). */
  mosh?: boolean;
  /** How to browse this host's files: absent for SFTP, "scp" when the server has SFTP off. */
  file_protocol?: "scp";
  /** Remote Desktop settings, for hosts whose protocol is "rdp". */
  rdp?: RdpOptions;
  /** FTP settings, for hosts whose protocol is "ftp". */
  ftp?: FtpOptions;
}

export type HostCompletion = "on" | "off" | "history";

export type FtpTls = "none" | "explicit" | "implicit";

export interface FtpOptions {
  tls: FtpTls;
  anonymous: boolean;
  /** SHA-256 of the server's certificate once trusted. */
  cert_sha256?: string;
}

export function emptyFtp(): FtpOptions {
  return { tls: "explicit", anonymous: false };
}

export type RdpSecurity = "auto" | "nla" | "tls";

export interface RdpOptions {
  domain?: string;
  /** 0 × 0 fits the tab. */
  width: number;
  height: number;
  color_depth: 16 | 32;
  security: RdpSecurity;
  /** SHA-256 of the server's certificate once trusted. */
  cert_sha256?: string;
}

export function emptyRdp(): RdpOptions {
  return { width: 0, height: 0, color_depth: 32, security: "auto" };
}

/** What a server's certificate looks like, from the error that asks whether to trust it. */
export interface RdpCertificate {
  fingerprint: string;
  subject: string;
  issuer: string;
  not_before: string;
  not_after: string;
}

/** A serial line, e.g. 115200 8N1. */
export interface SerialConfig {
  path: string;
  baud: number;
  data_bits: number;
  parity: "none" | "odd" | "even";
  stop_bits: number;
  flow: "none" | "software" | "hardware";
}

export interface Snippet {
  label: string;
  command: string;
  description: string;
  /** Slash-separated folder, e.g. "Kubernetes/Debug". */
  folder?: string;
  tags?: string[];
}

export type Collection =
  | "hosts"
  | "identities"
  | "snippets"
  | "forwards"
  | "groups"
  | "keys"
  | "known_hosts"
  | "proxies"
  | "workspaces"
  | "databases"
  | "settings"
  | "devices";

export type ForwardKind =
  | { kind: "local"; bind_addr: string; bind_port: number; dest_host: string; dest_port: number }
  | { kind: "remote"; bind_addr: string; bind_port: number; dest_host: string; dest_port: number }
  | { kind: "dynamic"; bind_addr: string; bind_port: number };

/** A saved port-forwarding rule. Mirrors models::ForwardRule (kind is flattened). */
export type ForwardRule = { label: string; host_id: Uuid; auto_start: boolean } & ForwardKind;

export type ForwardStatus =
  | { state: "starting" }
  | { state: "active"; port: number }
  | { state: "stopped" }
  | { state: "error"; message: string };

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  modified: number | null;
  permissions: number | null;
}

export type TransferProgress =
  | { state: "started"; total_bytes: number; total_files: number }
  | { state: "progress"; bytes: number; total_bytes: number; files_done: number; total_files: number; current: string }
  | { state: "paused"; bytes: number; total_bytes: number }
  | { state: "done"; bytes: number; files: number }
  | { state: "failed"; message: string }
  | { state: "cancelled" }
  /** Frontend only: waiting for a free slot in the queue. */
  | { state: "queued" };

export type VaultStatus =
  | { state: "not_configured" }
  | { state: "needs_setup"; path: string }
  | { state: "locked"; path: string; remembered: boolean; needs_upgrade: boolean }
  | { state: "unlocked"; path: string; vault_id: Uuid };

export interface ApiError {
  code: string;
  message: string;
  /** For code "conflict": what clashed. For "rdp_certificate_unknown": the certificate. */
  details?: ConflictDetails | RdpCertificate | { expected: string; found: RdpCertificate };
}

export interface ConflictDetails {
  collection: Collection;
  id: Uuid;
  reason: "changed_elsewhere" | "deleted_elsewhere" | "already_exists";
  fields: string[];
  their_device: Uuid;
  their_rev: number;
  their_updated_at: number;
}

export interface UnlockReport {
  migrated: boolean;
  merged_conflicts: number;
  open_conflicts: number;
  backed_up: boolean;
}

export interface UnlockResult {
  status: VaultStatus;
  report: UnlockReport;
  keychain_error: string | null;
}

export interface CreateResult {
  status: VaultStatus;
  /** Shown once; never stored. */
  recovery_key: string | null;
  keychain_error: string | null;
}

export interface ActiveSession {
  device_id: Uuid;
  device_name: string;
  since: number;
  expires_at: number;
}

export interface DeviceRecord {
  name: string;
  platform: string;
  app_version: string;
  first_seen: number;
  last_seen: number;
}

export interface VaultInfo {
  vault_id: Uuid;
  path: string;
  format_version: number;
  cipher: string;
  kdf: string;
  has_recovery: boolean;
  device_id: Uuid;
  device_name: string;
  remembered: boolean;
  state_hash: string;
  records: number;
  last_change: number;
  last_backup: number | null;
  active_sessions: ActiveSession[];
  devices: VaultRecord<DeviceRecord>[];
  open_conflicts: number;
}

export interface BackupInfo {
  file_name: string;
  created_at: number;
  size: number;
  /** null when the backup failed verification. */
  device_name: string | null;
  reason: string | null;
  records: number;
  error: string | null;
}

export interface RestoreReport {
  restored: number;
  removed: number;
  unchanged: number;
  /** Taken just before restoring, to undo it. */
  safety_backup: string;
}

export interface IntegrityReport {
  ok: boolean;
  format_version: number;
  records_checked: number;
  backups_ok: number;
  backups_bad: number;
  conflict_copies: number;
  errors: string[];
  warnings: string[];
}

export interface ConflictInfo {
  collection: Collection;
  id: Uuid;
  file_name: string;
  current: VaultRecord<Record<string, unknown>>;
  other: VaultRecord<Record<string, unknown>>;
  /** Top-level fields that differ. */
  fields: string[];
}

export interface SshKey {
  name: string;
  algorithm: string;
  public_key: string;
  fingerprint: string;
  /** Empty string = present but hidden. Absent = public key only. */
  private_key?: string;
  passphrase?: string;
  encrypted: boolean;
  certificate?: string;
  comment: string;
  created_at: number;
  for_host?: Uuid;
  /** Whether the vault's SSH agent offers this key. Absent = off. */
  agent?: AgentUse;
}

export type AgentUse = "off" | "ask" | "allow";

export interface AgentStatus {
  running: boolean;
  /** Socket path (or named pipe on Windows) for SSH_AUTH_SOCK. */
  path: string | null;
  enabled: boolean;
}

export interface CliStatus {
  enabled: boolean;
  running: boolean;
  /** This program's path, to put on PATH as `sshvault`. */
  executable: string | null;
}

/** A `sshvault run` waiting for approval. */
export interface CliPrompt {
  request_id: Uuid;
  command: string;
  hosts: string[];
  production: number;
}

export interface AgentPrompt {
  request_id: Uuid;
  key_name: string;
  fingerprint: string;
  /** The server asking through agent forwarding; null for local programs. */
  origin: string | null;
}

export type KeyAlgorithm = "ed25519" | "ecdsa_p256" | "ecdsa_p384" | "rsa3072" | "rsa4096";

export interface KeyUsage {
  identities: { id: Uuid; label: string }[];
  hosts: { id: Uuid; label: string }[];
}

export interface HostGroup {
  path: string;
  default_identity_id?: Uuid;
  default_jump_host_id?: Uuid;
  proxy_id?: Uuid;
  environment?: string;
  color?: string;
  notes: string;
}

export type ProxySpec =
  | { kind: "socks5"; host: string; port: number; username?: string; password?: string }
  | { kind: "http"; host: string; port: number; username?: string; password?: string }
  | { kind: "command"; command: string; approved: boolean };

export interface Proxy {
  name: string;
  spec: ProxySpec;
}

/** Saved tabs. `tabs` holds the frontend's WorkspaceTab objects. */
export interface Workspace {
  name: string;
  tabs: unknown[];
}

export interface VaultSettings {
  backup_retention: number;
  destructive_patterns: string[];
  paste_confirm_lines: number;
  clipboard_clear_secs: number;
}

export interface ReplacedKey {
  algorithm: string;
  fingerprint: string;
  replaced_at: number;
  replaced_by: string;
}

/** A trusted server key stored in the vault. */
export interface VaultKnownHost {
  id: Uuid;
  rev: number;
  host: string;
  port: number;
  algorithm: string;
  public_key: string;
  fingerprint: string;
  trusted_at: number;
  trusted_by: string;
  history: ReplacedKey[];
  note: string;
}

/** Asked when a server's key is new or has changed. */
export interface HostKeyPrompt {
  request_id: Uuid;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  previous: { algorithm: string; fingerprint: string } | null;
}

export interface KnownHostsImport {
  added: number;
  existing: number;
  hashed: number;
  invalid: number;
}

/** Payload of the `vault:changed` event emitted by the Rust file watcher. */
export interface RecordChange {
  collection: Collection;
  id: Uuid;
  record: VaultRecord<unknown> | null;
  /** A sync conflicted copy appeared that couldn't be merged automatically. */
  conflict_copy?: boolean;
}

export function isApiError(e: unknown): e is ApiError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

/** A shell the backend found for local terminals. */
export interface LocalShell {
  id: string;
  label: string;
  argv: string[];
  kind: "native" | "wsl";
  is_default: boolean;
}

export function errorMessage(e: unknown): string {
  if (isApiError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

export function emptyHost(): Host {
  return { label: "", hostname: "", port: 22, group: "", tags: [], notes: "" };
}

export function emptyIdentity(): Identity {
  return { label: "", username: "", auth: { type: "password", password: "" }, notes: "" };
}

export function emptySnippet(): Snippet {
  return { label: "", command: "", description: "" };
}

export function emptyForward(hostId: Uuid = ""): ForwardRule {
  return {
    label: "",
    host_id: hostId,
    auto_start: false,
    kind: "local",
    bind_addr: "127.0.0.1",
    bind_port: 8080,
    dest_host: "127.0.0.1",
    dest_port: 80,
  };
}

export interface KnownHost {
  line: number;
  hosts: string[];
  hashed: boolean;
  algorithm: string;
  fingerprint: string | null;
}

export interface PublicKeyInfo {
  public_key: string;
  fingerprint: string;
  algorithm: string;
}

export interface GeneratedKey {
  private_key: string;
  public_key: string;
  fingerprint: string;
}

export interface ImportedHost {
  alias: string;
  hostname: string;
  port: number;
  user: string | null;
  identity_file: string | null;
  proxy_jump: string | null;
  forward_agent: boolean;
  forward_x11: boolean;
  /** Folder inside the import group, e.g. an Ansible group path. */
  group?: string | null;
  /** Imported unapproved; never runs until approved in Proxies. */
  proxy_command?: string | null;
  keepalive_secs?: number | null;
  forwards?: ForwardKind[];
  /** CSV import only: applied after the hosts are created. */
  tags?: string[];
  notes?: string;
}

export type KeyImport = "reference" | "copy";

export type Health =
  | { state: "up"; latency_ms: number; banner: string | null }
  | { state: "down"; reason: string }
  | { state: "via_jump" };

export type HealthResult = { host_id: Uuid } & Health;

export const ENVIRONMENTS = [
  { value: "", label: "None" },
  { value: "production", label: "Production", short: "PROD", tone: "danger" },
  { value: "staging", label: "Staging", short: "STG", tone: "warning" },
  { value: "development", label: "Development", short: "DEV", tone: "success" },
] as const;

/** Matches the Badge component's `tone` prop. */
export type BadgeTone = "neutral" | "accent" | "danger" | "warning" | "success";

export interface EnvInfo {
  value: string;
  label: string;
  short?: string;
  tone?: BadgeTone;
}

/** Known environments get their colour; custom ones an accent badge. */
export function envInfo(value: string | undefined): EnvInfo {
  const v = value ?? "";
  const known = ENVIRONMENTS.find((e) => e.value === v);
  if (known) return known;
  return { value: v, label: v, short: v.slice(0, 5).toUpperCase(), tone: "accent" };
}

export interface ExecOutput {
  stdout: string;
  stderr: string;
  exit_code: number | null;
  truncated: boolean;
  duration_ms: number;
}

export type RunEvent =
  | { event: "started"; host_id: Uuid }
  | { event: "finished"; host_id: Uuid; output: ExecOutput }
  | { event: "failed"; host_id: Uuid; message: string }
  | { event: "done" };

export type EditEvent =
  | { edit_id: string; state: "uploaded"; bytes: number }
  | { edit_id: string; state: "failed"; message: string };

export interface SshConfigPreview {
  path: string;
  hosts: ImportedHost[];
  warnings: string[];
  existing: string[];
}

export interface ImportSummary {
  hosts_created: number;
  identities_created: number;
  keys_imported: number;
  proxies_created: number;
  forwards_created: number;
  skipped_existing: string[];
  warnings: string[];
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/** One-line summary like "L 127.0.0.1:8080 → db:5432". */
export function describeForward(r: ForwardKind): string {
  switch (r.kind) {
    case "local":
      return `L ${r.bind_addr}:${r.bind_port} → ${r.dest_host}:${r.dest_port}`;
    case "remote":
      return `R remote ${r.bind_addr}:${r.bind_port} → ${r.dest_host}:${r.dest_port}`;
    case "dynamic":
      return `D SOCKS5 ${r.bind_addr}:${r.bind_port}`;
  }
}

// -- Databases ---------------------------------------------------------

export type DbTls = "disable" | "require" | "verify_full";

export interface DbConnection {
  name: string;
  /** Which driver; "mysql" also covers MariaDB. */
  engine: string;
  host: string;
  port: number;
  username: string;
  /** Lists carry "" when one is stored; an empty password on save keeps it. */
  password?: string;
  database: string;
  tls: DbTls;
  /** Go through this saved SSH host, so the database port is never exposed. */
  ssh_host_id?: Uuid;
  group: string;
  environment?: string;
  notes: string;
}

export const DB_ENGINES: { value: string; label: string; port: number }[] = [
  { value: "mysql", label: "MySQL / MariaDB", port: 3306 },
  { value: "postgres", label: "PostgreSQL", port: 5432 },
];

export function emptyDbConnection(): DbConnection {
  return { name: "", engine: "mysql", host: "", port: 3306, username: "", password: undefined, database: "", tls: "verify_full", group: "", notes: "" };
}

export type DbColumnKind = "number" | "text" | "json" | "date_time" | "binary" | "other";

export interface DbColumn {
  name: string;
  data_type: string;
  kind: DbColumnKind;
}

/** A cell the server sent in full, or one cut short (read-only). */
export type DbCell = string | number | boolean | null | { truncated: true; preview: string; bytes: number };

export interface DbQueryResult {
  columns: DbColumn[];
  rows: DbCell[][];
  truncated: boolean;
  affected_rows: number | null;
  last_insert_id: number | null;
  elapsed_ms: number;
}

export type DbNodeKind = "database" | "schema" | "table" | "view" | "column" | "index";

export interface DbTreeNode {
  name: string;
  kind: DbNodeKind;
  detail?: string;
  expandable: boolean;
}

export interface DbTableInfo {
  columns: { name: string; data_type: string; nullable: boolean; primary_key: boolean; default: string | null }[];
  primary_key: string[];
}

export interface DbCellEdit {
  column: string;
  /** null is NULL */
  value: string | null;
}

export interface DbRowEdit {
  database: string;
  table: string;
  key: DbCellEdit[];
  changes: DbCellEdit[];
}

// -- Containers ----------------------------------------------------------

export type ContainerRuntime = "docker" | "podman" | "nerdctl";
export const CONTAINER_RUNTIMES: ContainerRuntime[] = ["docker", "podman", "nerdctl"];

export type ContainerState = "running" | "paused" | "restarting" | "exited" | "created" | "dead" | "removing" | "unknown";

export interface PortMapping {
  /** Empty when the port is exposed but not published. */
  host_ip: string;
  host_port: string;
  container_port: string;
  proto: string;
}

export interface ContainerInfo {
  /** The full ID. */
  id: string;
  name: string;
  image: string;
  state: ContainerState;
  status: string;
  ports: PortMapping[];
  /** Seconds since the Unix epoch. */
  created: number | null;
  size: string | null;
  labels: Record<string, string>;
  command: string;
  pod: string | null;
  /** Volume names and bind-mount source paths. */
  mounts: string[];
  networks: string[];
}

export interface ContainerVolume {
  name: string;
  driver: string;
  mountpoint: string;
  scope: string;
  labels: Record<string, string>;
  /** Only when sizes were asked for. */
  size: string | null;
  /** Containers (running or not) that mount it. */
  used_by: string[];
}

export interface ContainerNetwork {
  id: string;
  name: string;
  driver: string;
  scope: string;
  internal: boolean;
  ipv6: boolean;
  created: number | null;
  labels: Record<string, string>;
  /** bridge, host, none: the runtime won't remove them. */
  predefined: boolean;
  used_by: string[];
}

export interface ContainerResources {
  volumes: ContainerVolume[];
  networks: ContainerNetwork[];
  sizes_error: string | null;
}

export type ResourceKind = "image" | "volume" | "network";
export type PruneKind = { kind: "images"; all: boolean } | { kind: "volumes" } | { kind: "networks" };
export type ComposeVerb = "start" | "stop" | "restart" | "down";

export interface PruneItem {
  /** What is handed to the remove command. */
  id: string;
  label: string;
  detail: string;
  /** The Compose project it belongs to. */
  project: string | null;
}

export interface PruneResult {
  id: string;
  ok: boolean;
  error: string | null;
}

export interface ContainerImage {
  id: string;
  repository: string;
  tag: string;
  size_text: string;
  size_bytes: number | null;
  created: number | null;
  containers: number | null;
}

export interface ContainerListing {
  containers: ContainerInfo[];
  images: ContainerImage[];
  images_error: string | null;
}

export type ContainerLogEvent = { event: "chunk"; text: string } | { event: "end"; code: number | null; error: string | null };

export type ContainerAction = { action: "start" } | { action: "stop" } | { action: "restart" } | { action: "remove"; force: boolean };

export interface ContainerLogOptions {
  tail: number;
  follow: boolean;
  timestamps: boolean;
}
