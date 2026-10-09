// Typed wrappers over Tauri IPC. Keep this the only place that knows command names.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AgentPrompt,
  AgentStatus,
  AgentUse,
  BackupInfo,
  CliPrompt,
  CliStatus,
  ComposeVerb,
  ContainerAction,
  ContainerListing,
  ContainerLogEvent,
  ContainerLogOptions,
  ContainerResources,
  ContainerStat,
  CrashLogView,
  ContainerRuntime,
  DbConnection,
  DbQueryResult,
  DbRowEdit,
  DbTableInfo,
  DbTreeNode,
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
  ProxySpec,
  RestoreReport,
  SerialConfig,
  SshKey,
  UnlockResult,
  VaultInfo,
  VaultKnownHost,
  VaultSettings,
  SavedRunbook,
  Workspace,
  Health,
  HealthResult,
  Revealed,
  EditEvent,
  RunEvent,
  HostCredentials,
  GeneratedKey,
  ImportedHost,
  ImportSummary,
  PruneItem,
  PruneKind,
  PruneResult,
  ResourceKind,
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
  acceptRollbacks: () => invoke<void>("accept_rollbacks"),
  activity: (minutes: number) => invoke<void>("vault_activity", { minutes }),
  onIdleLocked: (handler: () => void): Promise<UnlistenFn> => listen("vault:idle-locked", () => handler()),
  setKeepKeyOnLock: (keep: boolean) => invoke<VaultStatus>("set_keep_key_on_lock", { keep }),
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

export const agent = {
  status: () => invoke<AgentStatus>("agent_status"),
  setEnabled: (enabled: boolean) => invoke<AgentStatus>("agent_set_enabled", { enabled }),
  answer: (requestId: Uuid, allow: boolean) => invoke<void>("answer_agent_request", { requestId, allow }),
  setKeyMode: (id: Uuid, baseRev: number | null, mode: AgentUse) =>
    invoke<VaultRecord<SshKey>>("set_key_agent", { id, baseRev, mode }),
  onPrompt: (handler: (p: AgentPrompt) => void): Promise<UnlistenFn> =>
    listen<AgentPrompt>("agent:prompt", (e) => handler(e.payload)),
};

/** Command-line control (`sshvault list/connect/run`). */
export const cli = {
  status: () => invoke<CliStatus>("cli_status"),
  setEnabled: (enabled: boolean) => invoke<CliStatus>("cli_set_enabled", { enabled }),
  answer: (requestId: Uuid, allow: boolean, trustMinutes: number | null) =>
    invoke<void>("answer_cli_request", { requestId, allow, trustMinutes }),
  onPrompt: (handler: (p: CliPrompt) => void): Promise<UnlistenFn> =>
    listen<CliPrompt>("cli:prompt", (e) => handler(e.payload)),
  onOpen: (handler: (p: { host_id: Uuid; label: string }) => void): Promise<UnlistenFn> =>
    listen<{ host_id: Uuid; label: string }>("cli:open", (e) => handler(e.payload)),
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
  /** Reach `target` (default github.com:22) through an unsaved or saved proxy. */
  test: (id: Uuid | null, spec: ProxySpec, target: string | null) => invoke<Health>("test_proxy", { id, spec, target }),
};

