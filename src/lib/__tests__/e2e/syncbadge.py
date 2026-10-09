"""The sync badge in the status bar, in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/syncbadge.py [chromium-executable]

With a folder that went backwards and a conflict waiting, the bar says so and the badge opens the Vault screen; with no
problems it says nothing (and never "in sync": the app does not do the syncing).
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
URL = "http://127.0.0.1:1420/"
MOCK = """
window.__health = %s;
window.__invoke = async (cmd) => {
  if (cmd === 'vault_health') return window.__health;
  return undefined;
};
"""
failures = []

def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok: failures.append(name)

async def page(b, health):
    pg = await b.new_page(viewport={"width": 1400, "height": 900})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(MOCK % health)
    await pg.add_init_script("window.__units = 1;")
    await pg.goto(URL); await pg.wait_for_timeout(3000)
    return pg

async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await page(b, "{ open_conflicts: 2, rollbacks: 3 }")
        badge = pg.locator("[data-testid=sync-badge]")
        check("a folder that went backwards shows in the status bar", await badge.count() == 1 and "went backwards: 3 records" in await badge.inner_text(), await badge.inner_text() if await badge.count() else "no badge")
        check("and it is the serious one, not a plain warning", "text-danger" in (await badge.get_attribute("class") or ""))
        await badge.click(); await pg.wait_for_timeout(600)
        check("the badge opens the Vault screen", "Vault" in await pg.inner_text("body") and await pg.locator("[data-testid=sync-badge]").count() == 1)
        check("no page errors", pg.errors == [], pg.errors)
        await pg.close()

        pg = await page(b, "{ open_conflicts: 1, rollbacks: 0 }")
        t = await pg.locator("[data-testid=sync-badge]").inner_text() if await pg.locator("[data-testid=sync-badge]").count() else ""
        check("a conflict alone is a warning with its count", t.strip() == "1 sync conflict", t)
        await pg.close()

        pg = await page(b, "{ open_conflicts: 0, rollbacks: 0 }")
        check("with nothing wrong the bar says nothing about sync", await pg.locator("[data-testid=sync-badge]").count() == 0 and "in sync" not in (await pg.inner_text("footer")).lower())
        await pg.close()
        await b.close()
    if failures:
        print("FAILED:", failures); sys.exit(1)
    print("all passed")

asyncio.run(main())
