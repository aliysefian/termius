// The template library: complete runbooks, kept as YAML files in runbook-templates/ (the backend's tests read the same files, so
// every template is a valid runbook that plans cleanly). A template's description opens with "Read-only." or "Changes the host:",
// which the gallery turns into a badge.
import { load } from "./runbookdoc";
import type { RunbookParam } from "./runbook";

const files = import.meta.glob("./runbook-templates/*.yaml", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

export interface Template {
  id: string;
  name: string;
  category: string;
  description: string;
  /** What it does to the host, for the badge. */
  effect: "read-only" | "changes";
  /** The variables it asks for. */
  variables: { name: string; label: string; kind: string; required: boolean }[];
  steps: number;
  body: string;
}

export function templateFrom(id: string, body: string): Template | null {
  const l = load(body);
  if (!l.model) return null;
  const m = l.model;
  const read = m.description.trim().startsWith("Read-only");
  return {
    id,
    name: m.name,
    category: m.category || "Custom",
    description: m.description.replace(/^Read-only\.?\s*/, "").replace(/^Changes the host:\s*/, "").trim(),
    effect: read ? "read-only" : "changes",
    variables: (m.params as unknown as RunbookParam[]).map((p) => ({ name: p.name, label: p.label || p.name, kind: p.kind ?? "text", required: !p.optional && (p.default === undefined || p.default === null) })),
    steps: m.steps.length,
    body,
  };
}

export const TEMPLATES: Template[] = Object.entries(files)
  .map(([path, body]) => templateFrom(path.split("/").pop()!.replace(/\.yaml$/, ""), body))
  .filter((t): t is Template => !!t)
  .sort((a, b) => a.category.localeCompare(b.category) || a.name.localeCompare(b.name));

export function searchTemplates(list: Template[], query: string, category: string | null): Template[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return list.filter((t) => (!category || t.category === category) && words.every((w) => `${t.name}\n${t.description}\n${t.category}`.toLowerCase().includes(w)));
}

/** A blank runbook to start from. */
export const BLANK = `name: New runbook
description: What this runbook does.
category: Custom
steps:
  - name: Say hello
    run: echo hello
`;
