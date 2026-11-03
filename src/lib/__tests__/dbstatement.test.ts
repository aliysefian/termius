import { describe, expect, it } from "vitest";
import { isScript, splitStatements, statementAt } from "../dbstatement";

describe("splitStatements", () => {
  it("splits on semicolons and drops empty pieces", () => {
    const s = splitStatements("select 1;\n\nselect 2;;  \nselect 3");
    expect(s.map((x) => x.text)).toEqual(["select 1", "select 2", "select 3"]);
  });

  it("keeps a semicolon inside a string, a quoted name or a comment", () => {
    const s = splitStatements(`select ';' as a; select "x;y" from t; -- a; b\n select 3 /* ; */; select \`a;b\``, "mysql");
    expect(s).toHaveLength(4);
    expect(s[0].text).toBe("select ';' as a");
    expect(s[2].text).toContain("select 3");
  });

  it("treats a doubled quote as an escaped one", () => {
    expect(splitStatements("select 'it''s; fine'; select 2").map((x) => x.text)).toEqual(["select 'it''s; fine'", "select 2"]);
  });

  it("reads a backslash escape in MySQL but not in PostgreSQL", () => {
    expect(splitStatements("select 'a\\';b'; select 2", "mysql")).toHaveLength(2);
    // PostgreSQL's plain string ends at the second quote, so the `;` after it splits.
    expect(splitStatements("select 'a\\'; select 2", "postgres")).toHaveLength(2);
  });

  it("keeps a PostgreSQL dollar-quoted body whole", () => {
    const body = "create function f() returns int language plpgsql as $$ begin return 1; end; $$; select 2";
    const s = splitStatements(body, "postgres");
    expect(s.map((x) => x.text)).toEqual(["create function f() returns int language plpgsql as $$ begin return 1; end; $$", "select 2"]);
    const tagged = splitStatements("do $body$ begin perform 1; end $body$; select 1", "postgres");
    expect(tagged).toHaveLength(2);
  });

  it("does not read a dollar sign as a quote outside PostgreSQL", () => {
    expect(splitStatements("select '$$'; select 2; select 3", "mysql")).toHaveLength(3);
  });

  it("ignores a piece that is only comments", () => {
    expect(splitStatements("-- nothing\n/* here */; select 1").map((x) => x.text)).toEqual(["select 1"]);
  });

  it("gives each statement's position in the script", () => {
    const sql = "select 1;\n  select 2;";
    const s = splitStatements(sql);
    expect(sql.slice(s[1].from, s[1].to)).toBe("select 2;");
  });
});

describe("statementAt", () => {
  const sql = "select 1;\nselect 2;\n\nselect 3";
  it("finds the statement the cursor is in", () => {
    expect(statementAt(sql, 3)?.text).toBe("select 1");
    expect(statementAt(sql, 12)?.text).toBe("select 2");
    expect(statementAt(sql, sql.length)?.text).toBe("select 3");
  });
  it("prefers the statement before a cursor on blank space or right after a semicolon", () => {
    expect(statementAt(sql, 9)?.text).toBe("select 1");
    expect(statementAt(sql, 20)?.text).toBe("select 2");
  });
  it("is null for an empty script", () => {
    expect(statementAt("  \n-- hi", 2)).toBeNull();
  });
});

describe("isScript", () => {
  it("is true only for more than one statement", () => {
    expect(isScript("select 1;")).toBe(false);
    expect(isScript("select 1; select 2")).toBe(true);
  });
});
