import type { DbTableInfo, DbTableMeta } from "./types";

// Wording for sizes and counts the explorer, inspector and table view share.
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${units[i]}`;
}

/** An estimated row count, always marked as an estimate. */
export function estimateText(n: number | null | undefined): string {
  return n === null || n === undefined ? "unknown" : `≈ ${n.toLocaleString()} rows (estimate)`;
}

/** "12 s", "3 min", "2 h 5 min": how long ago, for a connection's age. */
export function ageText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${m % 60} min`;
}

// -- what the inspector lists --------------------------------------------------------------------------------------

export interface Fact {
  label: string;
  value: string;
  mono?: boolean;
  /** Draw the value in the warning colour, for a thing the person should notice (an unencrypted connection). */
  warn?: boolean;
}

export interface ConnectionFactsInput {
  engineLabel: string;
  host: string;
  port: number;
  user: string;
  database: string;
  tls: "disable" | "require" | "verify_full";
  sshHost: string | null;
  environment: string | null;
  readOnly: boolean;
  /** Set when connected. */
  version: string | null;
  connectedAt: number | null;
  now: number;
}

/** What is known about a connection, without a password and without anything the server wasn't asked for. */
export function connectionFacts(i: ConnectionFactsInput): Fact[] {
  const out: Fact[] = [
    { label: "Status", value: i.version === null ? "Not connected" : `Connected${i.connectedAt ? ` for ${ageText(i.now - i.connectedAt)}` : ""}` },
    { label: "Engine", value: i.engineLabel },
  ];
  if (i.version) out.push({ label: "Server version", value: i.version, mono: true });
  out.push({ label: "Address", value: `${i.host}:${i.port}`, mono: true });
  if (i.user) out.push({ label: "User", value: i.user, mono: true });
  if (i.database) out.push({ label: "Database", value: i.database, mono: true });
  if (i.environment) out.push({ label: "Environment", value: i.environment });
  out.push(
    i.tls === "disable" && !i.sshHost
      ? { label: "Encryption", value: "None: the password and rows cross the network in clear text", warn: true }
      : { label: "Encryption", value: i.tls === "verify_full" ? "TLS, certificate checked" : i.tls === "require" ? "TLS, certificate not checked" : "None, inside the SSH tunnel" },
  );
  if (i.sshHost) out.push({ label: "SSH tunnel", value: `through ${i.sshHost}` });
  out.push({ label: "Sessions", value: i.readOnly ? "Read-only (the server refuses changes)" : "Can change data" });
  return out;
}

/** What is known about a table: only what the server reported. */
export function tableFacts(schema: string, table: string, meta: DbTableMeta | null, info: DbTableInfo | null): Fact[] {
  const out: Fact[] = [{ label: "Name", value: table, mono: true }, { label: "Schema", value: schema, mono: true }];
  if (meta?.kind) out.push({ label: "Kind", value: meta.kind });
  if (info) out.push({ label: "Columns", value: String(info.columns.length) });
  if (meta?.supported) out.push({ label: "Indexes", value: String(meta.indexes.length) });
  if (info) out.push({ label: "Primary key", value: info.primary_key.length ? info.primary_key.join(", ") : "none", mono: info.primary_key.length > 0 });
  if (meta && meta.approx_rows !== null) out.push({ label: "Rows", value: estimateText(meta.approx_rows) });
  if (meta && meta.size_bytes !== null) out.push({ label: "Size", value: formatBytes(meta.size_bytes) });
  if (meta?.comment) out.push({ label: "Comment", value: meta.comment });
  return out;
}
