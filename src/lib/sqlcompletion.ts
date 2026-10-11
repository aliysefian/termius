import type { CompletionContext, CompletionSource } from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import { MSSQL, MySQL, PLSQL, PostgreSQL, SQLite, StandardSQL, schemaCompletionSource, type SQLDialect, type SQLNamespace } from "@codemirror/lang-sql";
import type { DbTableInfo, DbTreeNode } from "$lib/types";

export interface SqlMetadataProvider {
  children(path: string[]): Promise<DbTreeNode[]>;
  tableInfo(namespace: string, table: string): Promise<DbTableInfo | null>;
  defaultNamespace?: string;
}

export interface RelationRef {
  namespace: string | null;
  table: string;
}

const IDENT = String.raw`(?:"(?:[^"]|"")*"|` + "`(?:[^`]|``)*`" + String.raw`|[\p{L}_$][\p{L}\p{N}_$]*)`;
const RELATION = new RegExp(String.raw`\b(?:from|join|update|into|delete\s+from)\s+(${IDENT})(?:\s*\.\s*(${IDENT}))?`, "giu");
const QUALIFIER = new RegExp(String.raw`(${IDENT})\s*\.\s*(?:[\p{L}\p{N}_$]*)$`, "u");

export function sqlDialect(engine: string): SQLDialect {
  switch (engine) {
    case "postgres": return PostgreSQL;
    case "mysql": return MySQL;
    case "mssql": return MSSQL;
    case "oracle": return PLSQL;
    case "rqlite": return SQLite;
    default: return StandardSQL;
  }
}

function unquote(name: string): string {
  if (name.startsWith('"') && name.endsWith('"')) return name.slice(1, -1).replace(/""/g, '"');
  if (name.startsWith("`") && name.endsWith("`")) return name.slice(1, -1).replace(/``/g, "`");
  return name;
}

/** Hide strings and comments while preserving identifier quotes and offsets. */
export function executableSql(sql: string, engine: string): string {
  const chars = [...sql];
  const out = [...chars];
  const blank = (from: number, to: number) => {
    for (let n = from; n < to; n++) if (out[n] !== "\n") out[n] = " ";
  };
  for (let i = 0; i < chars.length;) {
    if (chars[i] === "-" && chars[i + 1] === "-") {
      const start = i;
      while (i < chars.length && chars[i] !== "\n") i++;
      blank(start, i);
    } else if (engine === "mysql" && chars[i] === "#") {
      const start = i;
      while (i < chars.length && chars[i] !== "\n") i++;
      blank(start, i);
    } else if (chars[i] === "/" && chars[i + 1] === "*") {
      const start = i;
      i += 2;
      while (i < chars.length && !(chars[i] === "*" && chars[i + 1] === "/")) i++;
      i = Math.min(chars.length, i + 2);
      blank(start, i);
    } else if (chars[i] === "'") {
      const start = i++;
      while (i < chars.length) {
        if (chars[i] === "'" && chars[i + 1] === "'") i += 2;
        else if (chars[i] === "'") { i++; break; }
        else if (engine === "mysql" && chars[i] === "\\") i += 2;
        else i++;
      }
      blank(start, Math.min(i, chars.length));
    } else if (engine === "postgres" && chars[i] === "$") {
      const rest = chars.slice(i).join("");
      const tag = /^\$[\p{L}\p{N}_]*\$/u.exec(rest)?.[0];
      if (!tag) { i++; continue; }
      const start = i;
      i += [...tag].length;
      const tail = chars.slice(i).join("");
      const close = tail.indexOf(tag);
      i = close < 0 ? chars.length : i + [...tail.slice(0, close + tag.length)].length;
      blank(start, i);
    } else {
      i++;
    }
  }
  return out.join("");
}

export function referencedRelations(sql: string, engine: string): RelationRef[] {
  const clean = executableSql(sql, engine);
  const found: RelationRef[] = [];
  for (const match of clean.matchAll(RELATION)) {
    const first = unquote(match[1]);
    const second = match[2] ? unquote(match[2]) : null;
    const ref = second ? { namespace: first, table: second } : { namespace: null, table: first };
    if (!found.some((r) => r.namespace === ref.namespace && r.table === ref.table)) found.push(ref);
  }
  return found;
}

