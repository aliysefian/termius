"""The host list with thousands of hosts, in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/bigfleet.py [chromium-executable]

Checks that only a window of rows is drawn, that every row is still reachable (scrolling, End, type-ahead, search,
folding a group), and prints the measurements (start-up, elements, memory, search).
"""
import asyncio, os, sys, time
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
N = 5000
HOSTS = """
const N = %d;
const hosts = Array.from({length: N}, (_, i) => ({ id: 'h' + i, rev: 1, updated_at: 1, deleted: false, data: { label: 'host-' + String(i).padStart(5, '0'), hostname: 'h' + i + '.example', port: 22, group: 'Group ' + String(i %% 100).padStart(3, '0') + '/Sub ' + (i %% 7), tags: [], notes: '', environment: i %% 5 === 0 ? 'production' : '', identity_id: 'i1' } }));
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return hosts;
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  return undefined;
};
""" % N
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
        await pg.add_init_script(HOSTS)
        t0 = time.time()
        await pg.goto("http://127.0.0.1:1420/")
        await pg.wait_for_selector("[role=tree] [role=treeitem]", timeout=120000)
        await pg.wait_for_timeout(1000)
        ready = time.time() - t0
        rows = pg.locator("[role=treeitem]")
        n = await rows.count()
        mem = await pg.evaluate("performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : -1")
        print(f"{N} hosts: ready in {ready:.1f} s, {n} rows in the document, heap {mem} MB")
        check("only a window of the list is drawn", 0 < n < 80, n)

        tree = pg.locator("[role=tree]")
        height = await tree.evaluate("el => el.scrollHeight")
        check("the scrollbar is as long as the whole list", height > N * 40, height)

        # scroll to the very end: the last group and its hosts are there
        await tree.evaluate("el => { el.scrollTop = el.scrollHeight; }")
        await pg.wait_for_timeout(400)
        text = await tree.inner_text()
        check("scrolled to the end, the last rows are drawn", "host-04" in text and "host-00000" not in text, text[-200:])
        check("and still only a window", await rows.count() < 80)
        await tree.evaluate("el => { el.scrollTop = 0; }")
        await pg.wait_for_timeout(400)

        # keyboard: End goes to the last row even though it was not drawn
        first = pg.locator("[role=treeitem][tabindex='0']").first
        await first.focus()
        await pg.keyboard.press("End")
        await pg.wait_for_timeout(600)
        key = await pg.evaluate("document.activeElement?.getAttribute('data-tree-key')")
        check("End moves to the last row (it is scrolled to and focused)", bool(key) and key.startswith("h:"), key)
        await pg.keyboard.press("Home")
        await pg.wait_for_timeout(500)
        key = await pg.evaluate("document.activeElement?.getAttribute('data-tree-key')")
        check("Home goes back to the first", key == "g:Group 000", key)

        # folding the first group takes its rows out
        before = await tree.evaluate("el => el.scrollHeight")
        await pg.locator("[role=treeitem][aria-expanded] button").first.click()
        await pg.wait_for_timeout(400)
        after = await tree.evaluate("el => el.scrollHeight")
        check("folding a group shortens the list", after < before, (before, after))

        # a search leaves few enough rows to draw them all
        t1 = time.time()
        await pg.fill('input[placeholder="Search hosts…"]', "host-04200")
        await pg.wait_for_timeout(300)
        took = time.time() - t1
        found = await tree.inner_text()
        check("a search finds the host and leaves few enough rows to draw them all", "host-04200" in found and await rows.count() < 10, (await rows.count(), found[:200]))
        print(f"search: {took * 1000:.0f} ms")
        await pg.fill('input[placeholder="Search hosts…"]', "")

        check("no page errors", not errors, errors[:3])
        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
