import { describe, expect, it } from "vitest";
import { countSql, explainSql, filterProblem, isPageable, pageOf, pageSql, rangeText } from "../dbpaging";
import { canFormat, formatSql } from "../dbformat";

describe("pageSql", () => {
  const o = { limit: 200, offset: 400 };
  it("pages with LIMIT and OFFSET for PostgreSQL, MySQL and SQLite", () => {
    expect(pageSql("postgres", "public", "items", o)).toBe('SELECT * FROM "public"."items" LIMIT 200 OFFSET 400');
    expect(pageSql("mysql", "shop", "items", o)).toBe("SELECT * FROM `shop`.`items` LIMIT 200 OFFSET 400");
    expect(pageSql("rqlite", "main", "t", o)).toBe('SELECT * FROM "main"."t" LIMIT 200 OFFSET 400');
  });
  it("adds the filter and the order, quoting names", () => {
    expect(pageSql("postgres", "public", "Odd \"Name\"", { ...o, filter: " price > 3 ", order: [{ column: "price", desc: true }, { column: "id", desc: false }] })).toBe(
      'SELECT * FROM "public"."Odd ""Name""" WHERE price > 3 ORDER BY "price" DESC, "id" LIMIT 200 OFFSET 400',
    );
  });
  it("uses the standard form for SQL Server (which needs an order) and Oracle", () => {
    expect(pageSql("mssql", "dbo", "t", o)).toBe("SELECT * FROM [dbo].[t] ORDER BY (SELECT NULL) OFFSET 400 ROWS FETCH NEXT 200 ROWS ONLY");
    expect(pageSql("mssql", "dbo", "t", { ...o, order: [{ column: "id", desc: false }] })).toBe("SELECT * FROM [dbo].[t] ORDER BY [id] OFFSET 400 ROWS FETCH NEXT 200 ROWS ONLY");
    expect(pageSql("oracle", "HR", "T", o)).toBe('SELECT * FROM "HR"."T" OFFSET 400 ROWS FETCH NEXT 200 ROWS ONLY');
  });
  it("has nothing for engines that aren't SQL, and keeps limits sane", () => {
    expect(pageSql("redis", "db0", "k", o)).toBeNull();
    expect(isPageable("mongodb")).toBe(false);
    expect(pageSql("postgres", "p", "t", { limit: 0, offset: -5 })).toBe('SELECT * FROM "p"."t" LIMIT 1 OFFSET 0');
  });
});

describe("filterProblem", () => {
  it("accepts one condition, including one with a semicolon in a string", () => {
    expect(filterProblem("")).toBeNull();
    expect(filterProblem("name = 'a;b' AND id > 3")).toBeNull();
  });
  it("refuses a second statement and a leading WHERE", () => {
    expect(filterProblem("1=1; DROP TABLE items")).toMatch(/one condition/);
    expect(filterProblem("WHERE id = 1")).toMatch(/without the word WHERE/);
  });
});

describe("countSql and explainSql", () => {
  it("counts, with the filter", () => {
    expect(countSql("postgres", "public", "items")).toBe('SELECT COUNT(*) FROM "public"."items"');
    expect(countSql("mysql", "d", "t", "a = 1")).toBe("SELECT COUNT(*) FROM `d`.`t` WHERE a = 1");
    expect(countSql("redis", "d", "t")).toBeNull();
  });
  it("explains without running (no ANALYZE) where the engine allows one statement", () => {
    expect(explainSql("postgres", "select 1;")).toBe("EXPLAIN select 1");
    expect(explainSql("rqlite", "select 1")).toBe("EXPLAIN QUERY PLAN select 1");
    expect(explainSql("mssql", "select 1")).toBeNull();
    expect(explainSql("postgres", "  ")).toBeNull();
    expect(explainSql("postgres", "select 1")).not.toMatch(/ANALYZE/i);
  });
});

describe("paging text", () => {
  it("names the page and the range", () => {
    expect(pageOf(0, 200)).toBe(1);
    expect(pageOf(400, 200)).toBe(3);
    expect(rangeText(400, 200)).toBe("401–600");
    expect(rangeText(0, 0)).toBe("0");
  });
});

describe("formatSql", () => {
  it("formats in the engine's dialect and keeps the meaning", () => {
    const r = formatSql("postgres", "select a,b from t where x=1 and y='z' order by a");
    expect("text" in r && r.text).toContain("SELECT");
    expect("text" in r && r.text.replace(/\s+/g, " ")).toBe("SELECT a, b FROM t WHERE x = 1 AND y = 'z' ORDER BY a");
    expect(canFormat("mysql") && !canFormat("redis")).toBe(true);
  });
  it("says why it can't", () => {
    expect(formatSql("redis", "GET x")).toHaveProperty("error");
  });
});
