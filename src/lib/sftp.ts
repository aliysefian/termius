// IPC wrappers for the SFTP browser (src-tauri/src/sftp.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Credentials } from "./ssh";
import type { FileEntry, TransferProgress, Uuid } from "./types";

export type Direction = "upload" | "download";

/** The start of a file, for a quick look. */
export interface Preview {
  kind: "text" | "binary";
  text: string | null;
  base64: string | null;
  truncated: boolean;
}

/** What a place can do; the pane offers only these. Mirrors `Caps` in files/mod.rs. */
export interface Caps {
  mkdir: boolean;
  rename: boolean;
  delete: boolean;
  chmod: boolean;
  resume: boolean;
  preview: boolean;
  edit: boolean;
}

/** The permissive default, until a place has said what it supports. */
export const ALL_CAPS: Caps = { mkdir: true, rename: true, delete: true, chmod: false, resume: true, preview: true, edit: true };

/** Operations common to every pane so the UI can treat them uniformly. */
export interface FileSource {
  /** The id of this place in the backend: `local`, or the pane's session id. */
  id: string;
  list(path: string): Promise<FileEntry[]>;
  mkdir(dir: string, name: string): Promise<void>;
  rename(path: string, newName: string): Promise<void>;
  remove(paths: string[]): Promise<void>;
  preview(path: string): Promise<Preview>;
  /** Change permission bits. Only where `caps.chmod`. */
  chmod?(path: string, mode: number): Promise<void>;
  caps(): Promise<Caps>;
  /** Path separator for display and parent navigation. */
  sep: string;
}

/** Any open place by its id ("local" is this computer's disk). */
export function place(id: string, sep: string): FileSource {
  return {
    id,
    sep,
    list: (path) => invoke<FileEntry[]>("files_list", { sessionId: id, path }),
    mkdir: (dir, name) => invoke<void>("files_mkdir", { sessionId: id, dir, name }),
    rename: (path, newName) => invoke<void>("files_rename", { sessionId: id, path, newName }),
    remove: (paths) => invoke<void>("files_remove", { sessionId: id, paths }),
    preview: (path) => invoke<Preview>("files_preview", { sessionId: id, path }),
    chmod: (path, mode) => invoke<void>("files_chmod", { sessionId: id, path, mode }),
    caps: () => invoke<Caps>("files_caps", { sessionId: id }),
  };
}

export const LOCAL_ID = "local";

export const local: FileSource & { home(): Promise<string> } = {
  ...place(LOCAL_ID, navigator.userAgent.includes("Windows") ? "\\" : "/"),
  home: () => invoke<string>("local_home"),
};

export function remote(sessionId: string): FileSource {
  return place(sessionId, "/");
}

/** What to do when something is already at the destination. */
export type Conflict = "overwrite" | "skip" | "rename";

export const sftp = {
  open: (sessionId: string, hostId: Uuid, credentials: Credentials | null) =>
    invoke<{ home: string; new_host_keys: { host: string; fingerprint: string }[] }>("sftp_open", {
      sessionId,
      hostId,
      credentials,
    }),
  close: (sessionId: string) => invoke<void>("files_close", { sessionId }),
  /** Copy between any two open places; `removeSource` makes it a move. */
  transfer(
    sourceId: string,
    destId: string,
    transferId: string,
    sources: string[],
    destDir: string,
    onProgress: (p: TransferProgress) => void,
    options: { resume?: boolean; conflict?: Conflict; removeSource?: boolean } = {},
  ) {
    const channel = new Channel<TransferProgress>(onProgress);
    return invoke<void>("files_transfer_start", {
      sourceId,
      destId,
      transferId,
      sources,
      destDir,
      resume: options.resume ?? false,
      conflict: options.conflict ?? "overwrite",
      removeSource: options.removeSource ?? false,
      onProgress: channel,
    });
  },
  cancel: (transferId: string) => invoke<void>("transfer_cancel", { transferId }),
  pause: (transferId: string) => invoke<void>("transfer_pause", { transferId }),
  resume: (transferId: string) => invoke<void>("transfer_resume", { transferId }),
};

/** Parent directory for either path style. Returns the input at the root. */
export function parentPath(path: string, sep: string): string {
  if (sep === "/") {
    const trimmed = path.replace(/\/+$/, "");
    const i = trimmed.lastIndexOf("/");
    return i <= 0 ? "/" : trimmed.slice(0, i);
  }
  const trimmed = path.replace(/[\\/]+$/, "");
  const i = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (i < 0) return path;
  const parent = trimmed.slice(0, i);
  // "C:" alone is not navigable; keep the trailing backslash.
  return /^[A-Za-z]:$/.test(parent) ? parent + "\\" : parent || path;
}
