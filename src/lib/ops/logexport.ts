// Turning rows into files: plain text, JSON Lines and CSV. Everything is written exactly as the viewer holds it (the
// backend has already hidden secrets unless that was switched off), so what is saved matches what was seen.
import { PRIORITY_NAMES, type Row } from "./logrows";

export type ExportFormat = "text" | "jsonl" | "csv";
export const FORMATS: { id: ExportFormat; label: string; ext: string }[] = [
  { id: "text", label: "Plain text (.log)", ext: "log" },
  { id: "jsonl", label: "JSON Lines (.jsonl)", ext: "jsonl" },
  { id: "csv", label: "CSV (.csv)", ext: "csv" },
];

/** `2026-11-03T23:04:12.583Z` in UTC, so a file means the same wherever it is read. */
export const isoTime = (ms: number) => new Date(ms).toISOString();

export const priorityName = (p: number | null) => (p === null ? "" : (PRIORITY_NAMES[p] ?? "").toLowerCase());

export function toText(rows: Row[]): string {
  return rows.map((r) => `${isoTime(r.ts)} ${r.host} ${priorityName(r.priority) || r.sev} ${r.unit || r.ident || "-"}${r.pid ? `[${r.pid}]` : ""}: ${r.message}`).join("\n") + (rows.length ? "\n" : "");
}

export function toJsonl(rows: Row[]): string {
  return (
    rows
      .map((r) =>
        JSON.stringify({
          time: isoTime(r.ts),
          host: r.host,
          host_id: r.hostId,
          severity: priorityName(r.priority) || r.sev,
          priority: r.priority,
          unit: r.unit || null,
          program: r.ident || null,
          pid: r.pid,
          message: r.message,
          cursor: r.cursor,
          journal_host: r.journalHost,
          fields: r.fields.length ? Object.fromEntries(r.fields) : undefined,
        }),
      )
      .join("\n") + (rows.length ? "\n" : "")
  );
}

/** A CSV cell: quoted when needed, and a leading `=`, `+`, `-`, `@` is defused so a spreadsheet doesn't run it as a formula. */
export function csvCell(v: string | number | null): string {
  let s = v === null ? "" : String(v);
  if (/^[=+\-@\t\r]/.test(s)) s = `'${s}`;
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

export function toCsv(rows: Row[]): string {
  const head = ["time", "host", "severity", "unit", "program", "pid", "message"].join(",");
  const body = rows.map((r) => [isoTime(r.ts), r.host, priorityName(r.priority) || r.sev, r.unit, r.ident, r.pid, r.message].map(csvCell).join(","));
  return [head, ...body].join("\r\n") + "\r\n";
}

export function render(rows: Row[], format: ExportFormat): string {
  return format === "jsonl" ? toJsonl(rows) : format === "csv" ? toCsv(rows) : toText(rows);
}

/** One row as JSON, for "copy event". */
export function rowJson(r: Row): string {
  return toJsonl([r]).trimEnd();
}

/** A file name for an export, from the time. */
export function exportName(format: ExportFormat, at = new Date()): string {
  const stamp = at.toISOString().replace(/[:.]/g, "-").slice(0, 19);
  return `sshvault-logs-${stamp}.${FORMATS.find((f) => f.id === format)!.ext}`;
}
