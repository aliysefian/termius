"""The Databases view with every engine (feat.md D3 to D8) in a real (headless) browser with the backend mocked: the
connection form's fields per engine, the tree and what opening an item runs, the console wording, the destructive
prompt for commands that aren't SQL, and an inline edit's confirmation.

    pnpm dev &
    python3 src/lib/__tests__/e2e/databases.py [chromium-executable]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__calls = [];
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data: Object.assign({ host: 'h.example', port: 1, username: '', database: '', tls: 'disable', group: '', notes: '' }, data) });
window.__conns = [
  rec('c-redis', { name: 'cache', engine: 'redis', port: 6379 }),
  rec('c-mongo', { name: 'docs', engine: 'mongodb', port: 27017, database: 'shop' }),
  rec('c-es', { name: 'search', engine: 'elasticsearch', port: 9200 }),
  rec('c-mssql', { name: 'erp', engine: 'mssql', port: 1433, username: 'sa' }),
  rec('c-rqlite', { name: 'edge', engine: 'rqlite', port: 4001, options: { level: 'strong' } }),
  rec('c-prod', { name: 'main-store', engine: 'redis', port: 6379, environment: 'production' }),
];
const sessions = {};
const grid = (cols, rows, extra) => Object.assign({ columns: cols.map((c) => ({ name: c, data_type: 'text', kind: 'text' })), rows, truncated: false, affected_rows: null, last_insert_id: null, elapsed_ms: 3 }, extra || {});
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts' || cmd === 'list_identities') return [];
  if (cmd === 'list_db_connections') return window.__conns;
  if (cmd === 'save_db_connection') { window.__calls.push({ cmd, connection: args.connection }); return rec('new', args.connection); }
  if (cmd === 'db_open') {
    window.__calls.push({ cmd, id: args.id });
    const c = window.__conns.find((x) => x.id === args.id).data;
    const sid = 's-' + args.id; sessions[sid] = c.engine;
    return { session_id: sid, server_version: { redis: 'Redis 7.2.4', mongodb: 'MongoDB 7.0.5', elasticsearch: 'Elasticsearch 8.11.1', mssql: 'Microsoft SQL Server 2022', rqlite: 'rqlite 8.36.1' }[c.engine] };
  }
  const engine = sessions[args.sessionId];
  if (cmd === 'db_children') {
    const p = args.path;
    if (engine === 'redis') {
      if (p.length === 0) return [{ name: 'db0', kind: 'database', detail: '3 keys', expandable: true }, { name: 'db5', kind: 'database', detail: '1 keys', expandable: true }];
      if (p.length === 1) return [{ name: 'user:', kind: 'folder', detail: '2', expandable: true }, { name: 'my key', kind: 'table', detail: 'string · expires in 59 min', expandable: false }];
      return [{ name: 'user:1', kind: 'table', detail: 'hash', expandable: false }];
    }
    if (engine === 'mongodb') {
      if (p.length === 0) return [{ name: 'shop', kind: 'database', expandable: true }];
      if (p.length === 1) return [{ name: 'users', kind: 'table', expandable: true }, { name: 'recent', kind: 'view', expandable: true }];
      return [{ name: 'email_1', kind: 'index', detail: 'email: 1 · unique', expandable: false }];
    }
    if (engine === 'elasticsearch') {
      if (p.length === 0) return [{ name: 'prod-cluster', kind: 'database', expandable: true }];
      if (p.length === 1) return [{ name: 'books', kind: 'table', detail: '3 docs · 2.0 KB · green', expandable: true }];
      return [{ name: 'title', kind: 'column', detail: 'text', expandable: false }];
    }
    if (p.length === 0) return [{ name: engine === 'rqlite' ? 'main' : 'dbo', kind: 'schema', expandable: true }];
    if (p.length === 1) return [{ name: 'items', kind: 'table', expandable: true }];
    return [];
  }
  if (cmd === 'db_query') {
    window.__calls.push({ cmd, sql: args.sql, confirmed: args.confirmed, limit: args.limit });
    if (/drop|delete|flush/i.test(args.sql) && !args.confirmed) throw { code: 'needs_confirmation', message: 'This drops the collection and everything in it.' };
    if (engine === 'redis') return grid(['field', 'value'], [['name', 'Ada'], ['age', '36']]);
    if (engine === 'mongodb') return grid(['_id', 'name', 'age'], [['65f1a2b3c4d5e6f708192a3b', 'Ada', 36], ['65f1a2b3c4d5e6f708192a3c', 'Alan', 41]]);
    if (engine === 'elasticsearch') return grid(['_id', 'title'], [['1', 'Dune']]);
    return grid(['id', 'name'], [[1, 'a']]);
  }
  if (cmd === 'db_table_info') {
    window.__calls.push({ cmd, database: args.database, table: args.table });
    if (engine === 'mongodb') return { columns: [{ name: '_id', data_type: 'objectid', nullable: false, primary_key: true, default: null }, { name: 'name', data_type: 'string', nullable: true, primary_key: false, default: null }, { name: 'age', data_type: 'number', nullable: true, primary_key: false, default: null }], primary_key: ['_id'] };
    if (engine === 'redis') return { columns: [{ name: 'field', data_type: 'string', nullable: false, primary_key: true, default: null }, { name: 'value', data_type: 'string', nullable: false, primary_key: false, default: null }], primary_key: ['field'] };
    return { columns: [], primary_key: [] };
  }
  if (cmd === 'db_preview_update') { window.__calls.push({ cmd, edit: args.edit }); return engine === 'mongodb' ? 'db.getSiblingDB("shop").users.updateOne({"_id": …}, {"$set": {"name": "Grace"}})' : 'HSET user:1 name Grace'; }
  if (cmd === 'db_apply_update') { window.__calls.push({ cmd, edit: args.edit }); return 1; }
  if (cmd === 'db_close') return null;
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b):
    pg = await b.new_page(viewport={"width": 1500, "height": 1000})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    pg.on("console", lambda m: print("CONSOLE", m.text[:200]) if m.type == "error" else None)
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(MOCK)
    await pg.goto(URL)
    await pg.evaluate("() => localStorage.clear()")
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    await pg.click('button[aria-label="Databases"]')
    await pg.wait_for_timeout(500)
    return pg


async def connect(pg, name):
    await pg.locator("button[title=Connect]", has_text=name).click()
    await pg.wait_for_timeout(500)


async def toggle(pg, text):
    await pg.locator("aside button", has_text=text).first.click()
    await pg.wait_for_timeout(300)


async def open_node(pg, text):
    await pg.locator("aside button", has_text=text).first.dblclick()
    await pg.wait_for_timeout(700)


async def editor(pg):
    return await pg.locator("textarea[aria-label]").first.input_value()


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- the form ----
        pg = await fresh(b)
        await pg.get_by_role("button", name="New connection").first.click() if await pg.get_by_role("button", name="New connection").count() else await pg.locator("button[title*=connection i]").first.click()
        await pg.wait_for_timeout(500)
        options = await pg.locator("#db-engine option").all_inner_texts()
        check("the type list has every engine", options == ["MySQL / MariaDB", "PostgreSQL", "SQL Server", "Oracle Database", "rqlite", "Redis", "MongoDB", "Elasticsearch / OpenSearch"], options)
        await pg.select_option("#db-engine", "redis")
        check("choosing Redis sets its port and asks for a database number", await pg.input_value("#db-port") == "6379" and "database number" in (await pg.locator("label[for=db-db]").inner_text()).lower())
        check("and says what to know about it", "SCAN" in await pg.locator("[data-testid=db-engine-note]").inner_text())
        await pg.select_option("#db-engine", "oracle")
        check("Oracle asks for a service name, a user, and has the SID setting", await pg.input_value("#db-port") == "1521" and "service name" in (await pg.locator("label[for=db-db]").inner_text()).lower() and await pg.locator("[data-testid=db-option-sid]").count() == 1 and await pg.locator("#db-user").get_attribute("required") is not None)
        await pg.select_option("#db-engine", "rqlite")
        check("rqlite has no database field and a read consistency choice", await pg.locator("#db-db").count() == 0 and await pg.locator("#db-opt-level").count() == 1)
        await pg.select_option("#db-opt-level", "linearizable")
        await pg.select_option("#db-engine", "mongodb")
        check("changing engine drops the last engine's settings and shows Mongo's", await pg.locator("#db-opt-level").count() == 0 and await pg.locator("[data-testid=db-option-uri]").count() == 1)
        await pg.select_option("#db-engine", "elasticsearch")
        es = (await pg.locator("#db-opt-auth").count(), await pg.locator("label[for=db-pass]").inner_text())
        check("Elasticsearch has the sign-in choice and a password that can be a key", es[0] == 1 and "api key" in es[1].lower(), es)
        await pg.select_option("#db-engine", "rqlite")
        await pg.fill("#db-name", "edge-2")
        await pg.fill("#db-host", "10.0.0.5")
        await pg.select_option("#db-opt-level", "strong")
        await pg.get_by_role("button", name="Save").click()
        await pg.wait_for_timeout(500)
        saved = [c for c in await pg.evaluate("window.__calls") if c["cmd"] == "save_db_connection"]
        check("saving keeps the engine's settings, and only those", saved and saved[0]["connection"]["engine"] == "rqlite" and saved[0]["connection"].get("options") == {"level": "strong"} and saved[0]["connection"]["database"] == "", saved)
        check("no page errors from the form", not pg.errors, pg.errors)
        await pg.close()

        # ---- Redis ----
        pg = await fresh(b)
        await connect(pg, "cache")
        check("Redis shows its databases with their key counts", "db0" in await pg.locator("aside").inner_text() and "3 keys" in await pg.locator("aside").inner_text())
        await toggle(pg, "db0")
        await toggle(pg, "user:")
        names = await pg.locator("aside").inner_text()
        check("keys are grouped into folders, with their type and expiry", "user:1" in names and "string · expires in 59 min" in names)
        await open_node(pg, "my key")
        check("opening a key runs VIEW in its database", (await editor(pg)) == '@0 VIEW "my key"', await editor(pg))
        calls = await pg.evaluate("window.__calls")
        check("and asks which database and key it is", any(c["cmd"] == "db_table_info" and c["database"] == "db0" and c["table"] == "my key" for c in calls), calls)
        check("the console says what it takes", "Redis commands" in await pg.locator("textarea[aria-label]").first.get_attribute("aria-label"))
        await pg.locator("textarea[aria-label]").first.fill("FLUSHALL")
        await pg.keyboard.press("Control+Enter")
        await pg.wait_for_timeout(500)
        dialog = pg.locator("[role=dialog], [role=alertdialog]").last
        check("a command that deletes asks first, with the engine's own reason", "drops the collection" in await dialog.inner_text() or "FLUSHALL" in await dialog.inner_text())
        await dialog.get_by_role("button", name="Cancel").click()
        await pg.close()

        # ---- MongoDB: opening, an edit's wording, a destructive command ----
        pg = await fresh(b)
        await connect(pg, "docs")
        await toggle(pg, "shop")
        await open_node(pg, "users")
        text = await editor(pg)
        check("opening a collection writes its find", json.loads(text) == {"$db": "shop", "find": "users", "filter": {}}, text)
        await toggle(pg, "users")
        check("a collection lists its indexes", "email: 1 · unique" in await pg.locator("aside").inner_text())
        await pg.locator("[role=gridcell], td").filter(has_text="Ada").first.dblclick()
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Control+a")
        await pg.keyboard.type("Grace")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(500)
        dialog = pg.locator("[role=dialog], [role=alertdialog]").last
        shown = await dialog.inner_text()
        check("an edit says it is a change to a document, not an UPDATE", "Run this change?" in shown and "updateOne" in shown and "document" in shown, shown)
        await dialog.get_by_role("button", name="Run change").click()
        await pg.wait_for_timeout(400)
        applied = [c for c in await pg.evaluate("window.__calls") if c["cmd"] == "db_apply_update"]
        check("approving it applies the edit by _id", applied and applied[0]["edit"]["key"][0]["column"] == "_id" and applied[0]["edit"]["changes"][0] == {"column": "name", "value": "Grace"}, applied)
        await pg.locator("textarea[aria-label]").first.fill('{"drop": "users"}')
        await pg.keyboard.press("Control+Enter")
        await pg.wait_for_timeout(500)
        dialog = pg.locator("[role=dialog], [role=alertdialog]").last
        check("a drop asks, with the server-side reason", "drops the collection" in await dialog.inner_text())
        await dialog.get_by_role("button", name="Run it").click()
        await pg.wait_for_timeout(400)
        queries = [c for c in await pg.evaluate("window.__calls") if c["cmd"] == "db_query" and "drop" in c["sql"]]
        check("and runs it only once confirmed", [q["confirmed"] for q in queries] == [False, True], queries)
        check("no page errors from MongoDB", not pg.errors, pg.errors)
        await pg.close()

        # ---- Elasticsearch, SQL Server, rqlite ----
        pg = await fresh(b)
        await connect(pg, "search")
        await toggle(pg, "prod-cluster")
        check("an index shows its documents and size", "3 docs · 2.0 KB · green" in await pg.locator("aside").inner_text())
        await open_node(pg, "books")
        text = await editor(pg)
        check("opening an index writes its search", text.startswith("GET /books/_search\n") and "match_all" in text, text)
        await connect(pg, "erp")
        await toggle(pg, "dbo")
        await open_node(pg, "items")
        check("SQL Server opens a table with its own quoting", (await editor(pg)) == "SELECT * FROM [dbo].[items]", await editor(pg))
        await connect(pg, "edge")
        await toggle(pg, "main")
        await open_node(pg, "items")
        check("rqlite opens a table with SQLite quoting", (await editor(pg)) == 'SELECT * FROM "main"."items"', await editor(pg))
        check("no page errors from the other engines", not pg.errors, pg.errors)
        await pg.screenshot(path="/tmp/databases-engines.png")
        await pg.close()

        await b.close()
    print(f"\n{len(failures)} failed" if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
