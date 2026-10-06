"""Smart completion, inline suggestion, in a real (headless) browser with the backend mocked.

    pnpm dev &            # serves http://127.0.0.1:1420
    python3 src/lib/__tests__/e2e/completion.py [chromium-executable]

Checks: type -> see the ghost, accept (whole, one word), dismiss, resize and zoom keep it on the
cursor, it never covers the next line, accepting never presses Enter, nothing reaches storage while
"Remember commands" is off, and the master switch removes it all. (What "Remember commands" saves is
tested in completionhistory.test.ts; local terminals have no host to save it under.)
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def ghost(pg):
    """The suggestion's box, plus where the terminal's cell grid is, so tests can say which cell it is on."""
    return await pg.evaluate("""() => {
      const g = document.querySelector('[data-testid=completion-ghost]');
      if (!g) return null;
      const r = g.getBoundingClientRect(), s = getComputedStyle(g);
      const sc = document.querySelector('.xterm-screen').getBoundingClientRect();
      const text = g.textContent;
      return { text, left: r.left, top: r.top, width: r.width, height: r.height, size: s.fontSize,
               textWidth: g.scrollWidth, screenLeft: sc.left, screenTop: sc.top,
               cellW: r.width / [...text].length, cellH: r.height };
    }""")


def cell(g):
    """(column, row) the ghost starts on, as fractional cells."""
    return ((g["left"] - g["screenLeft"]) / g["cellW"], (g["top"] - g["screenTop"]) / g["cellH"])


def on(g, col, row):
    c = cell(g)
    return abs(c[0] - col) < 0.1 and abs(c[1] - row) < 0.1


async def session(pg, prefs):
    await pg.evaluate("(p) => { localStorage.clear(); localStorage.setItem('sshvault.prefs.v1', JSON.stringify(p)); }", prefs)
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    await pg.keyboard.press("Control+Shift+Backquote")
    await pg.wait_for_timeout(1200)
    await pg.click(".xterm")
    await pg.wait_for_timeout(300)


async def run_line(pg, text):
    await pg.keyboard.type(text)
    await pg.keyboard.press("Enter")
    await pg.wait_for_timeout(250)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1200, "height": 800})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.goto(URL)

        # ---- on, "Remember commands" off ----
        await session(pg, {"smartCompletion": True, "acInline": True, "rememberCommands": False})
        await run_line(pg, "ls -la /srv")
        await run_line(pg, "git status")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        g = await ghost(pg)
        check("typing shows the ghost", bool(g) and g["text"] == "a /srv", g)
        if g:
            # Prompt "$ " plus "ls -l" is 7 cells; four earlier lines (two commands and their output).
            check("ghost starts on the cursor's cell", on(g, 7, 4), cell(g))
            check("ghost is one cell high, so it cannot cover the next line", g["height"] == g["cellH"] and g["cellH"] < 40, g)
            lefts = await pg.evaluate("[...document.querySelectorAll('[data-testid=completion-ghost] span')].map(s => s.getBoundingClientRect().left)")
            check("every character sits in its own cell of the terminal's grid", all(abs(l - (g["left"] + i * g["cellW"])) < 0.5 for i, l in enumerate(lefts)) and len(lefts) == 6, lefts)
        if os.environ.get("SHOT"):
            await pg.screenshot(path=os.environ["SHOT"], clip={"x": 348, "y": 72, "width": 500, "height": 140})

        # accept the whole suggestion with Right
        sent_before = await pg.evaluate("window.__sent.length")
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(300)
        sent = await pg.evaluate("window.__sent.slice(%d)" % sent_before)
        flat = [x for s in sent for x in s]
        check("accept types the rest of the line", bytes(flat).decode() == "a /srv", flat)
        check("accept never presses Enter", 13 not in flat and 10 not in flat, flat)
        check("ghost gone after accept", await ghost(pg) is None)
        check("the shell now holds the complete line", await pg.evaluate("window.__line") == "ls -la /srv")

        # clear, then dismiss with Esc
        await pg.keyboard.press("Control+u")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        check("ghost back for a new line", await ghost(pg) is not None)
        before = await pg.evaluate("window.__sent.length")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        check("Esc hides it", await ghost(pg) is None)
        check("Esc is not sent to the shell while a suggestion is showing", await pg.evaluate("window.__sent.length") == before)
        await pg.keyboard.type("a")
        await pg.wait_for_timeout(300)
        g = await ghost(pg)
        check("a changed line suggests again", bool(g) and g["text"] == " /srv", g)

        # one word at a time
        await pg.keyboard.press("Control+u")
        await pg.keyboard.type("ls ")
        await pg.wait_for_timeout(300)
        before = await pg.evaluate("window.__sent.length")
        await pg.keyboard.press("Control+ArrowRight")
        await pg.wait_for_timeout(300)
        sent = await pg.evaluate("window.__sent.slice(%d)" % before)
        check("Ctrl+Right accepts one word", bytes([x for s in sent for x in s]).decode() == "-la", sent)
        g = await ghost(pg)
        check("the rest is offered again after a word", bool(g) and g["text"] == " /srv", g)

        # End accepts too; keys with no suggestion go to the shell
        await pg.keyboard.press("End")
        await pg.wait_for_timeout(300)
        check("End accepts", await ghost(pg) is None)
        before = await pg.evaluate("window.__sent.length")
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(200)
        check("Right with nothing showing reaches the shell", await pg.evaluate("window.__sent.length") == before + 1)

        # Resize and zoom. The terminal reflows the line, so the suggestion is dropped rather than drawn
        # where the line no longer is; the next prompt brings it back, on the new grid.
        await pg.keyboard.press("Control+u")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        await pg.set_viewport_size({"width": 700, "height": 500})
        await pg.wait_for_timeout(700)
        g = await ghost(pg)
        check("after a resize it is gone or still on the cursor's cell, never misplaced", g is None or on(g, 7, round(cell(g)[1])), g and cell(g))
        await pg.keyboard.press("Control+u")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        g = await ghost(pg)
        check("on the next line it is back, on the cursor's cell of the new grid", bool(g) and on(g, 7, round(cell(g)[1])) and abs(cell(g)[1] - round(cell(g)[1])) < 0.1, g and cell(g))
        sh = await pg.evaluate("document.querySelector('.xterm-screen').getBoundingClientRect().bottom")
        check("it stays inside the terminal", bool(g) and g["top"] + g["height"] <= sh + 0.5, (g, sh))
        await pg.keyboard.press("Control+Equal")
        await pg.keyboard.press("Control+Equal")
        await pg.wait_for_timeout(700)
        await pg.keyboard.press("Control+u")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        g2 = await ghost(pg)
        check("after zoom it is drawn at the larger size, on the cursor's cell", bool(g) and bool(g2) and float(g2["size"][:-2]) > float(g["size"][:-2]) and on(g2, 7, round(cell(g2)[1])) and g2["cellW"] > g["cellW"], g2 and (g2, cell(g2)))
        await pg.keyboard.press("Control+0")
        await pg.wait_for_timeout(500)

        # nothing in storage
        dump = await pg.evaluate("JSON.stringify(Object.fromEntries(Object.entries(localStorage)))")
        check("no command text in storage while Remember commands is off", "ls -la" not in dump and "git status" not in dump and "sshvault.history.v1" not in json.loads(dump))

        # secrets never offered
        await pg.keyboard.press("Control+u")
        await run_line(pg, "export API_TOKEN=abc123xyz")
        await pg.keyboard.type("export API")
        await pg.wait_for_timeout(300)
        check("a secret-looking command is never suggested", await ghost(pg) is None)
        await pg.keyboard.press("Control+u")
        await run_line(pg, " echo hidden-by-space")
        await pg.keyboard.type("echo hid")
        await pg.wait_for_timeout(300)
        check("a command that starts with a space is never suggested", await ghost(pg) is None)

        # ---- master switch off ----
        await session(pg, {"smartCompletion": False, "rememberCommands": False})
        await run_line(pg, "ls -la /srv")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(400)
        check("master switch off: no ghost", await ghost(pg) is None)
        before = await pg.evaluate("window.__sent.length")
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(200)
        check("master switch off: Right goes to the shell", await pg.evaluate("window.__sent.length") == before + 1)

        check("no page errors", not errors, errors[:3])
        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