function qualifierBeforeCursor(sql: string, engine: string): string | null {
  const match = QUALIFIER.exec(executableSql(sql, engine));
  return match ? unquote(match[1]) : null;
}

const MAX_NAMESPACES = 100;
const MAX_RELATIONS = 500;
const MAX_REFERENCES = 24;

function isCode(context: CompletionContext): boolean {
  for (let node = syntaxTree(context.state).resolveInner(context.pos, -1); node; node = node.parent!) {
    if (/String|Comment/.test(node.name)) return false;
    if (!node.parent) break;
  }
  return true;
}

/** Schema-aware completion layered on CodeMirror's dialect parser and alias resolver. */
export function sqlMetadataCompletion(engine: string, provider: SqlMetadataProvider): CompletionSource {
  const dialect = sqlDialect(engine);
  return async (context) => {
    if ((engine !== "postgres" && engine !== "mysql") || !isCode(context)) return null;
    const before = context.state.sliceDoc(0, context.pos);
    const statement = context.state.sliceDoc();
    const roots = (await provider.children([])).filter((n) => n.kind === "schema" || n.kind === "database").slice(0, MAX_NAMESPACES);
    if (context.aborted) return null;

    const names = new Map(roots.map((n) => [n.name.toLocaleLowerCase(), n.name]));
    const configuredDefault = provider.defaultNamespace && names.get(provider.defaultNamespace.toLocaleLowerCase());
    const defaultNamespace = configuredDefault ?? (roots.length === 1 ? roots[0].name : undefined);
    // SQL commonly names tables after the SELECT list, so inspect the whole
    // statement even when completion is requested before FROM.
    const refs = referencedRelations(statement, engine).slice(-MAX_REFERENCES);
    const qualifier = qualifierBeforeCursor(before, engine);
    const wanted = new Set<string>();
    if (defaultNamespace) wanted.add(defaultNamespace);
    if (qualifier && names.has(qualifier.toLocaleLowerCase())) wanted.add(names.get(qualifier.toLocaleLowerCase())!);
    for (const ref of refs) {
      const namespace = ref.namespace ? names.get(ref.namespace.toLocaleLowerCase()) : defaultNamespace;
      if (namespace) wanted.add(namespace);
    }

    const relationLists = new Map<string, DbTreeNode[]>();
    await Promise.all([...wanted].map(async (namespace) => {
      relationLists.set(namespace, (await provider.children([namespace])).filter((n) => n.kind === "table" || n.kind === "view").slice(0, MAX_RELATIONS));
    }));
    if (context.aborted) return null;

    const info = new Map<string, DbTableInfo>();
    await Promise.all(refs.map(async (ref) => {
      let namespace = ref.namespace ? names.get(ref.namespace.toLocaleLowerCase()) : defaultNamespace;
      if (!namespace && roots.length === 1) namespace = roots[0].name;
      if (!namespace) return;
      const relation = relationLists.get(namespace)?.find((n) => n.name.toLocaleLowerCase() === ref.table.toLocaleLowerCase());
      if (!relation) return;
      const table = await provider.tableInfo(namespace, relation.name);
      if (table) info.set(`${namespace}\u0000${relation.name}`, table);
    }));
    if (context.aborted) return null;

    const schema: Record<string, SQLNamespace> = {};
    for (const root of roots) {
      const relations: Record<string, SQLNamespace> = {};
      for (const relation of relationLists.get(root.name) ?? []) {
        const table = info.get(`${root.name}\u0000${relation.name}`);
        relations[relation.name] = {
          self: { label: relation.name, type: relation.kind === "view" ? "interface" : "class", detail: root.name },
          children: (table?.columns ?? []).map((column) => ({ label: column.name, type: "property", detail: column.data_type })),
        };
      }
      schema[root.name] = relations;
    }
    return schemaCompletionSource({ dialect, schema, defaultSchema: defaultNamespace })(context);
  };
}
