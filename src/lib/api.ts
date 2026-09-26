// Typed wrappers over Tauri IPC. Keep this the only place that knows command names.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  BackupInfo,
  Collection,
  ConflictInfo,
  CreateResult,
  HostGroup,
  HostKeyPrompt,
  IntegrityReport,
  KeyAlgorithm,
  KeyImport,
  KeyUsage,
  KnownHostsImport,
  Proxy,
  RestoreReport,
  SshKey,
  UnlockResult,
  VaultInfo,
  VaultKnownHost,
  VaultSettings,
  Workspace,
  HealthResult,
  Revealed,
  EditEvent,
  RunEvent,
  HostCredentials,
  GeneratedKey,
  ImportedHost,
  ImportSummary,
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
  create: (password: string, withRecovery: boolean, remember: boolean) =>
    invoke<CreateResult>("create_vault", { password, withRecovery, remember }),
  unlock: (password: string, remember: boolean) => invoke<UnlockResult>("unlock_vault", { password, remember }),
  /** Unlock with the key this device stored in the OS keychain. */
  unlockWithDevice: () => invoke<UnlockResult>("unlock_with_device"),
  /** "Forgot password": recovery key + a new master password. */
  unlockWithRecovery: (recoveryKey: string, newPassword: string, remember: boolean) =>
    invoke<UnlockResult>("unlock_with_recovery", { recoveryKey, newPassword, remember }),
  forgetDevice: () => invoke<VaultStatus>("forget_device"),
  rememberDevice: () => invoke<VaultStatus>("remember_device"),
  lock: () => invoke<VaultStatus>("lock_vault"),
  changePassword: (currentPassword: string, newPassword: string) =>
    invoke<void>("change_master_password", { currentPassword, newPassword }),
  /** Create or replace the recovery key; returns it (shown once). */
  setRecoveryKey: (masterPassword: string) => invoke<string>("set_recovery_key", { masterPassword }),
  removeRecoveryKey: (masterPassword: string) => invoke<void>("remove_recovery_key", { masterPassword }),
  info: () => invoke<VaultInfo>("vault_info"),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  createBackup: () => invoke<BackupInfo>("create_backup"),
  restoreBackup: (fileName: string) => invoke<RestoreReport>("restore_backup", { fileName }),
  verifyIntegrity: () => invoke<IntegrityReport>("verify_integrity"),
  listConflicts: () => invoke<ConflictInfo[]>("list_conflicts"),
  resolveConflict: (collection: Collection, id: Uuid, fileName: string, keep: "current" | "other") =>
    invoke<void>("resolve_conflict", { collection, id, fileName, keep }),
  move: (destination: string) => invoke<VaultStatus>("move_vault", { destination }),
  getSettings: () => invoke<{ rev: number; settings: VaultSettings }>("get_vault_settings"),
  saveSettings: (baseRev: number, settings: VaultSettings) =>
    invoke<{ rev: number; settings: VaultSettings }>("save_vault_settings", { baseRev, settings }),
  onChanged: (handler: (c: RecordChange) => void): Promise<UnlistenFn> =>
    listen<RecordChange>("vault:changed", (e) => handler(e.payload)),
};

// Saves take the revision being edited (null for new records), so a change
// made meanwhile on another device is merged or reported, never overwritten.

/** Put back something deleted in the last few minutes, secrets included. */
export const undelete = (collection: Collection, id: Uuid) => invoke<void>("undelete_record", { collection, id });

