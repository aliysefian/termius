#!/usr/bin/env node
// Builds src/lib/completion/specs/*.json from the MIT-licensed @withfig/autocomplete package.
//
// Development-time only: the app never runs Node and never reads the package.
// The output is committed, so a normal build does not need this script.
//
//   node scripts/build-completion-specs.mjs              download the pinned version and convert
//   node scripts/build-completion-specs.mjs --from DIR   use an unpacked package (DIR/build/*.js)
//
// What is kept: names, descriptions, subcommands, options, and what each argument is (a file, a
// folder, one of a list, another command, or free text). What is dropped: everything that runs
// code (generators, loadSpec), icons, priorities and hidden or deprecated entries. Remote lookups
// are the app's own job (a fixed allowlist), never something taken from a spec.
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";
import { applyExtras } from "./completion-extra.mjs";

const PACKAGE = "@withfig/autocomplete";
const VERSION = "2.692.3";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "src/lib/completion/specs");

/** The commands worth completing on a server, in the order they appear in the index. */
export const COMMANDS = [
  "git", "docker", "podman", "kubectl", "helm", "terraform", "systemctl", "apt", "npm", "yarn", "pnpm", "node",
  "python3", "cargo", "go", "make", "ssh", "scp", "rsync", "ssh-keygen", "curl", "wget", "ping", "tar", "unzip",
  "zip", "grep", "sed", "find", "xargs", "sort", "cut", "tr", "wc", "diff", "head", "tail", "less", "cat", "ls",
  "cp", "mv", "rm", "mkdir", "touch", "ln", "chmod", "chown", "du", "df", "ps", "kill", "top", "htop", "mount",
  "crontab", "tmux", "screen", "sudo", "env", "time", "which", "man",
];

/**
 * Specs that Fig loads on demand under another command, copied into it here: `docker compose` is
 * its own spec file. Everything else a `loadSpec` points at is left out.
 */
const INLINE = ["docker-compose"];

/**
 * Commands whose root options are global: kubectl's -n, --context and --kubeconfig go with every
 * verb, but the spec lists them once, at the top, without saying so.
 */
const ROOT_OPTIONS_ARE_GLOBAL = ["kubectl", "helm"];

const MAX_DESCRIPTION = 90;

