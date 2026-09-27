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
  /** Slash-separated group path, e.g. "Production/Databases". */
  group: string;
  tags: string[];
  color?: string;
  notes: string;
  favorite?: boolean;
  /** Proxy (SOCKS5, HTTP or ProxyCommand) used to reach this host or its first jump. */
  proxy_id?: Uuid;
  /** Keep-alive interval in seconds (ServerAliveInterval); 0 disables. */
  keepalive_secs?: number;
  /** Free-form metadata, e.g. owner or ticket. */
  custom?: Record<string, string>;
  /** "telnet" for Telnet hosts; absent or empty means SSH. */
  protocol?: string;
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
  /** For code "conflict": what clashed. */
  details?: ConflictDetails;
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
  { value: "production", label: "Production", short: "PROD", cls: "bg-danger/15 text-danger" },
  { value: "staging", label: "Staging", short: "STG", cls: "bg-warning/15 text-warning" },
  { value: "development", label: "Development", short: "DEV", cls: "bg-success/15 text-success" },
] as const;

export interface EnvInfo {
  value: string;
  label: string;
  short?: string;
  cls?: string;
}

/** Known environments get their colour; custom ones a neutral badge. */
export function envInfo(value: string | undefined): EnvInfo {
  const v = value ?? "";
  const known = ENVIRONMENTS.find((e) => e.value === v);
  if (known) return known;
  return { value: v, label: v, short: v.slice(0, 5).toUpperCase(), cls: "bg-accent/15 text-accent" };
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