export const hosts = {
  list: () => invoke<VaultRecord<Host>[]>("list_hosts"),
  save: (id: Uuid | null, baseRev: number | null, host: Host) =>
    invoke<VaultRecord<Host>>("save_host", { id, baseRev, host }),
  saveWithCredentials: (id: Uuid | null, baseRev: number | null, host: Host, credentials: HostCredentials) =>
    invoke<{ host: VaultRecord<Host>; public_key: string | null }>("save_host_with_credentials", {
      id,
      baseRev,
      host,
      credentials,
    }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_host", { id, baseRev }),
};

export const identities = {
  list: () => invoke<VaultRecord<Identity>[]>("list_identities"),
  save: (id: Uuid | null, baseRev: number | null, identity: Identity) =>
    invoke<VaultRecord<Identity>>("save_identity", { id, baseRev, identity }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_identity", { id, baseRev }),
  /** Stored secrets; needs the master password unless recently entered. */
  reveal: (id: Uuid, masterPassword: string | null) =>
    invoke<{ secret: Revealed; grace_secs: number }>("reveal_identity", { id, masterPassword }),
  revealClose: () => invoke<void>("reveal_close"),
  publicKey: (id: Uuid) => invoke<PublicKeyInfo>("identity_public_key", { id }),
};

export const keys = {
  generate: (comment: string) => invoke<GeneratedKey>("generate_key", { comment }),
  list: () => invoke<VaultRecord<SshKey>[]>("list_keys"),
  create: (name: string, algorithm: KeyAlgorithm, comment: string, passphrase: string | null, savePassphrase: boolean) =>
    invoke<VaultRecord<SshKey>>("generate_ssh_key", { name, algorithm, comment, passphrase, savePassphrase }),
  importPrivate: (name: string, privateKey: string, passphrase: string | null, savePassphrase: boolean) =>
    invoke<VaultRecord<SshKey>>("import_private_key", { name, privateKey, passphrase, savePassphrase }),
  importPrivateFile: (name: string, path: string, passphrase: string | null, savePassphrase: boolean) =>
    invoke<VaultRecord<SshKey>>("import_private_key_file", { name, path, passphrase, savePassphrase }),
  importPublic: (name: string, publicKey: string) => invoke<VaultRecord<SshKey>>("import_public_key", { name, publicKey }),
  update: (id: Uuid, baseRev: number | null, name: string, certificate: string | null) =>
    invoke<VaultRecord<SshKey>>("update_key", { id, baseRev, name, certificate }),
  changePassphrase: (id: Uuid, baseRev: number | null, current: string | null, next: string | null, savePassphrase: boolean) =>
    invoke<VaultRecord<SshKey>>("change_key_passphrase", { id, baseRev, current, new: next, savePassphrase }),
  usage: (id: Uuid) => invoke<KeyUsage>("key_usage", { id }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_key", { id, baseRev }),
  /** The only way a private key leaves the vault: master password every time, new owner-only file. */
  exportPrivate: (id: Uuid, masterPassword: string, destination: string) =>
    invoke<void>("export_private_key", { id, masterPassword, destination }),
};

export const knownHosts = {
  list: () => invoke<VaultKnownHost[]>("known_hosts_list"),
  forget: (host: string, port: number) => invoke<boolean>("known_hosts_forget", { host, port }),
  /** Import from an OpenSSH known_hosts file (default ~/.ssh/known_hosts). */
  import: (path: string | null) => invoke<KnownHostsImport>("known_hosts_import", { path }),
  answer: (requestId: Uuid, trust: boolean) => invoke<void>("answer_host_key", { requestId, trust }),
  onPrompt: (handler: (p: HostKeyPrompt) => void): Promise<UnlistenFn> =>
    listen<HostKeyPrompt>("hostkey:prompt", (e) => handler(e.payload)),
};

export const groups = {
  list: () => invoke<VaultRecord<HostGroup>[]>("list_groups"),
  save: (id: Uuid | null, baseRev: number | null, group: HostGroup) =>
    invoke<VaultRecord<HostGroup>>("save_group", { id, baseRev, group }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_group", { id, baseRev }),
};

export const proxies = {
  list: () => invoke<VaultRecord<Proxy>[]>("list_proxies"),
  save: (id: Uuid | null, baseRev: number | null, proxy: Proxy) =>
    invoke<VaultRecord<Proxy>>("save_proxy", { id, baseRev, proxy }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_proxy", { id, baseRev }),
};

export const workspaces = {
  list: () => invoke<VaultRecord<Workspace>[]>("list_workspaces"),
  save: (id: Uuid | null, baseRev: number | null, workspace: Workspace) =>
    invoke<VaultRecord<Workspace>>("save_workspace", { id, baseRev, workspace }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_workspace", { id, baseRev }),
};

export const sshConfig = {
  preview: (path: string | null) => invoke<SshConfigPreview>("ssh_config_preview", { path }),
  import: (hosts: ImportedHost[], group: string, keyImport: KeyImport) =>
    invoke<ImportSummary>("ssh_config_import", { hosts, group, keyImport }),
};

export const snippets = {
  list: () => invoke<VaultRecord<Snippet>[]>("list_snippets"),
  save: (id: Uuid | null, baseRev: number | null, snippet: Snippet) =>
    invoke<VaultRecord<Snippet>>("save_snippet", { id, baseRev, snippet }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_snippet", { id, baseRev }),
};

export const forwards = {
  list: () => invoke<VaultRecord<ForwardRule>[]>("list_forwards"),
  save: (id: Uuid | null, baseRev: number | null, rule: ForwardRule) =>
    invoke<VaultRecord<ForwardRule>>("save_forward", { id, baseRev, rule }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_forward", { id, baseRev }),
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
  spawn(paneId: string, cols: number, rows: number, onData: (bytes: Uint8Array) => void, shell: string | null = null, cwd: string | null = null) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) =>
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)),
    );
    return invoke<void>("local_spawn", { paneId, cols, rows, shell, cwd, onData: channel });
  },
  write: (paneId: string, data: Uint8Array) => invoke<void>("local_write", { paneId, data: Array.from(data) }),
  resize: (paneId: string, cols: number, rows: number) => invoke<void>("local_resize", { paneId, cols, rows }),
  close: (paneId: string) => invoke<void>("local_close", { paneId }),
};

export const health = {
  check: (hostIds: Uuid[] | null) => invoke<HealthResult[]>("check_hosts", { hostIds }),
};

export const putty = {
  sessions: () => invoke<SshConfigPreview>("putty_sessions"),
};

/** A picked CSV file as a header row and data rows. */
export const csv = {
  preview: (path: string) => invoke<{ headers: string[]; rows: string[][] }>("csv_preview", { path }),
};

export const readTextFile = (path: string) => invoke<string>("read_text_file", { path });

export const ansible = {
  preview: (path: string) => invoke<SshConfigPreview>("ansible_preview", { path }),
};

export const exportSshConfig = (path: string | null) => invoke<string>("export_ssh_config", { path });

/** Native folder picker. Resolves to null when the user cancels. */
export async function pickFolder(title: string): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}
