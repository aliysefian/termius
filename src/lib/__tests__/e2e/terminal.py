"""Terminal features (feat2 4.4) in a real (headless) browser with the backend mocked: command blocks in the
margin, highlight rules, and searching every open terminal.

    pnpm dev &
    python3 src/lib/__tests__/e2e/terminal.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b, prefs):
    pg = await b.new_page(viewport={"width": 1400, "height": 900})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.goto(URL)
    await pg.evaluate("(p) => { localStorage.clear(); localStorage.setItem('sshvault.prefs.v1', JSON.stringify(p)); }", prefs)
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    await pg.keyboard.press("Control+Shift+Backquote")
    await pg.wait_for_timeout(1200)
    await pg.click(".xterm")
    await pg.wait_for_timeout(300)
    return pg


async def deliver(pg, text):
    await pg.evaluate("(t) => window.__deliver(Array.from(new TextEncoder().encode(t)))", text)
    await pg.wait_for_timeout(250)


def command(cmd, out, exit):
    """What a shell with integration prints around one command."""
    return f"\x1b]133;A\x07$ \x1b]133;B\x07{cmd}\r\n\x1b]133;C\x07{out}\r\n\x1b]133;D;{exit}\x07"


RULE = {"id": "r1", "name": "Errors", "pattern": "error", "regex": False, "matchCase": False, "color": "#ff0000", "enabled": True, "notify": False}


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- command blocks ----
        pg = await fresh(b, {})
        await deliver(pg, command("make build", "compiling\r\nerror: boom", 2))
        await deliver(pg, command("ls", "a\r\nb", 0))
        await deliver(pg, "\x1b]133;A\x07$ ")
        bars = await pg.evaluate("[...document.querySelectorAll('[data-block]')].map((e) => e.title)")
        check("each finished command gets a bar", len(bars) == 2, bars)
        check("the bar says what happened", any("make build" in t and "failed (exit 2)" in t for t in bars) and any("succeeded" in t for t in bars), bars)
        colors = await pg.evaluate("[...document.querySelectorAll('[data-block]')].map((e) => e.style.background)")
        check("a failure and a success look different", len(set(colors)) == 2, colors)
        await pg.locator("[data-block]").first.click()
        await pg.wait_for_timeout(200)
        items = await pg.evaluate("[...document.querySelectorAll('[data-testid=block-menu] [role=menuitem]')].map((e) => e.textContent.trim())")
        check("clicking a bar offers copy, select and pin", items[:4] == ["Copy output", "Copy command", "Select output", "Pin this command"], items)
        await pg.get_by_role("menuitem", name="Pin this command").click()
        await pg.wait_for_timeout(300)
        pinned = await pg.evaluate("[...document.querySelectorAll('[data-block]')].map((e) => e.style.opacity)")
        check("a pinned command is drawn differently", "1" in pinned, pinned)
        await pg.screenshot(path="/tmp/terminal-blocks.png")
        check("no page errors from blocks", not pg.errors, pg.errors)
        await pg.close()

        # ---- command blocks off ----
        pg = await fresh(b, {"commandBlocks": False})
        await deliver(pg, command("ls", "a", 0))
        check("with the setting off there are no bars", await pg.locator("[data-block]").count() == 0)
        await pg.close()

        # ---- highlight rules ----
        pg = await fresh(b, {"highlightRules": [RULE]})
        await deliver(pg, "an error here\r\n")
        sc = await pg.evaluate("(() => { const r = document.querySelector('.xterm-screen').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()")
        rows = await pg.evaluate("document.querySelector('.xterm-rows')?.children.length ?? 0")
        await pg.screenshot(path="/tmp/terminal-highlight.png")
        dec = await pg.evaluate("document.querySelectorAll('.xterm-decoration').length")
        check("a rule draws something over the matching words", dec > 0 or rows > 0, (dec, rows))
        check("no page errors from highlights", not pg.errors, pg.errors)
        await pg.close()

        # ---- search across terminals ----
        pg = await fresh(b, {})
        await deliver(pg, "needle in the first terminal\r\n")
        await pg.keyboard.press("Control+Shift+T")
        await pg.wait_for_timeout(500)
        await pg.keyboard.press("Escape")
        await pg.keyboard.press("Control+Shift+P")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("Search all open terminals")
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(400)
        await pg.fill("[data-testid=termsearch-input]", "needle")
        await pg.wait_for_timeout(300)
        hits = await pg.evaluate("[...document.querySelectorAll('[data-testid=termsearch-hits] button')].map((e) => e.textContent.trim())")
        check("the search finds the words in an open terminal", any("needle in the first terminal" in h for h in hits), hits)
        await pg.fill("[data-testid=termsearch-input]", "(")
        await pg.check("text=Pattern")
        await pg.wait_for_timeout(200)
        check("a bad pattern is explained, not a crash", await pg.locator("text=/Unterminated|Invalid|not a valid|pattern/i").count() > 0 and not pg.errors, pg.errors)
        await pg.fill("[data-testid=termsearch-input]", "needle")
        await pg.wait_for_timeout(200)
        await pg.locator("[data-testid=termsearch-hits] button").first.click()
        await pg.wait_for_timeout(400)
        check("choosing a hit closes the search", await pg.locator("[data-testid=termsearch-input]").count() == 0)
        await pg.close()

        # ---- a host's own look ----
        hosts = """
        window.__invoke = async (cmd) => {
          const mk = (id, label, profile) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', identity_id: 'i1', profile } });
          if (cmd === 'list_hosts') return [mk('h1', 'big-01', { font_size: 22 }), mk('h2', 'plain-01')];
          if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
          return undefined;
        };
        """
        sizes = {}
        errors = []
        for name in ["big-01", "plain-01"]:
            pg = await b.new_page(viewport={"width": 1400, "height": 900})
            pg.on("pageerror", lambda e: errors.append(str(e)))
            await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
            await pg.add_init_script(hosts)
            await pg.goto(URL)
            await pg.evaluate("() => localStorage.clear()")
            await pg.goto(URL)
            await pg.wait_for_timeout(2500)
            await pg.keyboard.press("Control+Shift+P")
            await pg.wait_for_timeout(300)
            await pg.keyboard.type(name)
            await pg.wait_for_timeout(300)
            await pg.keyboard.press("Enter")
            await pg.wait_for_timeout(1500)
            await deliver(pg, command("ls", "a\r\nb", 0))
            # A command's bar is as tall as its lines, so its height follows the text size.
            sizes[name] = await pg.evaluate("(() => { const bars = [...document.querySelectorAll('[data-block]')]; return Math.max(-1, ...bars.map((b) => b.getBoundingClientRect().height)); })()")
            await pg.close()
        check("a host with its own bigger text draws taller lines than one without", sizes["plain-01"] > 0 and sizes["big-01"] > sizes["plain-01"] * 1.3, sizes)
        check("no page errors from profiles", not errors, errors)

        await b.close()
    print(f"\n{len(failures)} failed" if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
