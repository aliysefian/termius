// What differs between database engines in the window: the name, the default port, which fields the connection form
// shows, how a name is quoted, and the text a table, a key, a collection or an index opens as. Pure, so it is tested.
import type { DbTls } from "./types";

export interface EngineOption {
  /** The key in the connection's `options` (see src-tauri/src/models.rs DbConnection). */
  key: string;
  label: string;
  placeholder?: string;
  help?: string;
  choices?: { value: string; label: string }[];
  /** Shown only while the encryption setting verifies the certificate. */
  onlyWhenVerifying?: boolean;
}

export interface EngineInfo {
  value: string;
  label: string;
  port: number;
  /** The console takes SQL (so SELECT, UPDATE and the SQL safety wording apply). */
  sql: boolean;
  /** What the console takes, for its label. */
  console: string;
  consolePlaceholder: string;
  /** "UPDATE" or "change": what an inline edit is called in its confirmation. */
  change: string;
  user: { label: string; required?: boolean } | null;
  password: { label: string };
  /** The database field; null hides it. */
  database: { label: string; placeholder?: string; help?: string } | null;
  options: EngineOption[];
  /** What the person should know before using it. */
  note?: string;
}

const verify: Pick<EngineOption, "key" | "label" | "placeholder" | "help" | "onlyWhenVerifying"> = {
  key: "ca_file",
  label: "Certificate authority file",
  placeholder: "/etc/ssl/my-ca.pem",
  help: "A private CA's certificate (PEM), when the server's certificate isn't from a public one.",
  onlyWhenVerifying: true,
};

export const ENGINE_LIST: EngineInfo[] = [
  {
    value: "mysql",
    label: "MySQL / MariaDB",
    port: 3306,
    sql: true,
    console: "SQL",
    consolePlaceholder: "SELECT * FROM … (Ctrl+Enter to run; select text to run only that)",
    change: "UPDATE",
    user: { label: "User" },
    password: { label: "Password" },
    database: { label: "Database", help: "(optional, the one to start in)" },
    options: [],
  },
  {
    value: "postgres",
    label: "PostgreSQL",
    port: 5432,
    sql: true,
    console: "SQL",
    consolePlaceholder: "SELECT * FROM … (Ctrl+Enter to run; select text to run only that)",
    change: "UPDATE",
    user: { label: "User" },
    password: { label: "Password" },
    database: {
      label: "Database",
      placeholder: "postgres",
      help: "(defaults to postgres)",
    },
    options: [],
    note: "A PostgreSQL connection is to one database. Add a connection for each database you want to browse; its schemas appear in the tree.",
  },
  {
    value: "mssql",
    label: "SQL Server",
    port: 1433,
    sql: true,
    console: "T-SQL",
    consolePlaceholder: "SELECT * FROM … (Ctrl+Enter to run; select text to run only that)",
    change: "UPDATE",
    user: { label: "Login", required: true },
    password: { label: "Password" },
    database: { label: "Database", help: "(optional, the login's default if empty)" },
    options: [verify],
    note: "A SQL Server connection is to one database; its schemas appear in the tree. A named instance is reached by its port. DOMAIN\\user (a Windows login) works in the Windows version only; elsewhere use a SQL login. Not yet tried against a real server.",
  },
  {
    value: "oracle",
    label: "Oracle Database",
    port: 1521,
    sql: true,
    console: "SQL or PL/SQL",
    consolePlaceholder: "SELECT * FROM … (Ctrl+Enter to run; a PL/SQL block ends with ;)",
    change: "UPDATE",
    user: { label: "User", required: true },
    password: { label: "Password" },
    database: { label: "Service name", placeholder: "FREEPDB1", help: "(like FREEPDB1 or ORCL)" },
    options: [
      { key: "sid", label: "SID", placeholder: "XE", help: "Only for an older database that has a SID and no service name; used instead of the service name." },
      verify,
      { key: "wallet", label: "Wallet folder", placeholder: "/path/to/wallet", help: "An Oracle wallet (ewallet.pem), for Oracle Cloud and enterprise TCPS.", onlyWhenVerifying: true },
    ],
    note: "No Oracle client library is needed: this speaks Oracle's own protocol (12c or later). Data changes are committed as soon as they succeed. Not yet tried against a real server.",
  },
  {
    value: "rqlite",
    label: "rqlite",
    port: 4001,
    sql: true,
    console: "SQL (SQLite)",
    consolePlaceholder: "SELECT * FROM … (Ctrl+Enter to run; select text to run only that)",
    change: "UPDATE",
    user: { label: "User" },
    password: { label: "Password" },
    database: null,
    options: [
      {
        key: "level",
        label: "Read consistency",
        help: "How sure a read is to see the latest write. Weak is rqlite's own default; Strong goes through the cluster's leader and Raft; None may read stale data from any node.",
        choices: [
          { value: "weak", label: "Weak (default)" },
          { value: "none", label: "None (fastest, may be stale)" },
          { value: "linearizable", label: "Linearizable" },
          { value: "strong", label: "Strong (through Raft)" },
        ],
      },
    ],
    note: "The address is a node's HTTP API. Most nodes run without TLS: set Encryption to None (or reach it through SSH).",
  },
  {
    value: "redis",
    label: "Redis",
    port: 6379,
    sql: false,
    console: "Redis commands",
    consolePlaceholder: "GET key · HGETALL key · SCAN 0 MATCH user:* (Ctrl+Enter to run; start with @3 to use database 3; VIEW key reads any key)",
    change: "change",
    user: { label: "User", },
    password: { label: "Password" },
    database: { label: "Database number", placeholder: "0", help: "(the one commands run in)" },
    options: [],
    note: "Keys are listed with SCAN, grouped by the part before a colon. A user name is for Redis 6 ACL users (leave it empty for the default user). Most Redis servers run without TLS: set Encryption to None (or reach it through SSH).",
  },
  {
    value: "mongodb",
    label: "MongoDB",
    port: 27017,
    sql: false,
    console: "a database command (JSON)",
    consolePlaceholder: '{"find": "users", "filter": {"age": {"$gt": 30}}}  (Ctrl+Enter to run; "$db" picks the database)',
    change: "change",
    user: { label: "User" },
    password: { label: "Password" },
    database: { label: "Database", help: "(the one commands run in; also where the user signs in unless set below)" },
    options: [
      { key: "auth_source", label: "Authentication database", placeholder: "admin", help: "Where the user is defined, when not the database above." },
      {
        key: "uri",
        label: "Connection string",
        placeholder: "mongodb+srv://cluster0.example.mongodb.net/shop",
        help: "For Atlas (mongodb+srv://) or a replica set; the server above is then ignored (enter any name). Leave the password out of the string: the user and password above are added to it. Not available through an SSH host.",
      },
      verify,
    ],
    note: "Commands are JSON, the way db.runCommand takes them. Opening a collection writes its find for you. The sign-in (SCRAM) has not been tried against a real server.",
  },
  {
    value: "elasticsearch",
    label: "Elasticsearch / OpenSearch",
    port: 9200,
    sql: false,
    console: "a request",
    consolePlaceholder: "GET /my-index/_search  then a JSON body on the next lines (Ctrl+Enter to run)",
    change: "change",
    user: { label: "User" },
    password: { label: "Password, API key or token" },
    database: null,
    options: [
      {
        key: "auth",
        label: "Sign in with",
        help: "Basic is a user and password. For an API key or a bearer token, put it in the password field and leave the user empty.",
        choices: [
          { value: "basic", label: "User and password" },
          { value: "api_key", label: "API key" },
          { value: "bearer", label: "Bearer token" },
        ],
      },
      verify,
    ],
    note: "Requests are written like Kibana's console: a method and a path, then the JSON body. Searches are limited to the row limit. A local node often runs without TLS: set Encryption to None (or reach it through SSH).",
  },
];

