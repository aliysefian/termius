import { describe, expect, it } from "vitest";
import { DB_ENGINES, ENGINE_LIST, browseText, cleanOptions, engineInfo, isSql, quoteIdent, quoteName, redisArg } from "../dbengines";

describe("the engine catalogue", () => {
  it("lists every engine the backend takes, each once, with its usual port", () => {
    expect(ENGINE_LIST.map((e) => e.value)).toEqual(["mysql", "postgres", "mssql", "oracle", "rqlite", "redis", "mongodb", "elasticsearch"]);
    expect(Object.fromEntries(DB_ENGINES.map((e) => [e.value, e.port]))).toEqual({ mysql: 3306, postgres: 5432, mssql: 1433, oracle: 1521, rqlite: 4001, redis: 6379, mongodb: 27017, elasticsearch: 9200 });
    expect(new Set(ENGINE_LIST.map((e) => e.label)).size).toBe(ENGINE_LIST.length);
  });

  it("tells the SQL engines from the others", () => {
    for (const e of ["mysql", "postgres", "mssql", "oracle", "rqlite"]) expect(isSql(e), e).toBe(true);
    for (const e of ["redis", "mongodb", "elasticsearch"]) expect(isSql(e), e).toBe(false);
    expect(engineInfo("nope").value).toBe("mysql");
    expect(engineInfo("redis").change).toBe("change");
    expect(engineInfo("mssql").change).toBe("UPDATE");
  });

  it("every option has a key, a label, and a choice list that has its first value as the default", () => {
    for (const e of ENGINE_LIST) {
      const keys = e.options.map((o) => o.key);
      expect(new Set(keys).size, e.value).toBe(keys.length);
      for (const o of e.options) {
        expect(o.label.length, `${e.value}.${o.key}`).toBeGreaterThan(2);
        if (o.choices) expect(o.choices.length).toBeGreaterThan(1);
      }
    }
    expect(engineInfo("rqlite").options[0].choices?.[0].value).toBe("weak");
    expect(engineInfo("elasticsearch").options[0].choices?.[0].value).toBe("basic");
  });

  it("the form asks only for what an engine has", () => {
    expect(engineInfo("rqlite").database).toBeNull();
    expect(engineInfo("elasticsearch").database).toBeNull();
    expect(engineInfo("oracle").database?.label).toBe("Service name");
    expect(engineInfo("redis").database?.label).toBe("Database number");
    expect(engineInfo("mssql").user?.required).toBe(true);
    expect(engineInfo("oracle").user?.required).toBe(true);
    expect(engineInfo("redis").user?.required).toBeFalsy();
  });
});

describe("names", () => {
  it("are quoted the way each engine reads them", () => {
    expect(quoteName("mysql", "shop", "items")).toBe("`shop`.`items`");
    expect(quoteName("postgres", "public", "items")).toBe('"public"."items"');
    expect(quoteName("mssql", "dbo", "order items")).toBe("[dbo].[order items]");
    expect(quoteIdent("mssql", "a]b")).toBe("[a]]b]");
    expect(quoteName("oracle", "HR", "EMPLOYEES")).toBe('"HR"."EMPLOYEES"');
    expect(quoteIdent("oracle", 'a"b')).toBe('"a""b"');
    expect(quoteName("rqlite", "main", "it's")).toBe('"main"."it\'s"');
    expect(quoteIdent("", "x")).toBe("`x`");
  });

  it("a Redis argument is bare when plain and quoted with escapes when not", () => {
    expect(redisArg("user:42:name")).toBe("user:42:name");
    expect(redisArg("my key")).toBe('"my key"');
    expect(redisArg('say "hi"')).toBe('"say \\"hi\\""');
    expect(redisArg("a\nb")).toBe('"a\\nb"');
    expect(redisArg("")).toBe('""');
    expect(redisArg("back\\slash")).toBe('"back\\\\slash"');
    expect(redisArg("\x01")).toBe('"\\x01"');
  });
});

describe("what opening an item runs", () => {
  it("is a SELECT for the SQL engines, with the right quoting", () => {
    expect(browseText("mysql", "shop", "items")).toBe("SELECT * FROM `shop`.`items`");
    expect(browseText("mssql", "dbo", "items")).toBe("SELECT * FROM [dbo].[items]");
    expect(browseText("oracle", "HR", "EMP")).toBe('SELECT * FROM "HR"."EMP"');
    expect(browseText("rqlite", "main", "t")).toBe('SELECT * FROM "main"."t"');
  });

  it("is VIEW in the key's database for Redis", () => {
    expect(browseText("redis", "db3", "user:1")).toBe("@3 VIEW user:1");
    expect(browseText("redis", "db0", "my key")).toBe('@0 VIEW "my key"');
    expect(browseText("redis", "weird", "k")).toBe("VIEW k");
  });

  it("is a find command for MongoDB, with the names as JSON text", () => {
    expect(browseText("mongodb", "shop", "users")).toBe('{"$db": "shop", "find": "users", "filter": {}}');
    const odd = browseText("mongodb", 'sh"op', 'a\\b');
    expect(JSON.parse(odd)).toEqual({ $db: 'sh"op', find: "a\\b", filter: {} });
  });

  it("is a search request for Elasticsearch, with the index in one path segment", () => {
    const t = browseText("elasticsearch", "cluster", "my index/2024");
    expect(t.split("\n")[0]).toBe("GET /my%20index%2F2024/_search");
    expect(JSON.parse(t.split("\n").slice(1).join("\n"))).toEqual({ query: { match_all: {} } });
  });
});

describe("the saved settings", () => {
  it("keep only what the engine knows and what isn't blank", () => {
    expect(cleanOptions("oracle", { sid: " XE ", ca_file: "", junk: "x" })).toEqual({ sid: "XE" });
    expect(cleanOptions("mysql", { sid: "XE" })).toBeUndefined();
    expect(cleanOptions("rqlite", { level: "strong" })).toEqual({ level: "strong" });
    expect(cleanOptions("redis", {})).toBeUndefined();
  });
});
