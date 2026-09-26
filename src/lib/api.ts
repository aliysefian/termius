// Typed wrappers over Tauri IPC. Keep this the only place that knows command names.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  HealthResult,
  Revealed,
  EditEvent,
  RunEvent,
  HostCredentials,
  GeneratedKey,
  ImportedHost,
  ImportSummary,
  KnownHost,
  PublicKeyInfo,
  SshConfigPreview,
  ForwardRule,
  ForwardStatus,
  Host,
  Identity,
  RecordChange,
  Snippet,
  Uuid,
  VaultRecord,
  VaultStatus,
} from "./types";

export const vault = {
  status: () => invoke<VaultStatus>("get_status"),
  setPath: (path: string) => invoke<VaultStatus>("set_vault_path", { path }),
  create: (password: string) => invoke<VaultStatus>("create_vault", { password }),
  unlock: (password: string) => invoke<VaultStatus>("unlock_vault", { password }),
  lock: () => invoke<VaultStatus>("lock_vault"),
  changePassword: (currentPassword: string, newPassword: string) =>
    invoke<void>("change_master_password", { currentPassword, newPassword }),
  onChanged: (handler: (c: RecordChange) => void): Promise<UnlistenFn> =>
    listen<RecordChange>("vault:changed", (e) => handler(e.payload)),
};

export const hosts = {
  list: () => invoke<VaultRecord<Host>[]>("list_hosts"),
  save: (id: Uuid | null, host: Host) => invoke<VaultRecord<Host>>("save_host", { id, host }),
  saveWithCredentials: (id: Uuid | null, host: Host, credentials: HostCredentials) =>
    invoke<{ host: VaultRecord<Host>; public_key: string | null }>("save_host_with_credentials", {
      id,
      host,
      credentials,
    }),
  delete: (id: Uuid) => invoke<void>("delete_host", { id }),
};

export const identities = {
  list: () => invoke<VaultRecord<Identity>[]>("list_identities"),
  save: (id: Uuid | null, identity: Identity) =>
    invoke<VaultRecord<Identity>>("save_identity", { id, identity }),
  delete: (id: Uuid) => invoke<void>("delete_identity", { id }),
  /** Stored secrets; needs the master password unless recently entered. */
  reveal: (id: Uuid, masterPassword: string | null) =>
    invoke<{ secret: Revealed; grace_secs: number }>("reveal_identity", { id, masterPassword }),
  revealClose: () => invoke<void>("reveal_close"),
  publicKey: (id: Uuid) => invoke<PublicKeyInfo>("identity_public_key", { id }),
};

export const keys = {
  generate: (comment: string) => invoke<GeneratedKey>("generate_key", { comment }),
};

export const knownHosts = {
  list: () => invoke<KnownHost[]>("known_hosts_list"),
  remove: (line: number) => invoke<boolean>("known_hosts_remove", { line }),
  forget: (host: string, port: number) => invoke<number>("known_hosts_forget", { host, port }),
};

export const sshConfig = {
  preview: (path: string | null) => invoke<SshConfigPreview>("ssh_config_preview", { path }),
  import: (hosts: ImportedHost[], group: string) =>
    invoke<ImportSummary>("ssh_config_import", { hosts, group }),
};

export const snippets = {
  list: () => invoke<VaultRecord<Snippet>[]>("list_snippets"),
  save: (id: Uuid | null, snippet: Snippet) =>
    invoke<VaultRecord<Snippet>>("save_snippet", { id, snippet }),
  delete: (id: Uuid) => invoke<void>("delete_snippet", { id }),
};

export const forwards = {
  list: () => invoke<VaultRecord<ForwardRule>[]>("list_forwards"),
  save: (id: Uuid | null, rule: ForwardRule) =>
    invoke<VaultRecord<ForwardRule>>("save_forward", { id, rule }),
  delete: (id: Uuid) => invoke<void>("delete_forward", { id }),
  start: (id: Uuid) => invoke<void>("forward_start", { id }),
  stop: (id: Uuid) => invoke<void>("forward_stop", { id }),
  statuses: () => invoke<Record<Uuid, ForwardStatus>>("forward_statuses"),
  onStatus: (handler: (e: { rule_id: Uuid; status: ForwardStatus }) => void): Promise<UnlistenFn> =>
    listen<{ rule_id: Uuid; status: ForwardStatus }>("forward:status", (e) => handler(e.payload)),
};

export const runs = {
  start(runId: string, jobs: { host_id: Uuid; command: string }[], timeoutSecs: number, onEvent: (e: RunEvent) => void) {
    const channel = new Channel<RunEvent>(onEvent);
    return invoke<void>("run_on_hosts", { runId, jobs, timeoutSecs, onEvent: channel });
  },
  cancel: (runId: string) => invoke<boolean>("run_cancel", { runId }),
};

export const sessionLogs = {
  start: (paneId: string, path: string, plain: boolean, header: string) =>
    invoke<void>("ssh_log_start", { paneId, path, plain, header }),
  stop: (paneId: string) => invoke<void>("ssh_log_stop", { paneId }),
};

export const remoteEdit = {
  start: (sessionId: string, remotePath: string) =>
    invoke<{ edit_id: string; local_path: string }>("sftp_edit_start", { sessionId, remotePath }),
  stop: (editId: string) => invoke<void>("sftp_edit_stop", { editId }),
  onEvent: (handler: (e: EditEvent) => void): Promise<UnlistenFn> =>
    listen<EditEvent>("sftp:edit", (e) => handler(e.payload)),
};

export const localTerm = {
  spawn(paneId: string, cols: number, rows: number, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) =>
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)),
    );
    return invoke<void>("local_spawn", { paneId, cols, rows, onData: channel });
  },
  write: (paneId: string, data: Uint8Array) => invoke<void>("local_write", { paneId, data: Array.from(data) }),
  resize: (paneId: string, cols: number, rows: number) => invoke<void>("local_resize", { paneId, cols, rows }),
  close: (paneId: string) => invoke<void>("local_close", { paneId }),
};

export const health = {
  check: (hostIds: Uuid[] | null) => invoke<HealthResult[]>("check_hosts", { hostIds }),
};

export const ansible = {
  preview: (path: string) => invoke<SshConfigPreview>("ansible_preview", { path }),
};

export const exportSshConfig = (path: string | null) => invoke<string>("export_ssh_config", { path });

/** Native folder picker. Resolves to null when the user cancels. */
export async function pickFolder(title: string): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}
