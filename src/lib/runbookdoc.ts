// A runbook document, read and edited as text. The saved document is what the person wrote: JSON (the older form) or YAML,
// and the Builder changes it without rewriting what it doesn't touch. For YAML that means editing the parsed document in
// place, so comments, ordering and unknown keys survive; for JSON it means editing the object and writing it back with
// two-space indentation. Nothing here uses regular expressions on the text. Checking is the backend's (runbook_check); this
// reads the document to draw it and to change it.
import { Document, isAlias, isMap, isScalar, isSeq, LineCounter, parseDocument, visit, type YAMLMap, type YAMLSeq } from "yaml";

export type Format = "json" | "yaml";

/** A document that starts with `{` is JSON, like the backend reads it. */
export const formatOf = (text: string): Format => (text.trimStart().startsWith("{") ? "json" : "yaml");

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
export type Obj = { [k: string]: Json };

export const STEP_KINDS = ["run", "wait", "upload", "download", "delay", "http", "tcp", "docker"] as const;
export type StepKind = (typeof STEP_KINDS)[number];

const TOP_KEYS = ["name", "description", "category", "defaults", "params", "steps", "rollback"];
const STEP_KEYS = ["name", "id", "disabled", ...STEP_KINDS, "when", "on_error", "timeout_secs", "retries", "retry_delay_secs"];
const PARAM_KEYS = ["name", "label", "description", "default", "optional", "choices", "kind"];

export interface DocModel {
  name: string;
  description: string;
  category: string;
  defaults: Obj;
  params: Obj[];
  steps: Obj[];
  rollback: Obj[];
  /** Keys the document has that the Builder doesn't know. They are kept as written. */
  unknownKeys: string[];
}

export interface Loaded {
  format: Format;
  /** Null when the text can't be read as a runbook document at all. */
  model: DocModel | null;
  /** Why not, in the parser's words, with where. */
  error: { message: string; line: number | null; column: number | null } | null;
  /** Reasons the Builder can't edit this document safely (the YAML editor still can). Empty when it can. */
  unsupported: string[];
}

const asObj = (v: unknown): Obj => (v && typeof v === "object" && !Array.isArray(v) ? (v as Obj) : {});
const asList = (v: unknown): Obj[] => (Array.isArray(v) ? v.map(asObj) : []);
const asStr = (v: unknown): string => (typeof v === "string" ? v : "");

/** Read the text into a model. Never throws. */
export function load(text: string): Loaded {
  const format = formatOf(text);
  if (text.trim() === "") return { format, model: null, error: { message: "The document is empty.", line: 1, column: 1 }, unsupported: [] };
  const lc = new LineCounter();
  const doc = parseDocument(text, { lineCounter: lc, prettyErrors: true, uniqueKeys: true });
  const first = doc.errors[0];
  if (first) {
    const pos = first.linePos?.[0];
    // The parser appends the position to its message; it is reported on its own.
    const message = first.message.split("\n")[0].split(" at line ")[0];
    return { format, model: null, error: { message, line: pos?.line ?? null, column: pos?.col ?? null }, unsupported: [] };
  }
  const raw = doc.toJS({ maxAliasCount: 100 });
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { format, model: null, error: { message: "A runbook is a mapping with a name and steps.", line: 1, column: 1 }, unsupported: [] };
  }
  const o = raw as Obj;
  const unknown = new Set<string>();
  for (const k of Object.keys(o)) if (!TOP_KEYS.includes(k)) unknown.add(k);
  for (const list of [o.steps, o.rollback]) for (const s of asList(list)) for (const k of Object.keys(s)) if (!STEP_KEYS.includes(k)) unknown.add(k);
  for (const p of asList(o.params)) for (const k of Object.keys(p)) if (!PARAM_KEYS.includes(k)) unknown.add(k);
  const unsupported: string[] = [];
  let aliases = false;
  visit(doc, { Alias: () => void (aliases = true) });
  if (aliases) unsupported.push("It uses YAML anchors or aliases (&name / *name), which the Builder can't edit without expanding them.");
  if (doc.contents && isMap(doc.contents) && doc.contents.items.some((p) => isScalar(p.key) && p.key.value === "<<")) unsupported.push("It uses YAML merge keys (<<).");
  const dup = [o.steps, o.rollback].some((l) => Array.isArray(l) && l.some((s) => !s || typeof s !== "object" || Array.isArray(s)));
  if (dup) unsupported.push("A step is not a mapping.");
  return {
    format,
    model: {
      name: asStr(o.name),
      description: asStr(o.description),
      category: asStr(o.category),
      defaults: asObj(o.defaults),
      params: asList(o.params),
      steps: asList(o.steps),
      rollback: asList(o.rollback),
      unknownKeys: [...unknown],
    },
    error: null,
    unsupported,
  };
}

