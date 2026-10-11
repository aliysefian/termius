import { CompletionContext } from "@codemirror/autocomplete";
import { sql } from "@codemirror/lang-sql";
import { EditorState } from "@codemirror/state";
import { describe, expect, it, vi } from "vitest";
import { executableSql, referencedRelations, sqlDialect, sqlMetadataCompletion, type SqlMetadataProvider } from "../sqlcompletion";

const nodes = (items: [string, "schema" | "database" | "table" | "view"][]) =>
  items.map(([name, kind]) => ({ name, kind, expandable: true }));

function provider(engine: "postgres" | "mysql" = "postgres"): SqlMetadataProvider {
  return {
    defaultNamespace: engine === "postgres" ? "public" : "shop",
    children: vi.fn(async (path: string[]) => path.length === 0
      ? engine === "postgres" ? nodes([["public", "schema"], ["audit", "schema"]]) : nodes([["shop", "database"]])
      : path[0] === "public"
        ? nodes([["users", "table"], ["active_users", "view"]])
        : path[0] === "shop"
          ? nodes([["orders", "table"], ["customers", "table"]])
          : nodes([["events", "table"]])),
    tableInfo: vi.fn(async (_namespace: string, table: string) => ({
      columns: table === "users"
        ? [{ name: "id", data_type: "uuid", nullable: false, primary_key: true, default: null }, { name: "display name", data_type: "text", nullable: false, primary_key: false, default: null }]
        : [],
      primary_key: table === "users" ? ["id"] : [],
    })),
  };
}

async function labels(text: string, at = text.length, engine: "postgres" | "mysql" = "postgres") {
  const metadata = provider(engine);
  const state = EditorState.create({ doc: text, extensions: sql({ dialect: sqlDialect(engine) }) });
  const result = await sqlMetadataCompletion(engine, metadata)(new CompletionContext(state, at, true));
  return { labels: result?.options.map((o) => o.label) ?? [], metadata };
}

describe("SQL dialects and relation discovery", () => {
  it("selects distinct PostgreSQL and MySQL dialects", () => {
    expect(sqlDialect("postgres")).not.toBe(sqlDialect("mysql"));
    expect(sqlDialect("unknown")).toBeDefined();
  });

  it("finds qualified and quoted relations but ignores strings and comments", () => {
    const text = `SELECT * FROM public.users u
      JOIN "audit"."event log" e ON e.user_id = u.id
      WHERE u.note = 'JOIN secret.tokens' -- FROM hidden.table
      /* JOIN nope.rows */`;
    expect(referencedRelations(text, "postgres")).toEqual([
      { namespace: "public", table: "users" },
      { namespace: "audit", table: "event log" },
    ]);
  });

  it("keeps PostgreSQL dollar-quoted bodies out of metadata discovery", () => {
    expect(executableSql("SELECT $$ FROM secret.tokens $$; SELECT * FROM public.users", "postgres")).not.toContain("secret.tokens");
    expect(referencedRelations("SELECT $$ FROM secret.tokens $$; SELECT * FROM public.users", "postgres")).toEqual([{ namespace: "public", table: "users" }]);
  });
});

describe("schema completion", () => {
  it("offers tables from the default schema", async () => {
    const result = await labels("SELECT * FROM us");
    expect(result.labels).toContain("users");
  });

  it("offers MySQL tables from the selected database", async () => {
    const result = await labels("SELECT * FROM ord", undefined, "mysql");
    expect(result.labels).toContain("orders");
    expect(result.labels).toContain("customers");
    expect(result.metadata.children).toHaveBeenCalledWith(["shop"]);
  });

  it("loads columns and resolves a table alias", async () => {
    const result = await labels("SELECT u. FROM public.users AS u", "SELECT u.".length);
    expect(result.labels).toContain("id");
    expect(result.labels).toContain("display name");
    expect(result.metadata.tableInfo).toHaveBeenCalledWith("public", "users");
  });

  it("does not offer schema data inside a string or comment", async () => {
    const quoted = await labels("SELECT 'public.us'", "SELECT 'public.us".length);
    const commented = await labels("SELECT 1 -- public.us");
    expect(quoted.labels).toEqual([]);
    expect(commented.labels).toEqual([]);
  });

  it("does not request metadata for unsupported consoles", async () => {
    const metadata = provider();
    const state = EditorState.create({ doc: "GET user", extensions: sql() });
    const result = await sqlMetadataCompletion("redis", metadata)(new CompletionContext(state, state.doc.length, true));
    expect(result).toBeNull();
    expect(metadata.children).not.toHaveBeenCalled();
  });
});
