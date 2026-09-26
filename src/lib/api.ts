// Typed wrappers over Tauri IPC. Keep this the only place that knows command names.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
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
  delete: (id: Uuid) => invoke<void>("delete_host", { id }),
};

export const identities = {
  list: () => invoke<VaultRecord<Identity>[]>("list_identities"),
  save: (id: Uuid | null, identity: Identity) =>
    invoke<VaultRecord<Identity>>("save_identity", { id, identity }),
  delete: (id: Uuid) => invoke<void>("delete_identity", { id }),
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

/** Native folder picker. Resolves to null when the user cancels. */
export async function pickFolder(title: string): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}