// -- editing ---------------------------------------------------------------------------------------

/** What an edit gives back: the new text, or why it couldn't be made. */
export type Edit = { text: string } | { error: string };

type Path = (string | number)[];

/** The document in a form that can be edited by path and written back. */
interface Handle {
  get(path: Path): Json | undefined;
  set(path: Path, value: Json): void;
  remove(path: Path): void;
  /** Put `value` into the list at `path` before `index` (the end if omitted). Makes the list if it is missing. */
  insert(path: Path, value: Json, index?: number): void;
  move(path: Path, from: number, to: number): void;
  length(path: Path): number;
  text(): string;
}

function yamlHandle(doc: Document): Handle {
  const seqAt = (path: Path): YAMLSeq | null => {
    const n = doc.getIn(path, true);
    return isSeq(n) ? n : null;
  };
  return {
    get: (path) => {
      const v = doc.getIn(path);
      return v === undefined ? undefined : (JSON.parse(JSON.stringify(v ?? null)) as Json);
    },
    set: (path, value) => doc.setIn(path, doc.createNode(value)),
    remove: (path) => void doc.deleteIn(path),
    insert: (path, value, index) => {
      const seq = seqAt(path);
      if (!seq) {
        doc.setIn(path, doc.createNode([value]));
        return;
      }
      seq.items.splice(index ?? seq.items.length, 0, doc.createNode(value));
    },
    move: (path, from, to) => {
      const seq = seqAt(path);
      if (!seq) return;
      const [item] = seq.items.splice(from, 1);
      seq.items.splice(to, 0, item);
    },
    length: (path) => seqAt(path)?.items.length ?? 0,
    // A long string is kept on one line: wrapping a command would change how it reads.
    text: () => doc.toString({ lineWidth: 0 }),
  };
}

function jsonHandle(root: Obj): Handle {
  const walk = (path: Path): { parent: unknown; key: string | number | null } => {
    let cur: unknown = root;
    for (const k of path.slice(0, -1)) cur = (cur as Record<string | number, unknown>)?.[k];
    return { parent: cur, key: path.length ? path[path.length - 1] : null };
  };
  const listAt = (path: Path): Json[] | null => {
    let cur: unknown = root;
    for (const k of path) cur = (cur as Record<string | number, unknown>)?.[k];
    return Array.isArray(cur) ? (cur as Json[]) : null;
  };
  return {
    get: (path) => {
      let cur: unknown = root;
      for (const k of path) cur = (cur as Record<string | number, unknown>)?.[k];
      return cur as Json | undefined;
    },
    set: (path, value) => {
      const { parent, key } = walk(path);
      if (parent && key !== null) (parent as Record<string | number, Json>)[key] = value;
    },
    remove: (path) => {
      const { parent, key } = walk(path);
      if (Array.isArray(parent) && typeof key === "number") parent.splice(key, 1);
      else if (parent && key !== null) delete (parent as Record<string, Json>)[key];
    },
    insert: (path, value, index) => {
      const list = listAt(path);
      if (list) list.splice(index ?? list.length, 0, value);
      else {
        const { parent, key } = walk(path);
        if (parent && key !== null) (parent as Record<string | number, Json>)[key] = [value];
      }
    },
    move: (path, from, to) => {
      const list = listAt(path);
      if (!list) return;
      const [item] = list.splice(from, 1);
      list.splice(to, 0, item);
    },
    length: (path) => listAt(path)?.length ?? 0,
    text: () => JSON.stringify(root, null, 2) + "\n",
  };
}

/** Open the text for editing, or say why it can't be. */
function open(text: string): Handle | { error: string } {
  const loaded = load(text);
  if (!loaded.model) return { error: loaded.error ? `The document can't be read: ${loaded.error.message}` : "The document can't be read." };
  if (loaded.unsupported.length) return { error: loaded.unsupported[0] + " Edit it in the YAML view." };
  if (loaded.format === "json") return jsonHandle(JSON.parse(text) as Obj);
  return yamlHandle(parseDocument(text, { uniqueKeys: true }));
}

const isError = (h: Handle | { error: string }): h is { error: string } => "error" in h;

function edit(text: string, change: (h: Handle) => string | void): Edit {
  const h = open(text);
  if (isError(h)) return h;
  const err = change(h);
  return err ? { error: err } : { text: h.text() };
}

