// The answers for an encrypted host import: for each host whose name is already used in its group, keep it under
// another name or skip it. Pure, so the rules are tested apart from the dialog.
import type { HostsImportConflict, HostsImportResult, HostsResolution } from "./types";

export type Choice = { action: "rename"; label: string } | { action: "skip" };

/** Everything kept, under the suggested names: the safe answer, nothing is skipped or overwritten. */
export function initialChoices(conflicts: HostsImportConflict[]): Record<string, Choice> {
  return Object.fromEntries(conflicts.map((c) => [c.id, { action: "rename", label: c.suggested } as Choice]));
}

export function setAll(conflicts: HostsImportConflict[], action: "rename" | "skip"): Record<string, Choice> {
  return action === "rename" ? initialChoices(conflicts) : Object.fromEntries(conflicts.map((c) => [c.id, { action: "skip" } as Choice]));
}

const key = (label: string, group: string) => `${group.replace(/^\/+|\/+$/g, "").toLowerCase()}\u0000${label.trim().toLowerCase()}`;

/** What is wrong with the chosen names, by conflict id: empty, the same as the clashing one, or the same as another chosen name. */
export function nameProblems(conflicts: HostsImportConflict[], choices: Record<string, Choice>): Record<string, string> {
  const problems: Record<string, string> = {};
  const used = new Map<string, string>();
  for (const c of conflicts) {
    const ch = choices[c.id];
    if (ch?.action !== "rename") continue;
    const label = ch.label.trim();
    if (!label) problems[c.id] = "Type a name.";
    else if (key(label, c.group) === key(c.label, c.group)) problems[c.id] = "That is the name already in use. Pick a different one.";
    else if (used.has(key(label, c.group))) problems[c.id] = "Another imported host is getting this name.";
    else used.set(key(label, c.group), c.id);
  }
  return problems;
}

export function toResolutions(conflicts: HostsImportConflict[], choices: Record<string, Choice>): HostsResolution[] {
  return conflicts.map((c) => {
    const ch = choices[c.id] ?? ({ action: "rename", label: c.suggested } as Choice);
    return ch.action === "skip" ? { id: c.id, action: "skip" } : { id: c.id, action: "rename", label: ch.label.trim() };
  });
}

const n = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

/** What an import did, in plain lines. */
export function describeResult(r: HostsImportResult): string[] {
  const lines = [`Added ${n(r.hosts_added, "host")}${r.hosts_renamed ? `, ${r.hosts_renamed} under a new name` : ""}.`];
  if (r.hosts_skipped) lines.push(`Skipped ${n(r.hosts_skipped, "host")}.`);
  const extra: string[] = [];
  if (r.groups_added) extra.push(n(r.groups_added, "group"));
  if (r.identities_added) extra.push(n(r.identities_added, "login"));
  if (r.keys_added) extra.push(n(r.keys_added, "key"));
  if (r.proxies_added) extra.push(n(r.proxies_added, "proxy", "proxies"));
  if (extra.length) lines.push(`Also added ${extra.join(", ")}.`);
  return lines;
}
