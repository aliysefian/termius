// Reads a half-typed shell line and says what could come next: subcommands,
// options, values and command names, from the bundled specs in ./specs.
// Pure: it never touches the terminal, the network or the clock.

// -- the spec format (see scripts/build-completion-specs.mjs) --------------------

export interface Arg {
  /** file and dir are the host's to list (see remote paths); cmd is another command line. */
  k: "file" | "dir" | "cmd" | "enum" | "text";
  /** What it is called in the spec, for a hint. */
  t?: string;
  /** For enum: the values, each a name or [name, description]. */
  v?: (string | string[])[];
  /** Optional, and variadic (any number of them). */
  o?: 1;
  m?: 1;
}

export interface Opt {
  n: string | string[];
  d?: string;
  a?: Arg[];
  /** May be given more than once. */
  r?: 1;
  /** Also valid in every subcommand below this one. */
  p?: 1;
  /** The value must follow a separator (usually "="), not a space. */
  e?: string;
}

export interface Node {
  n?: string | string[];
  d?: string;
  s?: Node[];
  o?: Opt[];
  a?: Arg[];
}

export interface Specs {
  /** Whether a spec by this name exists (loaded or not). */
  known(command: string): boolean;
  /** The spec if it is loaded. */
  get(command: string): Node | undefined;
  /** Command names, for completing the first word. */
  names(): [string, string][];
}

// -- reading the line ----------------------------------------------------------

export interface Parsed {
  /** The words of the command being typed, before the one being typed, unquoted. */
  words: string[];
  /** The word being typed, unquoted, and exactly as typed. */
  current: string;
  raw: string;
  /** Inside an unclosed quote: not ours to complete. */
  quoted: boolean;
  /** After a redirection (> or <), where a file name goes. */
  redirect: boolean;
}

const ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*=/;
const REDIRECT_ONLY = /^\d*(?:>>?|<<?<?|&>>?|>&|<&)\d*$/;
const REDIRECT_WITH_TARGET = /^\d*(?:>>?|<<?<?|&>>?|>&|<&)./;

/** Cut a line into the words of its last simple command (after |, ||, &&, ;, & and an opening $( or backtick). */
export function parseLine(line: string): Parsed {
  let words: string[] = [];
  let cur = "";
  let raw = "";
  let started = false;
  let quote: "" | "'" | '"' = "";
  let skipNext = false;

  const endWord = () => {
    if (!started) return;
    if (skipNext) skipNext = false;
    else if (REDIRECT_ONLY.test(raw) && raw === cur) skipNext = true;
    else if (REDIRECT_WITH_TARGET.test(raw) && raw === cur) {
      // `2>file`: the target is part of this word; nothing for the command.
    } else if (!(words.length === 0 && ASSIGNMENT.test(cur))) words.push(cur);
    cur = "";
    raw = "";
    started = false;
  };
  const newCommand = () => {
    endWord();
    words = [];
    skipNext = false;
  };

  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote === "'") {
      raw += c;
      if (c === "'") quote = "";
      else cur += c;
      continue;
    }
    if (c === "\\") {
      raw += c;
      started = true;
      if (i + 1 < line.length) {
        const n = line[++i];
        raw += n;
        // In double quotes a backslash only escapes a few characters.
        cur += quote === '"' && !'"\\$`'.includes(n) ? c + n : n;
      }
      continue;
    }
    if (quote === '"') {
      raw += c;
      if (c === '"') quote = "";
      else cur += c;
      continue;
    }
    if (c === "'" || c === '"') {
      quote = c;
      raw += c;
      started = true;
      continue;
    }
    if (c === "`" || (c === "(" && line[i - 1] === "$")) {
      newCommand();
      continue;
    }
    if (c === "|" || c === ";" || c === "\n" || c === "(" || c === ")") {
      newCommand();
      continue;
    }
    if (c === "&") {
      // 2>&1 and &> are redirections, not the end of a command.
      if (line[i - 1] === ">" || line[i - 1] === "<" || line[i + 1] === ">") {
        raw += c;
        cur += c;
        started = true;
        continue;
      }
      newCommand();
      continue;
    }
    if (c === " " || c === "\t") {
      endWord();
      continue;
    }
    raw += c;
    cur += c;
    started = true;
  }
  const redirect = skipNext || (started && REDIRECT_WITH_TARGET.test(raw) && raw === cur);
  return { words, current: cur, raw, quoted: quote !== "", redirect };
}

