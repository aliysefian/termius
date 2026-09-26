// Map a CSV table (from Termius, a spreadsheet or a CMDB export) to hosts.
import type { ImportedHost } from "./types";

export type Field = "label" | "hostname" | "port" | "user" | "group" | "tags" | "notes";
export type Mapping = Record<Field, number | null>;

const GUESSES: Record<Field, RegExp> = {
  label: /^(label|name|alias|title|host ?name?$|server)$/i,
  hostname: /^(hostname|host|address|ip|ip ?address|hostname\/ip|fqdn|dns)$/i,
  port: /^port$/i,
  user: /^(user|username|user ?name|login)$/i,
  group: /^(group|groups|folder|path|environment|env)$/i,
  tags: /^(tags?|labels)$/i,
  notes: /^(notes?|description|comment)$/i,
};

/** Best guess at which column holds what. `hostname` falls back to `label`. */
export function guessMapping(headers: string[]): Mapping {
  const m: Mapping = { label: null, hostname: null, port: null, user: null, group: null, tags: null, notes: null };
  const taken = new Set<number>();
  // Most specific first, so "Hostname/IP" isn't taken as a label.
  for (const f of ["hostname", "port", "user", "group", "tags", "notes", "label"] as Field[]) {
    const i = headers.findIndex((h, idx) => !taken.has(idx) && GUESSES[f].test(h.trim()));
    if (i >= 0) {
      m[f] = i;
      taken.add(i);
    }
  }
  return m;
}

/** Columns that look like secrets; they're never imported. */
export function secretColumns(headers: string[]): string[] {
  return headers.filter((h) => /pass(word)?|secret|private|ssh_key|key ?data|token/i.test(h));
}

export function rowsToHosts(rows: string[][], m: Mapping): { hosts: ImportedHost[]; skipped: number } {
  const hosts: ImportedHost[] = [];
  const seen = new Set<string>();
  let skipped = 0;
  const col = (r: string[], f: Field) => (m[f] === null ? "" : (r[m[f]!] ?? "").trim());
  for (const r of rows) {
    const hostname = col(r, "hostname") || col(r, "label");
    if (!hostname) {
      skipped++;
      continue;
    }
    let alias = col(r, "label") || hostname;
    // Keep labels unique inside the import.
    for (let n = 2; seen.has(alias.toLowerCase()); n++) alias = `${col(r, "label") || hostname} (${n})`;
    seen.add(alias.toLowerCase());
    const port = Number(col(r, "port"));
    const group = col(r, "group").replace(/\s*[,>|]\s*/g, "/");
    hosts.push({
      alias,
      hostname,
      port: Number.isInteger(port) && port > 0 && port < 65536 ? port : 22,
      user: col(r, "user") || null,
      identity_file: null,
      proxy_jump: null,
      forward_agent: false,
      forward_x11: false,
      group: group || null,
      tags: col(r, "tags")
        .split(/[,;]/)
        .map((t) => t.trim())
        .filter(Boolean),
      notes: col(r, "notes") || undefined,
    });
  }
  return { hosts, skipped };
}