export const workspaces = {
  list: () => invoke<VaultRecord<Workspace>[]>("list_workspaces"),
  save: (id: Uuid | null, baseRev: number | null, workspace: Workspace) =>
    invoke<VaultRecord<Workspace>>("save_workspace", { id, baseRev, workspace }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_workspace", { id, baseRev }),
};

export const runbooks = {
  list: () => invoke<VaultRecord<SavedRunbook>[]>("list_runbooks"),
  save: (id: Uuid | null, baseRev: number | null, runbook: SavedRunbook) =>
    invoke<VaultRecord<SavedRunbook>>("save_runbook", { id, baseRev, runbook }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_runbook", { id, baseRev }),
};

/** The editor's live check of a runbook's text. */
export const runbookCheck = (body: string) => invoke<import("./runbook").RunbookCheck>("runbook_check", { body });
/** The steps filled in for one host, without running anything. */
export const runbookPlan = (body: string, params: Record<string, string>, host: string, label: string) =>
  invoke<import("./runbook").PlannedStep[]>("runbook_plan", { body, params, host, label });
/** Run on hosts in the background; events arrive on the channel. `files` maps a file parameter to the chosen path. */
export function runbookStart(
  runId: string,
  body: string,
  params: Record<string, string>,
  files: Record<string, string>,
  hostIds: Uuid[],
  scheduled: boolean,
  onEvent: (e: import("./runbook").RunbookEvent) => void,
  /** On a host where the run fails, also run the runbook's rollback steps. */
  rollback = false,
) {
  const channel = new Channel<import("./runbook").RunbookEvent>(onEvent);
  return invoke<void>("runbook_start", { runId, body, params, files, hostIds, scheduled, onEvent: channel, rollback });
}
export const runbookCancel = (runId: string) => invoke<boolean>("runbook_cancel", { runId });
export const runbookHistory = {
  list: () => invoke<import("./runbook").RunSummary[]>("runbook_history_list"),
  get: (id: string) => invoke<import("./runbook").RunRecord | null>("runbook_history_get", { id }),
  delete: (id: string) => invoke<boolean>("runbook_history_delete", { id }),
  clear: () => invoke<number>("runbook_history_clear"),
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

export type RunOrder = "parallel" | { sequential: { stop_on_failure: boolean } };

/** Why SSHVault closed by itself, if it did. Works while the vault is locked. */
export const crash = {
  log: () => invoke<CrashLogView>("crash_log"),
  clear: () => invoke<void>("clear_crash_log"),
};

export const runs = {
  /** `order`: all hosts at once (the default), or one after another, optionally stopping at the first failure. */
  start(runId: string, jobs: { host_id: Uuid; command: string }[], timeoutSecs: number, onEvent: (e: RunEvent) => void, order: RunOrder = "parallel") {
    const channel = new Channel<RunEvent>(onEvent);
    return invoke<void>("run_on_hosts", { runId, jobs, timeoutSecs, onEvent: channel, order });
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

/** Mosh: SSH logs in and starts mosh-server; the system's mosh-client does the rest in a local terminal. */
export const mosh = {
  available: () => invoke<boolean>("mosh_available"),
  connect(paneId: string, hostId: string, cols: number, rows: number, credentials: import("./ssh").Credentials | null, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) =>
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)),
    );
    return invoke<void>("mosh_connect", { paneId, hostId, cols, rows, credentials, onData: channel });
  },
};

/** Remote desktop (RDP). Pictures arrive as byte messages; see `decodeMessage` in rdp.ts. */
export const rdp = {
  connect(
    paneId: string,
    hostId: string,
    width: number,
    height: number,
    credentials: import("./ssh").Credentials | null,
    accept: string | null,
    onMessage: (bytes: Uint8Array) => void,
  ) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) => onMessage(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)));
    return invoke<{ fingerprint: string; pinned_now: boolean }>("rdp_connect", { paneId, hostId, width, height, credentials, accept, onEvent: channel });
  },
  input: (paneId: string, input: RdpInput) => invoke<void>("rdp_input", { paneId, input }),
  close: (paneId: string) => invoke<void>("rdp_close", { paneId }),
};

/** VNC: the same pictures and input as RDP, through an SSH connection to the machine by default. */
export const vnc = {
  connect(paneId: string, hostId: string, password: string | null, credentials: import("./ssh").Credentials | null, onMessage: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) => onMessage(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)));
    return invoke<void>("vnc_connect", { paneId, hostId, password, credentials, onEvent: channel });
  },
  input: (paneId: string, input: RdpInput) => invoke<void>("vnc_input", { paneId, input }),
  close: (paneId: string) => invoke<void>("vnc_close", { paneId }),
};

/** What a cloud or tool's own program lists, as the JSON it prints (see src-tauri/src/inventory.rs for the programs). */
export type InventoryProgram = "tailscale" | "aws" | "gcp" | "azure" | "digital_ocean" | "hetzner" | "kubernetes" | "terraform";
export const inventoryRun = (source: InventoryProgram, option: string | null) => invoke<string>("inventory_run", { source, option });
/** SSH servers on a private network: the addresses that answer with an SSH banner. */
export const inventoryScan = (range: string, port: number) => invoke<{ ip: string; banner: string }[]>("inventory_scan", { range, port });