// -- walking the spec --------------------------------------------------------------

/** Commands that run the next command: after their own options, the first word is a new command. */
const WRAPPERS = new Set(["sudo", "env", "xargs", "time", "nohup", "nice", "exec", "command", "watch", "timeout", "doas"]);

const asList = (n: string | string[] | undefined): string[] => (n === undefined ? [] : Array.isArray(n) ? n : [n]);
const basename = (p: string) => p.slice(p.lastIndexOf("/") + 1);

function findOption(chain: Node[], flag: string): Opt | undefined {
  for (let i = chain.length - 1; i >= 0; i--) {
    for (const o of chain[i].o ?? []) {
      if (asList(o.n).includes(flag) && (i === chain.length - 1 || o.p)) return o;
    }
  }
  return undefined;
}

function argAt(node: Node, position: number): Arg | undefined {
  const a = node.a;
  if (!a?.length) return undefined;
  if (position < a.length) return a[position];
  const last = a[a.length - 1];
  return last.m ? last : undefined;
}

const takesValue = (o: Opt) => !!o.a?.length && !o.a[0].o;

export interface Walk {
  /** The command and the subcommands walked into, deepest last. */
  chain: Node[];
  /** Their names: ["git", "checkout"]. */
  path: string[];
  /** Options already given, so they are not offered twice. */
  used: Set<Opt>;
  /** Positional words consumed in the deepest node. */
  positional: number;
  /** The previous word was an option that wants this one as its value. */
  pending: Opt | null;
  afterDashes: boolean;
  /** Nothing is typed yet but the command's name. */
  commandWord: boolean;
  /** A command that has a spec but whose spec is not loaded yet. */
  missing?: string;
  /** No spec for the command. */
  unknown?: string;
}

/** Walk `words` (command name first) through the specs. */
export function walk(words: string[], specs: Specs): Walk {
  const w: Walk = { chain: [], path: [], used: new Set(), positional: 0, pending: null, afterDashes: false, commandWord: words.length === 0 };
  if (words.length === 0) return w;
  const name = basename(words[0]);
  if (!specs.known(name)) {
    w.unknown = name;
    return w;
  }
  const root = specs.get(name);
  if (!root) {
    w.missing = name;
    return w;
  }
  w.chain = [root];
  w.path = [name];
  const wrapper = WRAPPERS.has(name);

  for (let i = 1; i < words.length; i++) {
    const word = words[i];
    const node = w.chain[w.chain.length - 1];
    if (w.pending) {
      w.pending = null;
      continue;
    }
    if (!w.afterDashes && word === "--") {
      w.afterDashes = true;
      continue;
    }
    if (!w.afterDashes && word.startsWith("-") && word.length > 1) {
      const eq = word.indexOf("=");
      const flag = eq > 0 ? word.slice(0, eq) : word;
      const opt = findOption(w.chain, word) ?? findOption(w.chain, flag);
      // tar -x or --extract: the spec makes the operation a subcommand that is spelled like an option.
      const dashed = w.positional === 0 ? node.s?.find((s) => asList(s.n).includes(flag)) : undefined;
      if (!opt && dashed) {
        w.chain.push(dashed);
        w.path.push(asList(dashed.n)[0]);
        continue;
      }
      if (opt) {
        w.used.add(opt);
        if (eq < 0 && takesValue(opt)) w.pending = opt;
        continue;
      }
      // -xzf: several one-letter options in a row, the last of which may take a value.
      if (!word.startsWith("--") && eq < 0) {
        for (let k = 1; k < word.length; k++) {
          let o = findOption(w.chain, `-${word[k]}`);
          if (!o) {
            // The first letter may name the operation (-xzf: extract, then z and f).
            const here = w.chain[w.chain.length - 1];
            const sub = k === 1 && w.positional === 0 ? here.s?.find((s) => asList(s.n).includes(`-${word[k]}`)) : undefined;
            if (!sub) break;
            w.chain.push(sub);
            w.path.push(asList(sub.n)[0]);
            continue;
          }
          w.used.add(o);
          if (takesValue(o)) {
            if (k === word.length - 1) w.pending = o;
            break;
          }
        }
      }
      continue;
    }
    // A positional word.
    if (wrapper && w.chain.length === 1) {
      if (ASSIGNMENT.test(word)) continue;
      const inner = walk(words.slice(i), specs);
      return inner;
    }
    if (w.positional === 0 && node.s) {
      const sub = node.s.find((s) => asList(s.n).includes(word));
      if (sub) {
        w.chain.push(sub);
        w.path.push(asList(sub.n)[0]);
        continue;
      }
    }
    const arg = argAt(node, w.positional);
    if (arg?.k === "cmd") return walk(words.slice(i), specs);
    w.positional++;
  }
  return w;
}

