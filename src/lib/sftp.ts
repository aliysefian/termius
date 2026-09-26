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

/** Operations common to both panes so the UI can treat them uniformly. */
export interface FileSource {
  list(path: string): Promise<FileEntry[]>;
  mkdir(dir: string, name: string): Promise<void>;
  rename(path: string, newName: string): Promise<void>;
  remove(paths: string[]): Promise<void>;
  preview(path: string): Promise<Preview>;
  /** Change permission bits. Remote only. */
  chmod?(path: string, mode: number): Promise<void>;
  /** Path separator for display and parent navigation. */
  sep: string;
}

export const local: FileSource & { home(): Promise<string> } = {
  sep: navigator.userAgent.includes("Windows") ? "\\" : "/",
  home: () => invoke<string>("local_home"),
  list: (path) => invoke<FileEntry[]>("local_list", { path }),
  mkdir: (dir, name) => invoke<void>("local_mkdir", { dir, name }),
  rename: (path, newName) => invoke<void>("local_rename", { path, newName }),
  remove: (paths) => invoke<void>("local_remove", { paths }),
  preview: (path) => invoke<Preview>("local_preview", { path }),
};

export function remote(sessionId: string): FileSource {
  return {
    sep: "/",
    list: (path) => invoke<FileEntry[]>("sftp_list", { sessionId, path }),
    mkdir: (dir, name) => invoke<void>("sftp_mkdir", { sessionId, dir, name }),
    rename: (path, newName) => invoke<void>("sftp_rename", { sessionId, path, newName }),
    remove: (paths) => invoke<void>("sftp_remove", { sessionId, paths }),
    preview: (path) => invoke<Preview>("sftp_preview", { sessionId, path }),
    chmod: (path, mode) => invoke<void>("sftp_chmod", { sessionId, path, mode }),
  };
}

export const sftp = {
  open: (sessionId: string, hostId: Uuid, credentials: Credentials | null) =>
    invoke<{ home: string; new_host_keys: { host: string; fingerprint: string }[] }>("sftp_open", {
      sessionId,
      hostId,
      credentials,
    }),
  close: (sessionId: string) => invoke<void>("sftp_close", { sessionId }),
  transfer(
    sessionId: string,
    transferId: string,
    direction: Direction,
    sources: string[],
    destDir: string,
    onProgress: (p: TransferProgress) => void,
  ) {
    const channel = new Channel<TransferProgress>(onProgress);
    return invoke<void>("transfer_start", {
      sessionId,
      transferId,
      direction,
      sources,
      destDir,
      onProgress: channel,
    });
  },
  cancel: (transferId: string) => invoke<void>("transfer_cancel", { transferId }),
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