/** Run a hook command on this computer (already approved in the window). */
export const runHook = (command: string, timeoutSecs: number) => invoke<{ exit_code: number | null; output: string; timed_out: boolean }>("run_hook", { command, timeoutSecs });

/** Send a Wake-on-LAN packet to a machine on this computer's network. */
export const wakeOnLan = (mac: string, broadcast: string) => invoke<void>("wake_on_lan", { mac, broadcast });

export type RdpInput =
  | { type: "key"; code: string; down: boolean }
  | { type: "mouse_move"; x: number; y: number }
  | { type: "button"; button: number; down: boolean }
  | { type: "wheel"; vertical: boolean; units: number }
  | { type: "ctrl_alt_del" }
  | { type: "release_all" }
  | { type: "clipboard_text"; text: string };

/** Smart completion's questions to a host, over an extra channel of the pane's SSH connection. */
export const completion = {
  lookup: (paneId: string, request: import("./completion/remote").Request) =>
    invoke<import("./completion/remote").Reply>("completion_lookup", { paneId, request }),
  /** Folder listings for a tab running a shell on this computer. */
  lookupLocal: (request: import("./completion/remote").Request) =>
    invoke<import("./completion/remote").Reply>("completion_lookup_local", { request }),
};

export const localTerm = {
  spawn(paneId: string, cols: number, rows: number, onData: (bytes: Uint8Array) => void, shell: string | null = null, cwd: string | null = null, shellId: string | null = null) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) =>
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)),
    );
    return invoke<void>("local_spawn", { paneId, cols, rows, shell, shellId, cwd, onData: channel });
  },
  shells: () => invoke<import("./types").LocalShell[]>("local_shells"),
  write: (paneId: string, data: Uint8Array) => invoke<void>("local_write", { paneId, data: Array.from(data) }),
  resize: (paneId: string, cols: number, rows: number) => invoke<void>("local_resize", { paneId, cols, rows }),
  close: (paneId: string) => invoke<void>("local_close", { paneId }),
};

/** Telnet and serial consoles. */
export const raw = {
  telnet(paneId: string, host: string, port: number, cols: number, rows: number, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) =>
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)),
    );
    return invoke<void>("raw_telnet", { paneId, host, port, cols, rows, onData: channel });
  },
  serial(paneId: string, config: SerialConfig, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>((msg) =>
      onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg)),
    );
    return invoke<void>("raw_serial", { paneId, config, onData: channel });
  },
  write: (paneId: string, data: Uint8Array) => invoke<void>("raw_write", { paneId, data: Array.from(data) }),
  resize: (paneId: string, cols: number, rows: number) => invoke<void>("raw_resize", { paneId, cols, rows }),
  close: (paneId: string) => invoke<void>("raw_close", { paneId }),
  ports: () => invoke<string[]>("serial_ports"),
};

export const health = {
  check: (hostIds: Uuid[] | null) => invoke<HealthResult[]>("check_hosts", { hostIds }),
};

export const mobaxterm = {
  preview: (path: string) => invoke<SshConfigPreview>("mobaxterm_preview", { path }),
};

export const putty = {
  sessions: () => invoke<SshConfigPreview>("putty_sessions"),
};

/** A picked CSV file as a header row and data rows. */
export const csv = {
  preview: (path: string) => invoke<{ headers: string[]; rows: string[][] }>("csv_preview", { path }),
};

export const readTextFile = (path: string) => invoke<string>("read_text_file", { path });
/** Write a .json file the person chose (a snippet pack). */
export const exportTextFile = (path: string, contents: string) => invoke<void>("export_text_file", { path, contents });

export const ansible = {
  preview: (path: string) => invoke<SshConfigPreview>("ansible_preview", { path }),
};

export const exportSshConfig = (path: string | null) => invoke<string>("export_ssh_config", { path });

/** Native folder picker. Resolves to null when the user cancels. */
export async function pickFolder(title: string): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}

