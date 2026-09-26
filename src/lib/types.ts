// Mirrors the Rust types in src-tauri/src/{vault,models,session,commands}.rs.

export type Uuid = string;

export interface VaultRecord<T> {
  id: Uuid;
  updated_at: number;
  deleted: boolean;
  data?: T;
}

export type AuthMethod =
  | { type: "password"; password: string }
  | { type: "private_key"; private_key: string; passphrase?: string }
  | { type: "agent" };

export interface Identity {
  label: string;
  username: string;
  auth: AuthMethod;
  notes: string;
  /** Set when the credentials were entered in that host's own form. */
  for_host?: Uuid;
}

/** Secret-free auth sent from the host form. Empty secrets keep the stored ones. */
export type InlineAuth =
  | { type: "password"; password: string | null }
  | { type: "private_key"; private_key: string | null; passphrase: string | null }
  | { type: "key_file"; path: string; passphrase: string | null }
  | { type: "generate_key" }
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
  /** Slash-separated group path, e.g. "Production/Databases". */
  group: string;
  tags: string[];
  color?: string;
  notes: string;
}

export interface Snippet {
  label: string;
  command: string;
  description: string;
}

export type Collection = "hosts" | "identities" | "snippets" | "forwards";

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
  | { state: "done"; bytes: number; files: number }
  | { state: "failed"; message: string }
  | { state: "cancelled" };

export type VaultStatus =
  | { state: "not_configured" }
  | { state: "needs_setup"; path: string }
  | { state: "locked"; path: string }
  | { state: "unlocked"; path: string; vault_id: Uuid };

export interface ApiError {
  code: string;
  message: string;
}

/** Payload of the `vault:changed` event emitted by the Rust file watcher. */
export interface RecordChange {
  collection: Collection;
  id: Uuid;
  record: VaultRecord<unknown> | null;
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
}

export interface SshConfigPreview {
  path: string;
  hosts: ImportedHost[];
  warnings: string[];
  existing: string[];
}

export interface ImportSummary {
  hosts_created: number;
  identities_created: number;
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
