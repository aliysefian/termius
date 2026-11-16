"""The "Close tab when session ends" setting, in a real (headless) browser with the backend mocked: it is found by
search, off by default, and can be switched on.

    pnpm dev &
    python3 src/lib/__tests__/e2e/sessionend.py [chromium-executable] [url]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
URL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:1420/"
failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 900})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)

        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_timeout(400)
        await pg.fill('input[placeholder="Search…"]', "close tab")
        await pg.wait_for_timeout(600)
        row = pg.locator("label", has_text="Close tab when session ends")
        check("searching 'close tab' finds the setting", await row.count() == 1)
        box = row.locator('input[type="checkbox"]')
        check("it is off by default", not await box.is_checked())
        await box.check()
        check("it can be switched on", await box.is_checked())
        check("the auto-reconnect setting is untouched", await pg.locator("label", has_text="Reconnect automatically").locator('input[type="checkbox"]').is_checked())
        check("no page errors", not errors, "; ".join(errors))
        await b.close()


asyncio.run(main())
sys.exit(1 if failures else 0)