export const db = {
  list: () => invoke<VaultRecord<DbConnection>[]>("list_db_connections"),
  save: (id: Uuid | null, baseRev: number | null, connection: DbConnection) =>
    invoke<VaultRecord<DbConnection>>("save_db_connection", { id, baseRev, connection }),
  delete: (id: Uuid, baseRev: number | null) => invoke<void>("delete_db_connection", { id, baseRev }),
  /** Connect with a saved connection; the password never leaves the Rust side. */
  open: (id: Uuid) => invoke<{ session_id: Uuid; server_version: string }>("db_open", { id }),
  /** Try the form's values (saved or not) and hang up. Resolves to the server version. */
  test: (id: Uuid | null, connection: DbConnection) => invoke<string>("db_test", { id, connection }),
  close: (sessionId: Uuid) => invoke<void>("db_close", { sessionId }),
  query: (sessionId: Uuid, queryId: Uuid, sql: string, limit: number | null, confirmed: boolean) =>
    invoke<DbQueryResult>("db_query", { sessionId, queryId, sql, limit, confirmed }),
  cancel: (sessionId: Uuid, queryId: Uuid) => invoke<void>("db_cancel", { sessionId, queryId }),
  children: (sessionId: Uuid, path: string[]) => invoke<DbTreeNode[]>("db_children", { sessionId, path }),
  tableInfo: (sessionId: Uuid, database: string, table: string) =>
    invoke<DbTableInfo>("db_table_info", { sessionId, database, table }),
  previewUpdate: (sessionId: Uuid, edit: DbRowEdit) => invoke<string>("db_preview_update", { sessionId, edit }),
  applyUpdate: (sessionId: Uuid, edit: DbRowEdit) => invoke<number>("db_apply_update", { sessionId, edit }),
  saveExport: (path: string, contents: string) => invoke<void>("db_save_export", { path, contents }),
};

export const containers = {
  /** Open this computer (no host) or a saved host. The host needs saved credentials. */
  open: (hostId: Uuid | null) => invoke<{ session_id: Uuid; runtimes: ContainerRuntime[] }>("containers_open", { hostId }),
  close: (sessionId: Uuid) => invoke<void>("containers_close", { sessionId }),
  list: (sessionId: Uuid, runtime: ContainerRuntime, sizes: boolean) =>
    invoke<ContainerListing>("containers_list", { sessionId, runtime, sizes }),
  act: (sessionId: Uuid, runtime: ContainerRuntime, action: ContainerAction, id: string) =>
    invoke<void>("containers_act", { sessionId, runtime, action, id }),
  /** CPU, memory, network and disk use of the running containers: one reading, taking a second or two. */
  stats: (sessionId: Uuid, runtime: ContainerRuntime) => invoke<ContainerStat[]>("containers_stats", { sessionId, runtime }),
  /** The runtime's `inspect` output as JSON text. */
  inspect: (sessionId: Uuid, runtime: ContainerRuntime, id: string) => invoke<string>("containers_inspect", { sessionId, runtime, id }),
  /** Resolves to a stream id for `logsStop`; events arrive on `onEvent` until an "end". */
  logsStart(sessionId: Uuid, runtime: ContainerRuntime, id: string, options: ContainerLogOptions, onEvent: (e: ContainerLogEvent) => void) {
    const channel = new Channel<ContainerLogEvent>(onEvent);
    return invoke<Uuid>("containers_logs_start", { sessionId, runtime, id, options, onEvent: channel });
  },
  logsStop: (streamId: Uuid) => invoke<void>("containers_logs_stop", { streamId }),
  /** Volumes and networks (Docker), each with the containers that use it. */
  resources: (sessionId: Uuid, runtime: ContainerRuntime, sizes: boolean) =>
    invoke<ContainerResources>("containers_resources", { sessionId, runtime, sizes }),
  remove: (sessionId: Uuid, runtime: ContainerRuntime, kind: ResourceKind, id: string, force: boolean) =>
    invoke<void>("containers_remove", { sessionId, runtime, kind, id, force }),
  /** What a prune would remove. Removes nothing. */
  pruneItems: (sessionId: Uuid, runtime: ContainerRuntime, kind: PruneKind) =>
    invoke<PruneItem[]>("containers_prune_preview", { sessionId, runtime, kind }),
  /** Removes exactly `ids`, each re-checked first. */
  pruneRun: (sessionId: Uuid, runtime: ContainerRuntime, kind: PruneKind, ids: string[]) =>
    invoke<PruneResult[]>("containers_prune_run", { sessionId, runtime, kind, ids }),
  /** Resolves to a stream id for `logsStop`; progress arrives on `onEvent`. */
  pull(sessionId: Uuid, runtime: ContainerRuntime, reference: string, onEvent: (e: ContainerLogEvent) => void) {
    const channel = new Channel<ContainerLogEvent>(onEvent);
    return invoke<Uuid>("containers_pull", { sessionId, runtime, reference, onEvent: channel });
  },
  compose: (sessionId: Uuid, project: string, verb: ComposeVerb) => invoke<void>("containers_compose", { sessionId, project, verb }),
};