// -- what to offer --------------------------------------------------------------

export interface Suggestion {
  kind: "command" | "subcommand" | "option" | "value";
  name: string;
  description: string;
  /** What replaces the word being typed, with the space or "=" that goes after. */
  insert: string;
  /** How many characters of the line it replaces (the word as typed). */
  replaces: number;
}

export interface Completion {
  items: Suggestion[];
  /** A spec to load before there is anything to offer. */
  missing?: string;
  /** What the argument here is, when it is for the host to list (a file or folder). */
  expects?: "file" | "dir";
  /** A read-only lookup (git branches, containers, units) whose names fit here; see `GENERATORS`. */
  generator?: string;
  /** The word being typed, as typed and unquoted, for completing it as a path. */
  word: { raw: string; value: string };
  /** The word is a path to complete over the connection, and whether only folders fit. */
  path?: "file" | "dir";
}

const SAFE = /^[\w@%+=:,./-]+$/;
const quoteIfNeeded = (s: string) => (SAFE.test(s) ? s : `'${s.replace(/'/g, "'\\''")}'`);

export const MAX_SPEC_ITEMS = 10;

// Which names fit where. The ids are the host-side lookups in `completion.rs`; a spec's own generators
// (code) are never run, so this is the whole list. [command, subcommands that take it, position].
const GIT_REF = ["checkout", "switch", "branch", "merge", "rebase", "log", "show", "diff", "cherry-pick", "reset"];
const UNIT = ["start", "stop", "restart", "reload", "status", "enable", "disable", "is-active", "is-enabled", "is-failed", "show", "cat", "mask", "unmask"];
const CONTAINER = ["exec", "logs", "start", "stop", "restart", "rm", "kill", "inspect", "attach", "top", "port", "pause", "unpause", "stats"];
const IMAGE = ["rmi", "run", "history", "tag", "push"];
const KUBE_POD = ["logs", "exec", "describe"];

/** The lookup whose names fit the next word, or nothing. `option` is set when the word is an option's value. */
export function generatorFor(path: string[], positional: number, option?: string): string | undefined {
  const [cmd, sub] = path;
  if (cmd === "kubectl" && option) {
    if (option === "--context") return "kubectl-contexts";
    if (option === "-n" || option === "--namespace") return "kubectl-namespaces";
  }
  if (option) return undefined;
  if (cmd === "git" && GIT_REF.includes(sub)) return "git-branches";
  if (cmd === "systemctl" && UNIT.includes(sub)) return "systemd-units";
  if (cmd === "docker" && positional === 0) {
    if (CONTAINER.includes(sub)) return "docker-containers";
    if (IMAGE.includes(sub)) return "docker-images";
  }
  if (cmd === "kubectl" && positional === 0 && KUBE_POD.includes(sub)) return "kubectl-pods";
  return undefined;
}

const LOOKS_LIKE_PATH = /^(?:\/|~|\.{1,2}\/)|\//;

function values(arg: Arg | undefined): [string, string][] {
  return (arg?.k === "enum" ? arg.v ?? [] : []).map((v) => (typeof v === "string" ? [v, ""] : [v[0], v[1] ?? ""]));
}

type Inner = Omit<Completion, "word" | "path">;

/** What fits after `line`, which is the text up to the cursor. */
export function completeLine(line: string, specs: Specs): Completion {
  const p = parseLine(line);
  // After --opt= the word to complete is what follows the equals sign.
  const eq = !p.quoted && p.current.startsWith("--") ? p.current.indexOf("=") : -1;
  const word = eq > 0 ? { raw: p.raw.slice(p.raw.indexOf("=") + 1), value: p.current.slice(eq + 1) } : { raw: p.raw, value: p.current };
  if (p.quoted) return { items: [], word };
  const inner: Inner = p.redirect ? { items: [] } : suggest(p, specs);
  let path: Completion["path"];
  if (p.redirect) path = "file";
  else if (inner.expects) path = inner.expects;
  else if (!p.current.startsWith("-") && LOOKS_LIKE_PATH.test(word.value)) path = "file";
  return { ...inner, word, path };
}

