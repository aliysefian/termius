"""The "closed unexpectedly" notice and the Diagnostics section, in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/crashlog.py [chromium-executable]

The mock backend reports one crash. Checks that it is announced once (not again after a reload), that Settings shows it with
its place in the code, and that Clear empties the list.
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
URL = "http://127.0.0.1:1420/"
DATA = """
window.__crash = [{ at: 1791500000, text: "v0.27.0 panic in thread 'main' at src/monitor.rs:76: there is no reactor running" }];
window.__invoke = async (cmd) => {
  if (cmd === 'crash_log') return { path: '/home/me/.config/sshvault/crash.log', entries: window.__crash };
  if (cmd === 'clear_crash_log') { window.__crash = []; return null; }
  return undefined;
};
"""

async def seen_within(pg, text, ms):
    """Whether `text` shows up on the page at any moment in the next `ms` (a notice fades after a few seconds)."""
    for _ in range(ms // 150):
        if text in await pg.inner_text("body"):
            return True
        await pg.wait_for_timeout(150)
    return False

async def main():
    failures = []
    def expect(what, got, want):
        ok = got == want
        print(f"{'ok  ' if ok else 'FAIL'} {what}: {got!r}" + ("" if ok else f" (wanted {want!r})"))
        if not ok: failures.append(what)
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        ctx = await b.new_context(viewport={"width": 1400, "height": 900})
        pg = await ctx.new_page()
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)[:160]))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(DATA)
        await pg.add_init_script("window.__units = 1;")
        await pg.goto(URL)
        expect("announced on start", await seen_within(pg, "closed unexpectedly on", 5000), True)
        await pg.reload()
        expect("not announced again after a reload", await seen_within(pg, "closed unexpectedly on", 4000), False)
        await pg.click('button[aria-label="Settings"]'); await pg.wait_for_timeout(500)
        await pg.fill('input[placeholder*="Search"]', "diagnostics"); await pg.wait_for_timeout(400)
        body = await pg.inner_text("body")
        expect("Diagnostics shows the place in the code", "src/monitor.rs:76" in body, True)
        expect("and where the file is", "crash.log" in body, True)
        await pg.locator('button:has-text("Clear")').first.click(); await pg.wait_for_timeout(400)
        expect("Clear empties the list", "Nothing recorded" in await pg.inner_text("body"), True)
        expect("page errors", errs, [])
        await b.close()
    if failures:
        print("FAILED:", failures); sys.exit(1)

asyncio.run(main())