/** Kubernetes, through the kubectl on this computer or on a saved host. */
export const kube = {
  open: (hostId: Uuid | null) => invoke<{ session_id: Uuid; info: import("./kubedata").KubeInfo }>("kube_open", { hostId }),
  close: (sessionId: Uuid) => invoke<void>("kube_close", { sessionId }),
  pods: (sessionId: Uuid, context: string, scope: import("./kubedata").Scope) => invoke<import("./kubedata").KubePod[]>("kube_pods", { sessionId, context, scope }),
  resources: (sessionId: Uuid, context: string, scope: import("./kubedata").Scope, kind: import("./kubedata").ResourceKind) => invoke<import("./kubedata").KubeResource[]>("kube_resources", { sessionId, context, scope, kind }),
  describeResource: (sessionId: Uuid, context: string, namespace: string, kind: import("./kubedata").ResourceKind, name: string) => invoke<string>("kube_describe_resource", { sessionId, context, namespace, kind, name }),
  namespaces: (sessionId: Uuid, context: string) => invoke<string[]>("kube_namespaces", { sessionId, context }),
  describe: (sessionId: Uuid, context: string, namespace: string, pod: string) => invoke<string>("kube_describe", { sessionId, context, namespace, pod }),
  deletePod: (sessionId: Uuid, context: string, namespace: string, pod: string) => invoke<void>("kube_delete_pod", { sessionId, context, namespace, pod }),
  /** Resolves to an id for `stop`; events arrive until an "end". */
  logs(sessionId: Uuid, context: string, namespace: string, pod: string, container: string | null, options: { tail: number; follow: boolean; timestamps: boolean; previous: boolean }, onEvent: (e: ContainerLogEvent) => void) {
    const channel = new Channel<ContainerLogEvent>(onEvent);
    return invoke<Uuid>("kube_logs_start", { sessionId, context, namespace, pod, container, options, onEvent: channel });
  },
  forward(sessionId: Uuid, context: string, namespace: string, to: { kind: "pod" | "service"; name: string }, localPort: number, remotePort: number, onEvent: (e: ContainerLogEvent) => void) {
    const channel = new Channel<ContainerLogEvent>(onEvent);
    return invoke<Uuid>("kube_forward_start", { sessionId, context, namespace, to, localPort, remotePort, onEvent: channel });
  },
  stop: (streamId: Uuid) => invoke<void>("kube_stop", { streamId }),
};

/** Detail monitoring: one SSH connection per watched host, scripts run under `sh -c`. */
export const monitor = {
  open: (hostId: Uuid) => invoke<Uuid>("monitor_open", { hostId }),
  exec: (sessionId: Uuid, script: string, timeoutSecs: number) =>
    invoke<{ stdout: string; stderr: string; code: number | null }>("monitor_exec", { sessionId, script, timeoutSecs }),
  close: (sessionId: Uuid) => invoke<void>("monitor_close", { sessionId }),
  /** Follow a script's output (logs). Resolves to an id for `streamStop`; events arrive until an "end". */
  streamStart(sessionId: Uuid, script: string, onEvent: (e: ContainerLogEvent) => void) {
    const channel = new Channel<ContainerLogEvent>(onEvent);
    return invoke<Uuid>("monitor_stream_start", { sessionId, script, onEvent: channel });
  },
  streamStop: (streamId: Uuid) => invoke<void>("monitor_stream_stop", { streamId }),
};
