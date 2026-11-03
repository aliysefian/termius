import { describe, expect, it } from "vitest";
import { menuFor, type MenuContext } from "../dbactions";

const ctx: MenuContext = { target: "table", connected: true, sql: true, transfer: true, changes: true, readOnly: false };
const ids = (c: MenuContext) => menuFor(c).map((i) => i.id);
const item = (c: MenuContext, id: string) => menuFor(c).find((i) => i.id === id);

describe("menuFor", () => {
  it("offers a table everything its connection can do", () => {
    expect(ids(ctx)).toEqual(["open-data", "open-structure", "open-ddl", "select-query", "copy-name", "copy-qualified", "refresh", "export", "import"]);
    expect(menuFor(ctx).every((i) => i.enabled)).toBe(true);
  });

  it("greys out, with a reason, what the engine can't do", () => {
    const redis = { ...ctx, sql: false, transfer: false, changes: false };
    expect(item(redis, "open-structure")).toMatchObject({ enabled: false });
    expect(item(redis, "export")?.why).toMatch(/PostgreSQL and MySQL/);
    expect(item(redis, "import")?.enabled).toBe(false);
    expect(item(redis, "open-data")?.enabled).toBe(true);
  });

  it("does not let a read-only connection import", () => {
    expect(item({ ...ctx, readOnly: true }, "import")).toMatchObject({ enabled: false, why: "This connection is read-only." });
  });

  it("offers a view the same but no import", () => {
    expect(ids({ ...ctx, target: "view" })).not.toContain("import");
    expect(ids({ ...ctx, target: "view" })).toContain("export");
  });

  it("offers a connection to connect, or when connected to disconnect, query and refresh", () => {
    expect(ids({ ...ctx, target: "connection", connected: false })).toEqual(["connect", "new-query", "refresh", "copy-name", "edit", "delete"]);
    expect(item({ ...ctx, target: "connection", connected: false }, "new-query")?.enabled).toBe(false);
    expect(ids({ ...ctx, target: "connection" })[0]).toBe("disconnect");
    expect(item({ ...ctx, target: "connection" }, "delete")?.danger).toBe(true);
  });

  it("offers containers a new query and a refresh, and leaves other objects to copy", () => {
    expect(ids({ ...ctx, target: "schema" })).toEqual(["new-query", "refresh", "copy-name"]);
    expect(ids({ ...ctx, target: "function" })).toEqual(["copy-name", "copy-qualified"]);
  });
});