const listPath = (rollback: boolean): Path => [rollback ? "rollback" : "steps"];

/** Set a field of the runbook itself (`name`, `description`, `category`, `defaults`…); undefined or an empty string removes it. */
export function setTop(text: string, key: string, value: Json | undefined): Edit {
  return edit(text, (h) => {
    if (value === undefined || value === "" || (value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).length === 0)) h.remove([key]);
    else h.set([key], value);
  });
}

/** Set a field of the runbook's `defaults`, removing the whole block when the last field goes. */
export function setDefault(text: string, key: string, value: Json | undefined): Edit {
  return edit(text, (h) => {
    const cur = (h.get(["defaults"]) ?? {}) as Obj;
    const next: Obj = { ...cur };
    if (value === undefined || value === "" || (Array.isArray(value) && value.length === 0)) delete next[key];
    else next[key] = value;
    if (Object.keys(next).length === 0) h.remove(["defaults"]);
    else if (h.get(["defaults"]) === undefined) h.set(["defaults"], next);
    else if (value === undefined || value === "" || (Array.isArray(value) && value.length === 0)) h.remove(["defaults", key]);
    else h.set(["defaults", key], value);
  });
}

// -- steps ---------------------------------------------------------------------------------------

/** A new step of a kind, with the fields it needs and nothing else. */
export function newStep(kind: StepKind): Obj {
  switch (kind) {
    case "run":
      return { name: "Run a command", run: "echo hello" };
    case "wait":
      return { name: "Wait until ready", wait: { run: "true", every_secs: 5, timeout_secs: 60 } };
    case "upload":
      return { name: "Upload a file", upload: { local: "{{file}}", remote: "/tmp/file" } };
    case "download":
      return { name: "Download a file", download: { remote: "/var/log/syslog", to: "{{folder}}" } };
    case "delay":
      return { name: "Pause", delay: { secs: 5 } };
    case "http":
      return { name: "Check a web address", http: { url: "https://{{host}}/", expect_status: 200, timeout_secs: 10 } };
    case "tcp":
      return { name: "Check a port", tcp: { port: 22, timeout_secs: 10 } };
    case "docker":
      return { name: "Restart a container", docker: { action: "restart", container: "web" } };
  }
}

/** The kind a step is, or null for a step with none or more than one. */
export function kindOf(step: Obj): StepKind | null {
  const k = STEP_KINDS.filter((x) => step[x] !== undefined);
  return k.length === 1 ? k[0] : null;
}