function text(value) {
  if (typeof value !== "string") return undefined;
  const clean = value.replace(/\s+/g, " ").trim();
  if (!clean) return undefined;
  if (clean.length <= MAX_DESCRIPTION) return clean;
  const cut = clean.slice(0, MAX_DESCRIPTION - 1);
  return cut.slice(0, cut.lastIndexOf(" ") > 40 ? cut.lastIndexOf(" ") : cut.length).replace(/[,;:(]$/, "") + "…";
}

const names = (n) => (Array.isArray(n) ? n : [n]).filter((x) => typeof x === "string" && x);
const one = (a) => (a.length === 1 ? a[0] : a);
const skip = (x) => !x || x.hidden || x.deprecated;

function arg(a) {
  if (!a || typeof a !== "object") return null;
  const templates = [].concat(a.template ?? []);
  const o = {};
  if (a.isCommand) o.k = "cmd";
  else if (templates.includes("filepaths")) o.k = "file";
  else if (templates.includes("folders")) o.k = "dir";
  else if (Array.isArray(a.suggestions) && a.suggestions.length) o.k = "enum";
  else o.k = "text";
  const label = text(a.name);
  // Fig finds archives and the like with a generator, which is dropped. An argument that is
  // only called FILE, PATH or DIRECTORY is a file or a folder all the same.
  if (o.k === "text" && label) {
    if (/^(archive|file|files|filename|path|paths|pathname)$/i.test(label)) o.k = "file";
    else if (/^(dir|dirs|directory|directories|folder|altpath)$/i.test(label)) o.k = "dir";
  }
  if (label) o.t = label;
  if (Array.isArray(a.suggestions)) {
    const v = a.suggestions
      .filter((s) => typeof s === "string" || (s && !skip(s) && typeof s.name === "string"))
      .map((s) => (typeof s === "string" ? [s] : [s.name, text(s.description)].filter(Boolean)))
      .slice(0, 60);
    if (v.length) o.v = v.map((x) => (x.length === 1 ? x[0] : x));
  }
  if (a.isOptional) o.o = 1;
  if (a.isVariadic) o.m = 1;
  return o;
}

const args = (a) => {
  const list = (Array.isArray(a) ? a : a ? [a] : []).map(arg).filter(Boolean);
  return list.length ? list : undefined;
};

function option(o) {
  if (skip(o)) return null;
  const n = names(o.name);
  if (!n.length) return null;
  const r = { n: one(n) };
  const d = text(o.description);
  if (d) r.d = d;
  const a = args(o.args);
  if (a) r.a = a;
  if (o.isRepeatable) r.r = 1;
  if (o.isPersistent) r.p = 1;
  if (o.requiresSeparator) r.e = typeof o.requiresSeparator === "string" ? o.requiresSeparator : "=";
  return r;
}

let inline = {};

function node(s) {
  if (typeof s.loadSpec === "string" && inline[s.loadSpec]) s = { ...inline[s.loadSpec], ...s, subcommands: inline[s.loadSpec].subcommands, options: inline[s.loadSpec].options, args: inline[s.loadSpec].args };
  const r = {};
  const d = text(s.description);
  if (d) r.d = d;
  const sub = (s.subcommands ?? []).filter((x) => !skip(x) && names(x.name).length).map((x) => ({ n: one(names(x.name)), ...node(x) }));
  if (sub.length) r.s = sub;
  const opts = (s.options ?? []).map(option).filter(Boolean);
  if (opts.length) r.o = opts;
  const a = args(s.args);
  if (a) r.a = a;
  return r;
}

function load(dir) {
  const have = new Set(readdirSync(join(dir, "build")).map((f) => f.replace(/\.js$/, "")));
  return async (name) => {
    if (!have.has(name)) throw new Error(`${name}: not in ${PACKAGE}@${VERSION}`);
    const mod = await import(pathToFileURL(join(dir, "build", `${name}.js`)).href);
    const spec = mod.default;
    if (!spec || typeof spec !== "object") throw new Error(`${name}: spec is generated by code, not data`);
    return spec;
  };
}

/** `--check`: is there a newer package than the one pinned? Exits 1 when there is, so a scheduled job can say so. */
async function check() {
  const latest = execFileSync("npm", ["view", PACKAGE, "version"], { encoding: "utf8" }).trim();
  console.log(latest === VERSION ? `up to date (${VERSION})` : `pinned ${VERSION}, newest ${latest}: bump VERSION, run this script, review the diff`);
  process.exit(latest === VERSION ? 0 : 1);
}

async function main() {
  if (process.argv.includes("--check")) return check();
  const from = process.argv.indexOf("--from");
  let dir = from > 0 ? resolve(process.argv[from + 1]) : null;
  let temp = null;
  if (!dir) {
    temp = mkdtempSync(join(tmpdir(), "specs-"));
    execFileSync("npm", ["pack", `${PACKAGE}@${VERSION}`, "--silent"], { cwd: temp, stdio: ["ignore", "pipe", "inherit"] });
    const tgz = readdirSync(temp).find((f) => f.endsWith(".tgz"));
    execFileSync("tar", ["xzf", tgz], { cwd: temp });
    dir = join(temp, "package");
  }
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  if (pkg.version !== VERSION) console.warn(`note: package is ${pkg.version}, the script pins ${VERSION}`);
  const get = load(dir);
  for (const name of INLINE) inline[name] = await get(name);

  mkdirSync(OUT, { recursive: true });
  for (const f of readdirSync(OUT)) if (f.endsWith(".json")) rmSync(join(OUT, f));

  const index = {};
  let total = 0;
  let gz = 0;
  for (const name of COMMANDS) {
    let spec;
    try {
      spec = await get(name);
    } catch (e) {
      console.warn(`skipped ${e.message}`);
      continue;
    }
    const body = node(spec);
    if (ROOT_OPTIONS_ARE_GLOBAL.includes(name)) for (const o of body.o ?? []) o.p = 1;
    const json = JSON.stringify(body);
    writeFileSync(join(OUT, `${name}.json`), json + "\n");
    index[name] = body.d ?? "";
    total += json.length;
    gz += gzipSync(json).length;
  }
  // Specs written by hand for what the package lacks go on top (see completion-extra.mjs).
  const { added } = applyExtras(OUT, index);
  writeFileSync(join(OUT, "index.json"), JSON.stringify(index) + "\n");
  writeFileSync(join(OUT, "SOURCE.json"), JSON.stringify({ package: PACKAGE, version: pkg.version, packageLicense: pkg.license, licenceFile: "MIT, see THIRD_PARTY.md", commands: Object.keys(index).length, handwritten: added }, null, 2) + "\n");
  console.log(`${Object.keys(index).length} commands, ${(total / 1024).toFixed(0)} KB raw, ${(gz / 1024).toFixed(0)} KB gzipped`);
  if (temp) rmSync(temp, { recursive: true, force: true });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