function suggest(p: Parsed, specs: Specs): Inner {
  if (p.quoted || p.redirect) return { items: [] };
  const w = walk(p.words, specs);
  if (w.missing) return { items: [], missing: w.missing };
  if (w.unknown) return { items: [] };
  const cur = p.current;
  const replaces = [...p.raw].length;
  const out: Suggestion[] = [];
  const add = (kind: Suggestion["kind"], name: string, description: string, suffix = " ", whole?: string) => {
    out.push({ kind, name, description, insert: (whole ?? quoteIfNeeded(name)) + suffix, replaces });
  };

  // The first word: a command's name.
  if (w.commandWord) {
    if (!cur) return { items: [] };
    for (const [n, d] of specs.names()) if (n.startsWith(cur) && n !== cur) add("command", n, d);
    return { items: out.slice(0, MAX_SPEC_ITEMS) };
  }

  const node = w.chain[w.chain.length - 1];

  // The value of an option, after it or after "--opt=".
  const valueFor = (opt: Opt, prefix: string, head: string) => {
    for (const [v, d] of values(opt.a?.[0])) if (v.startsWith(prefix)) add("value", v, d, " ", head + quoteIfNeeded(v));
    const k = opt.a?.[0]?.k;
    const generator = asList(opt.n).map((n) => generatorFor(w.path, 0, n)).find(Boolean);
    return { items: out.slice(0, MAX_SPEC_ITEMS), expects: k === "file" || k === "dir" ? k : undefined, generator } satisfies Inner;
  };
  if (w.pending) return valueFor(w.pending, cur, "");
  if (!w.afterDashes && cur.startsWith("--") && cur.includes("=")) {
    const eq = cur.indexOf("=");
    const opt = findOption(w.chain, cur.slice(0, eq));
    return opt && opt.a?.length ? valueFor(opt, cur.slice(eq + 1), cur.slice(0, eq + 1)) : { items: [] };
  }

  if (!w.afterDashes && cur.startsWith("-")) {
    const seen = new Set<string>();
    const scopes = w.chain.map((n, i) => ({ n, i })).reverse();
    for (const { n, i } of scopes) {
      for (const o of n.o ?? []) {
        if (i < w.chain.length - 1 && !o.p) continue;
        if (w.used.has(o) && !o.r) continue;
        for (const name of asList(o.n)) {
          // Bare letters (tar's old style) are not offered; they are not what anyone types here.
          if (!name.startsWith("-") || !name.startsWith(cur) || name === cur || seen.has(name)) continue;
          seen.add(name);
          add("option", name, o.d ?? "", o.e ? o.e : " ");
        }
      }
    }
    if (w.positional === 0) {
      for (const s of node.s ?? []) {
        const hit = asList(s.n).find((n) => n.startsWith("-") && n.startsWith(cur) && n !== cur && !seen.has(n));
        if (hit) add("subcommand", hit, s.d ?? "");
      }
    }
    return { items: out.slice(0, MAX_SPEC_ITEMS) };
  }

  // A subcommand or the value of the argument that is next.
  if (w.positional === 0 && node.s && !w.afterDashes) {
    for (const s of node.s) {
      const names = asList(s.n);
      const hit = names.find((n) => !n.startsWith("-") && n.startsWith(cur) && n !== cur);
      if (hit) add("subcommand", hit, s.d ?? "");
    }
  }
  const arg = argAt(node, w.positional);
  for (const [v, d] of values(arg)) if (v.startsWith(cur) && v !== cur) add("value", v, d);
  if (arg?.k === "cmd" && cur) for (const [n, d] of specs.names()) if (n.startsWith(cur) && n !== cur) add("command", n, d);
  const expects = arg?.k === "file" || arg?.k === "dir" ? arg.k : undefined;
  const generator = expects || w.afterDashes ? undefined : generatorFor(w.path, w.positional);
  return { items: out.slice(0, MAX_SPEC_ITEMS), expects, generator };
}