/** A name that isn't used yet, by adding a number. */
function unique(base: string, taken: Set<string>): string {
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base}_${n}`)) return `${base}_${n}`;
}

/** A step id from a name: lowercase letters, digits and underscores, starting with a letter. */
export function idFrom(name: string): string {
  const base = name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 30);
  return /^[a-z]/.test(base) ? base : base ? `s_${base}` : "step";
}

const idsOf = (h: Handle): Set<string> => {
  const ids = new Set<string>();
  for (const list of [h.get(["steps"]), h.get(["rollback"])]) for (const s of asList(list)) if (typeof s.id === "string") ids.add(s.id);
  return ids;
};

export function addStep(text: string, step: Obj, opts: { index?: number; rollback?: boolean } = {}): Edit {
  return edit(text, (h) => {
    const path = listPath(!!opts.rollback);
    h.insert(path, step as Json, opts.index);
  });
}

export function updateStep(text: string, index: number, step: Obj, rollback = false): Edit {
  return edit(text, (h) => {
    const path = [...listPath(rollback), index];
    if (h.get(path) === undefined) return "That step isn't in the document.";
    h.set(path, step as Json);
  });
}

/** Change one field of a step (undefined removes it). */
export function setStepField(text: string, index: number, key: string, value: Json | undefined, rollback = false): Edit {
  return edit(text, (h) => {
    const path = [...listPath(rollback), index];
    if (h.get(path) === undefined) return "That step isn't in the document.";
    if (value === undefined || value === "" || value === false) h.remove([...path, key]);
    else h.set([...path, key], value);
  });
}

export function removeStep(text: string, index: number, rollback = false): Edit {
  return edit(text, (h) => {
    if (h.get([...listPath(rollback), index]) === undefined) return "That step isn't in the document.";
    h.remove([...listPath(rollback), index]);
  });
}

/** Move a step to another place in its list. */
export function moveStep(text: string, from: number, to: number, rollback = false): Edit {
  return edit(text, (h) => {
    const n = h.length(listPath(rollback));
    if (from < 0 || from >= n || to < 0 || to >= n) return "That is not a place in the list.";
    if (from !== to) h.move(listPath(rollback), from, to);
  });
}

/** A copy of a step right after it, with an id and a name that are not used yet. */
export function duplicateStep(text: string, index: number, rollback = false): Edit {
  return edit(text, (h) => {
    const src = h.get([...listPath(rollback), index]);
    if (!src || typeof src !== "object" || Array.isArray(src)) return "That step isn't in the document.";
    const copy = JSON.parse(JSON.stringify(src)) as Obj;
    if (typeof copy.name === "string" && copy.name) copy.name = `${copy.name} (copy)`;
    if (typeof copy.id === "string") copy.id = unique(copy.id, idsOf(h));
    h.insert(listPath(rollback), copy, index + 1);
  });
}

/** Give a step an id (so a later step can look at it), or remove it. */
export function setStepId(text: string, index: number, id: string | undefined, rollback = false): Edit {
  return edit(text, (h) => {
    const taken = idsOf(h);
    const cur = h.get([...listPath(rollback), index, "id"]);
    if (typeof cur === "string") taken.delete(cur);
    if (id !== undefined && taken.has(id)) return `The id "${id}" is already used by another step.`;
    if (id === undefined || id === "") h.remove([...listPath(rollback), index, "id"]);
    else h.set([...listPath(rollback), index, "id"], id);
  });
}

/** Make a step another kind: its name, condition, timeout and failure policy stay; the old kind's settings go. */
export function changeKind(text: string, index: number, kind: StepKind, rollback = false): Edit {
  return edit(text, (h) => {
    const path = [...listPath(rollback), index];
    const cur = h.get(path);
    if (!cur || typeof cur !== "object" || Array.isArray(cur)) return "That step isn't in the document.";
    const keep: Obj = {};
    for (const [k, v] of Object.entries(cur)) if (!(STEP_KINDS as readonly string[]).includes(k)) keep[k] = v;
    const fresh = newStep(kind);
    const body = Object.fromEntries(Object.entries(fresh).filter(([k]) => (STEP_KINDS as readonly string[]).includes(k)));
    // The kind goes where the old one was: after the name, before the conditions.
    const next: Obj = {};
    let put = false;
    for (const [k, v] of Object.entries(keep)) {
      if (!put && ["when", "on_error", "timeout_secs", "retries", "retry_delay_secs", "disabled"].includes(k)) {
        Object.assign(next, body);
        put = true;
      }
      next[k] = v;
    }
    if (!put) Object.assign(next, body);
    h.set(path, next);
  });
}

// -- variables -------------------------------------------------------------------------------------

export function addParam(text: string, param: Obj): Edit {
  return edit(text, (h) => {
    const names = new Set(asList(h.get(["params"])).map((p) => asStr(p.name)));
    if (names.has(asStr(param.name))) return `There is already a variable called ${asStr(param.name)}.`;
    h.insert(["params"], param as Json);
  });
}

export function updateParam(text: string, index: number, param: Obj): Edit {
  return edit(text, (h) => {
    if (h.get(["params", index]) === undefined) return "That variable isn't in the document.";
    h.set(["params", index], param as Json);
  });
}

export function removeParam(text: string, index: number): Edit {
  return edit(text, (h) => {
    if (h.get(["params", index]) === undefined) return "That variable isn't in the document.";
    h.remove(["params", index]);
    if (h.length(["params"]) === 0) h.remove(["params"]);
  });
}

export function moveParam(text: string, from: number, to: number): Edit {
  return edit(text, (h) => {
    const n = h.length(["params"]);
    if (from < 0 || from >= n || to < 0 || to >= n) return "That is not a place in the list.";
    h.move(["params"], from, to);
  });
}

/** A `{{old}}`, `{{ old|q }}` placeholder is rewritten to the new name; text that only looks similar is left alone. */
export function renameInTemplate(template: string, from: string, to: string): string {
  let out = "";
  let rest = template;
  for (;;) {
    const start = rest.indexOf("{{");
    if (start < 0) break;
    const end = rest.indexOf("}}", start + 2);
    if (end < 0) break;
    const inner = rest.slice(start + 2, end);
    const bar = inner.indexOf("|");
    const name = (bar < 0 ? inner : inner.slice(0, bar)).trim();
    const renamed = name === from ? inner.replace(from, to) : inner;
    out += rest.slice(0, start) + "{{" + renamed + "}}";
    rest = rest.slice(end + 2);
  }
  return out + rest;
}

/** Rename a variable and every place that uses it: placeholders in any step text, and the `param` a condition looks at. */
export function renameParam(text: string, index: number, to: string): Edit {
  return edit(text, (h) => {
    const cur = asStr(h.get(["params", index, "name"]));
    if (!cur) return "That variable isn't in the document.";
    if (cur === to) return;
    if (asList(h.get(["params"])).some((p, i) => i !== index && p.name === to)) return `There is already a variable called ${to}.`;
    h.set(["params", index, "name"], to);
    const fix = (path: Path, value: Json): void => {
      if (typeof value === "string") {
        const next = renameInTemplate(value, cur, to);
        if (next !== value) h.set(path, next);
      } else if (Array.isArray(value)) value.forEach((v, i) => fix([...path, i], v));
      else if (value && typeof value === "object") {
        for (const [k, v] of Object.entries(value)) {
          if (k === "param" && v === cur && path[path.length - 1] === "when") h.set([...path, k], to);
          else fix([...path, k], v);
        }
      }
    };
    for (const list of ["steps", "rollback"]) asList(h.get([list])).forEach((s, i) => fix([list, i], s));
  });
}

/** The variable names a text uses in placeholders, in order, without the built-in ones. */
export function usedNames(template: string): string[] {
  const out: string[] = [];
  let rest = template;
  for (;;) {
    const start = rest.indexOf("{{");
    if (start < 0) break;
    const end = rest.indexOf("}}", start + 2);
    if (end < 0) break;
    const inner = rest.slice(start + 2, end);
    const bar = inner.indexOf("|");
    const name = (bar < 0 ? inner : inner.slice(0, bar)).trim();
    if (name && !out.includes(name)) out.push(name);
    rest = rest.slice(end + 2);
  }
  return out;
}

// -- conversion and formatting ------------------------------------------------------------------

/** The same document in the other form. JSON → YAML loses nothing; YAML → JSON loses its comments, and the caller says so. */
export function convert(text: string, to: Format): Edit {
  const loaded = load(text);
  if (!loaded.model) return { error: loaded.error?.message ?? "The document can't be read." };
  if (loaded.format === to) return { text };
  const doc = parseDocument(text, { uniqueKeys: true });
  const value = doc.toJS({ maxAliasCount: 100 }) as Json;
  if (to === "json") return { text: JSON.stringify(value, null, 2) + "\n" };
  return { text: new Document(value).toString({ lineWidth: 0 }) };
}

/** Whether going to `to` would drop comments the text has. */
export function wouldLoseComments(text: string, to: Format): boolean {
  if (to !== "json" || formatOf(text) === "json") return false;
  const doc = parseDocument(text);
  let found = !!doc.commentBefore || !!doc.comment;
  visit(doc, {
    Node(_k, node) {
      if (node.comment || node.commentBefore) found = true;
    },
    Pair(_k, pair) {
      const k = pair.key as { comment?: string; commentBefore?: string } | null;
      if (k?.comment || k?.commentBefore) found = true;
    },
  });
  return found;
}

/** The text written out again with the same structure and normal indentation (comments are kept in YAML). */
export function format(text: string): Edit {
  const loaded = load(text);
  if (!loaded.model) return { error: loaded.error?.message ?? "The document can't be read." };
  if (loaded.format === "json") return { text: JSON.stringify(JSON.parse(text), null, 2) + "\n" };
  return { text: parseDocument(text, { uniqueKeys: true }).toString({ lineWidth: 0 }) };
}

// -- where things are -------------------------------------------------------------------------------

export interface Where {
  /** 1-based line of the first character of the step (or variable) in the text. */
  line: number;
  to: number;
  from: number;
}

/** Where each step, rollback step and variable starts in the text, for putting a problem on its line. */
export function positions(text: string): { steps: Where[]; rollback: Where[]; params: Where[] } {
  const empty = { steps: [], rollback: [], params: [] };
  const lc = new LineCounter();
  const doc = parseDocument(text, { lineCounter: lc, uniqueKeys: true });
  if (doc.errors.length || !isMap(doc.contents)) return empty;
  const list = (key: string): Where[] => {
    const seq = (doc.contents as YAMLMap).get(key, true);
    if (!isSeq(seq)) return [];
    return seq.items.map((n) => {
      const r = (n as { range?: [number, number, number] | null }).range ?? [0, 0, 0];
      return { from: r[0], to: r[1], line: lc.linePos(r[0]).line };
    });
  };
  return { steps: list("steps"), rollback: list("rollback"), params: list("params") };
}

export function isAliasNode(n: unknown): boolean {
  return isAlias(n);
}
