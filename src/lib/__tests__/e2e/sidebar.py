"""The Manage menu on a short window, and the Fleet sidebar panel, in a headless browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/sidebar.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
HOSTS = """
const mk = (i, label, group, host) => ({ id: 'h' + i, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: host, port: 22, group, tags: [], notes: '', environment: '', identity_id: 'i1' } });
const hosts = [mk(1, 'web-1', 'Prod/web', 'w1.example'), mk(2, 'web-2', 'Prod/web', 'w2.example'), mk(3, 'db-1', 'Prod/db', '10.0.0.5'), mk(4, 'laptop', '', 'lap.example')];
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return hosts;
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  return undefined;
};
"""
failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1200, "height": 900})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(HOSTS)
        await pg.goto("http://127.0.0.1:1420/")
        await pg.wait_for_selector("[role=tree] [role=treeitem]", timeout=60000)

        # Manage menu on a short window: inside the window, and scrolls when it can't all fit.
        for h in (900, 420, 300):
            await pg.set_viewport_size({"width": 1200, "height": h})
            await pg.get_by_role("button", name="Manage").click()
            menu = pg.locator("[role=menu]")
            await menu.wait_for()
            box = await menu.bounding_box()
            fits = box["y"] >= 0 and box["y"] + box["height"] <= h + 1
            check(f"Manage menu stays inside a {h}px window", fits, box)
            sh = await menu.evaluate("e => [e.scrollHeight, e.clientHeight]")
            if h == 300:
                check("and scrolls when it is taller than the window", sh[0] > sh[1], sh)
                await menu.evaluate("e => e.scrollTop = e.scrollHeight")
                last = menu.locator("button").last
                lb = await last.bounding_box()
                check("so the last entry can be reached", lb["y"] + lb["height"] <= h + 1, lb)
            await pg.keyboard.press("Escape")

        # The Fleet panel.
        await pg.set_viewport_size({"width": 1200, "height": 800})
        await pg.get_by_role("button", name="Fleet").first.click()
        panel = pg.locator("aside", has=pg.get_by_label("Search the fleet"))
        await panel.locator("section").first.wait_for()
        heads = panel.locator("section > button[aria-expanded]")
        names = await heads.locator("span.truncate").all_inner_texts()
        check("hosts are separated by group, ungrouped last", names == ["Prod/db", "Prod/web", "Ungrouped"], names)
        await pg.get_by_label("Search the fleet").fill("w2.example")
        n = await panel.locator("section").count()
        check("search narrows to the matching group", n == 1 and "web-2" in await panel.locator("section").inner_text(), n)
        await pg.get_by_label("Search the fleet").fill("")
        await heads.first.click()
        folded = await panel.locator("section").first.locator("button").count()
        check("a group folds", folded == 1, folded)
        check("no page errors", not errors, errors)
        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