export const DB_ENGINES = ENGINE_LIST.map((e) => ({ value: e.value, label: e.label, port: e.port }));

export function engineInfo(value: string): EngineInfo {
  return ENGINE_LIST.find((e) => e.value === value) ?? ENGINE_LIST[0];
}

export const isSql = (engine: string) => engineInfo(engine).sql;

/** Which TLS choice a freshly chosen engine starts with: the secure one for all of them. */
export const DEFAULT_TLS: DbTls = "verify_full";

// -- names ----------------------------------------------------------------------------------------

/** One name quoted the way the engine reads it. */
export function quoteIdent(engine: string, name: string): string {
  switch (engine) {
    case "mssql":
      return `[${name.replace(/]/g, "]]")}]`;
    case "postgres":
    case "oracle":
    case "rqlite":
      return `"${name.replace(/"/g, '""')}"`;
    default:
      return `\`${name.replace(/`/g, "``")}\``;
  }
}

export const quoteName = (engine: string, ...parts: string[]) => parts.map((p) => quoteIdent(engine, p)).join(".");

/** A Redis argument as the console takes it: bare when it is plain, in double quotes with escapes when not. */
export function redisArg(s: string): string {
  if (s !== "" && /^[A-Za-z0-9_\-.:/@+=%]+$/.test(s)) return s;
  let out = '"';
  for (const c of s) {
    if (c === '"') out += '\\"';
    else if (c === "\\") out += "\\\\";
    else if (c === "\n") out += "\\n";
    else if (c === "\r") out += "\\r";
    else if (c === "\t") out += "\\t";
    else if (c.charCodeAt(0) < 32) out += `\\x${c.charCodeAt(0).toString(16).padStart(2, "0")}`;
    else out += c;
  }
  return `${out}"`;
}

/**
 * The console text that opens one item from the tree: a table's rows, a key, a collection, an index. `database` is
 * the first level of the tree above it (a MySQL database, a PostgreSQL, SQL Server or Oracle schema, a Redis `dbN`,
 * a MongoDB database, an Elasticsearch cluster).
 */
export function browseText(engine: string, database: string, name: string): string {
  switch (engine) {
    case "redis": {
      const n = /^db(\d+)$/.exec(database)?.[1];
      return `${n !== undefined ? `@${n} ` : ""}VIEW ${redisArg(name)}`;
    }
    case "mongodb":
      return `{"$db": ${JSON.stringify(database)}, "find": ${JSON.stringify(name)}, "filter": {}}`;
    case "elasticsearch":
      return `GET /${encodeURIComponent(name)}/_search\n{\n  "query": { "match_all": {} }\n}`;
    default:
      return `SELECT * FROM ${quoteName(engine, database, name)}`;
  }
}

/** Whether the item is something a tab can browse (a table, a view, a key, a collection, an index). */
export const isBrowsable = (kind: string) => kind === "table" || kind === "view";

/** The settings of a connection with the blank ones dropped, as they are saved. */
export function cleanOptions(engine: string, options: Record<string, string>): Record<string, string> | undefined {
  const known = new Set(engineInfo(engine).options.map((o) => o.key));
  const out = Object.fromEntries(Object.entries(options).map(([k, v]) => [k, v.trim()]).filter(([k, v]) => known.has(k) && v !== ""));
  return Object.keys(out).length ? out : undefined;
}
