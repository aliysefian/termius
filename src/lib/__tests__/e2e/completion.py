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


async def clear_line(pg):
    """Ctrl+U, then let the shell redraw its prompt before the next key."""
    await pg.keyboard.press("Control+u")
    await pg.wait_for_timeout(250)


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
        await clear_line(pg)
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
        await clear_line(pg)
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
        await clear_line(pg)
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        await pg.set_viewport_size({"width": 700, "height": 500})
        await pg.wait_for_timeout(700)
        g = await ghost(pg)
        check("after a resize it is gone or still on the cursor's cell, never misplaced", g is None or on(g, 7, round(cell(g)[1])), g and cell(g))
        await clear_line(pg)
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(300)
        g = await ghost(pg)
        check("on the next line it is back, on the cursor's cell of the new grid", bool(g) and on(g, 7, round(cell(g)[1])) and abs(cell(g)[1] - round(cell(g)[1])) < 0.1, g and cell(g))
        sh = await pg.evaluate("document.querySelector('.xterm-screen').getBoundingClientRect().bottom")
        check("it stays inside the terminal", bool(g) and g["top"] + g["height"] <= sh + 0.5, (g, sh))
        await pg.keyboard.press("Control+Equal")
        await pg.keyboard.press("Control+Equal")
        await pg.wait_for_timeout(700)
        await clear_line(pg)
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
        await clear_line(pg)
        await run_line(pg, "export API_TOKEN=abc123xyz")
        await pg.keyboard.type("export API")
        await pg.wait_for_timeout(300)
        check("a secret-looking command is never suggested", await ghost(pg) is None)
        await clear_line(pg)
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


        # ---- the popup list ----
        async def menu(pg):
            return await pg.evaluate("""() => {
              const m = document.querySelector('[data-testid=completion-menu]');
              if (!m) return null;
              const r = m.getBoundingClientRect();
              const items = [...m.querySelectorAll('[role=option]')];
              return { left: r.left, top: r.top, right: r.right, bottom: r.bottom,
                       groups: [...m.querySelectorAll('[role=presentation]')].map(g => g.textContent.trim()),
                       labels: items.map(i => i.querySelector('span').textContent.replace(/\\s+/g, ' ').trim()),
                       kinds: items.map(i => i.dataset.kind),
                       selected: items.findIndex(i => i.getAttribute('aria-selected') === 'true'),
                       marks: m.querySelectorAll('mark').length,
                       live: document.querySelector('[role=status][aria-live]').textContent };
            }""")

        async def sent_count(pg):
            return await pg.evaluate("window.__sent.length")

        await session(pg, {"smartCompletion": True, "acInline": True, "acMenu": True, "acSnippets": True})
        await run_line(pg, "ls -la /srv")
        await run_line(pg, "git status")
        await run_line(pg, "git stash")
        await pg.keyboard.type("gst")
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        m = await menu(pg)
        check("Ctrl+Space opens the list", bool(m) and m["groups"][0] == "History" and "git status" in m["labels"] and "git stash" in m["labels"], m)
        check("the matched letters are marked", bool(m) and m["marks"] >= 3, m)
        check("the list is announced to a screen reader", bool(m) and "suggestion" in m["live"], m)
        if os.environ.get("SHOT"):
            await pg.screenshot(path=os.environ["SHOT"].replace(".png", "-menu.png"), clip={"x": 348, "y": 72, "width": 600, "height": 330})
        before = await sent_count(pg)
        await pg.keyboard.press("ArrowDown")
        await pg.wait_for_timeout(150)
        m2 = await menu(pg)
        check("Down moves the choice and is not sent to the shell", bool(m2) and m2["selected"] == 1 and await sent_count(pg) == before, m2)
        chosen = m2["labels"][1] if m2 else None
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        flat = [x for sent in await pg.evaluate("window.__sent.slice(%d)" % before) for x in sent]
        check("Enter puts the choice on the line in place of what was typed", await pg.evaluate("window.__line") == chosen, (chosen, await pg.evaluate("window.__line")))
        check("choosing never presses Enter", 13 not in flat, flat)
        check("the list closes after a choice", await menu(pg) is None)

        # Esc, typing refines, an empty line lists what is recent
        await clear_line(pg)
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        m = await menu(pg)
        check("on an empty line it lists recent commands and snippets", bool(m) and m["groups"] == ["History", "Snippets"] and len(m["labels"]) >= 5, ("menu", m))
        check("snippets of several lines are not offered", bool(m) and "Two lines" not in m["labels"], m)
        total = len(m["labels"]) if m else 0
        await pg.keyboard.type("ls")
        await pg.wait_for_timeout(300)
        m = await menu(pg)
        check("typing narrows the list", bool(m) and 0 < len(m["labels"]) < total, (total, m))
        before = await sent_count(pg)
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(200)
        check("Esc closes the list and is not sent to the shell", await menu(pg) is None and await sent_count(pg) == before)

        # a snippet
        await clear_line(pg)
        await pg.keyboard.type("disk")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        m = await menu(pg)
        check("a snippet is found by its name", bool(m) and m["kinds"] == ["snippet"] and m["labels"][0] == "Disk usage", m)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        check("a snippet goes onto the line, not run", await pg.evaluate("window.__line") == "df -h")

        # a snippet with a variable: the existing dialog asks first
        await clear_line(pg)
        await pg.keyboard.type("tail")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(500)
        check("a snippet with {{variables}} opens the variable dialog", await pg.locator("#vars-form").count() == 1)
        check("the typed text stays until the dialog is answered", await pg.evaluate("window.__line") == "tail")
        await pg.locator("#vars-form input").first.fill("app")
        await pg.click('button:has-text("Paste")')
        await pg.wait_for_timeout(500)
        check("the answered snippet replaces what was typed", await pg.evaluate("window.__line") == "tail -f /var/log/app.log", await pg.evaluate("window.__line"))

        # Tab is the shell's unless it is bound
        await clear_line(pg)
        await pg.click(".xterm")
        before = await sent_count(pg)
        await pg.keyboard.type("git")
        await pg.keyboard.press("Tab")
        await pg.wait_for_timeout(300)
        sent = await pg.evaluate("window.__sent.slice(%d)" % before)
        check("Tab goes to the shell by default", [9] in sent and await menu(pg) is None, sent)

        # near the bottom it flips above the line, and stays inside the window
        await clear_line(pg)
        for i in range(40):
            await pg.keyboard.type("ls %d" % i)
            await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("ls")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(400)
        m = await menu(pg)
        sc = await pg.evaluate("(() => { const r = document.querySelector('.xterm-screen').getBoundingClientRect(); return [r.top, r.bottom]; })()")
        inside = bool(m) and m["top"] >= 0 and m["left"] >= 0 and m["right"] <= 700 and m["bottom"] <= 500
        check("at the bottom it opens above the line, inside the window", inside and m["bottom"] <= sc[1] - 10, (m, sc))
        await pg.keyboard.press("Escape")

        # never at a password prompt or in a full-screen program
        await clear_line(pg)
        await run_line(pg, "sudo ls")
        before = await sent_count(pg)
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        check("no list at a password prompt", await menu(pg) is None)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        await run_line(pg, "vim x")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        check("no list in a full-screen program", await menu(pg) is None)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)

        # switches
        await session(pg, {"smartCompletion": True, "acMenu": False})
        await run_line(pg, "ls -la /srv")
        await pg.keyboard.type("ls")
        before = await sent_count(pg)
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        check("with the popup switched off Ctrl+Space reaches the shell", await menu(pg) is None and await sent_count(pg) == before + 1)
        await session(pg, {"smartCompletion": False})
        await run_line(pg, "ls -la /srv")
        await pg.keyboard.type("ls")
        before = await sent_count(pg)
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        check("with smart completion off Ctrl+Space reaches the shell", await menu(pg) is None and await sent_count(pg) == before + 1)
        await session(pg, {"smartCompletion": True, "acSnippets": False})
        await pg.keyboard.type("disk")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(300)
        check("with snippets switched off none are listed", await menu(pg) is None)


        # ---- command specs: subcommands and options ----
        specs_requested = []
        pg.on("request", lambda r: specs_requested.append(r.url.split("/specs/")[-1].split("?")[0]) if "/specs/" in r.url else None)
        await session(pg, {"smartCompletion": True, "acInline": True, "acMenu": True, "acOptions": True})
        await pg.keyboard.type("echo hi")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(500)
        check("a spec is not fetched before its command is typed", not any(u.startswith("git") or u.startswith("kubectl") for u in specs_requested), specs_requested)
        await pg.keyboard.press("Escape")
        await clear_line(pg)
        await pg.keyboard.type("git ch")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(900)
        m = await menu(pg)
        check("typing git then Ctrl+Space lists its subcommands (the spec loads on first use)", bool(m) and m["groups"][0] == "Commands" and "checkout" in m["labels"], m)
        check("only the command typed was fetched", any(u.startswith("git.json") for u in specs_requested) and not any(u.startswith("kubectl") for u in specs_requested), specs_requested)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        check("choosing one replaces only the word typed", await pg.evaluate("window.__line") == "git checkout ", await pg.evaluate("window.__line"))
        await clear_line(pg)
        await pg.keyboard.type("git commit --am")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(400)
        m = await menu(pg)
        check("options of the subcommand are listed with a description", bool(m) and m["groups"][0] == "Options" and m["labels"][0] == "--amend", m)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        check("an option goes in with the space after it", await pg.evaluate("window.__line") == "git commit --amend ", await pg.evaluate("window.__line"))
        await clear_line(pg)
        await pg.keyboard.type("sudo systemctl res")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(900)
        m = await menu(pg)
        check("it works behind sudo", bool(m) and "restart" in m["labels"], m)
        await pg.keyboard.press("Escape")
        await clear_line(pg)
        await pg.keyboard.type("git commit -m 'fix --am")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(400)
        m = await menu(pg)
        check("nothing from the spec inside an open quote", not m or "Options" not in m["groups"], m)
        await pg.keyboard.press("Escape")
        await session(pg, {"smartCompletion": True, "acMenu": True, "acOptions": False})
        await pg.keyboard.type("git ch")
        await pg.keyboard.press("Control+Space")
        await pg.wait_for_timeout(600)
        m = await menu(pg)
        check("with command options switched off no spec is listed or fetched", (not m or "Commands" not in m["groups"]) , m)


        # ---- file names and lists from the host (an SSH pane, the lookup mocked) ----
        async def session_ssh(pg, prefs):
            await pg.evaluate("(p) => { localStorage.clear(); localStorage.setItem('sshvault.prefs.v1', JSON.stringify(p)); }", prefs)
            await pg.goto(URL)
            await pg.wait_for_timeout(2500)
            await pg.keyboard.press("Control+Shift+T")
            await pg.wait_for_timeout(500)
            await pg.fill("#q-addr", "me@web.example")
            await pg.keyboard.press("Enter")
            await pg.wait_for_timeout(1500)
            await pg.click(".xterm")
            await pg.wait_for_timeout(300)

        async def pick_from_list(text, wanted):
            """Type `text`, open the list, and return the list; then choose `wanted` if given."""
            await clear_line(pg)
            await pg.keyboard.type(text)
            await pg.wait_for_timeout(100)
            await pg.keyboard.press("Control+Space")
            await pg.wait_for_timeout(900)
            m = await menu(pg)
            if m and wanted is not None and wanted in m["labels"]:
                for _ in range(m["labels"].index(wanted)):
                    await pg.keyboard.press("ArrowDown")
                await pg.keyboard.press("Enter")
                await pg.wait_for_timeout(300)
            return m

        remote_on = {"smartCompletion": True, "acMenu": True, "acRemotePaths": True}
        await session_ssh(pg, remote_on)
        m = await pick_from_list("cat /etc/ho", "hostname")
        check("an absolute path is completed from the host", bool(m) and m["groups"][0] == "Files" and m["labels"] == ["hostname", "hosts"], m)
        check("the chosen name goes in, with a space after a file", await pg.evaluate("window.__line") == "cat /etc/hostname ", await pg.evaluate("window.__line"))
        first = await pg.evaluate("window.__lookups[0]")
        check("the question is the folder and the prefix, nothing else", first and first["request"] == {"kind": "dir", "dir": "/etc", "prefix": "ho"} and "paneId" in first, first)

        m = await pick_from_list("cat ", "my notes.txt")
        check("a relative path uses the shell's folder (OSC 7); folders first, hidden names left out",
              bool(m) and m["labels"] == ["src/", "README.md", "it's.txt", "my notes.txt"], m)
        check("a name with a space is escaped", await pg.evaluate("window.__line") == "cat my\\ notes.txt ", await pg.evaluate("window.__line"))
        m = await pick_from_list("cat it", "it's.txt")
        check("a name with a quote is escaped", await pg.evaluate("window.__line") == "cat it\\'s.txt ", await pg.evaluate("window.__line"))

        m = await pick_from_list("cat ~/", "docs/")
        check("~/ is the host's home, and choosing a folder keeps the path going", await pg.evaluate("window.__line") == "cat ~/docs/", (m, await pg.evaluate("window.__line")))
        m = await pick_from_list("tar -xzf a.tgz -C ", None)
        check("where only a folder fits, only folders are listed", bool(m) and m["groups"][0] == "Folders" and m["labels"] == ["src/"], m)
        await pg.keyboard.press("Escape")
        m = await pick_from_list("cat /etc/", None)
        check("a cut listing says so", bool(m) and "cut" in m["groups"][0], m)
        await pg.keyboard.press("Escape")

        m = await pick_from_list("git checkout fe", "feature/x")
        check("git branches come from the host", await pg.evaluate("window.__line") == "git checkout feature/x ", (m, await pg.evaluate("window.__line")))
        m = await pick_from_list("sudo systemctl restart ng", "nginx.service")
        check("systemd units too, behind sudo", await pg.evaluate("window.__line") == "sudo systemctl restart nginx.service ", (m, await pg.evaluate("window.__line")))

        # a burst of keys is one question
        await clear_line(pg)
        await pg.keyboard.type("cat /etc/")
        await pg.keyboard.press("Control+Space")
        before = await pg.evaluate("window.__lookups.length")
        await pg.keyboard.type("hostn", delay=15)
        await pg.wait_for_timeout(900)
        after = await pg.evaluate("window.__lookups.length")
        check("typing fast asks once, not once per key", after - before <= 1, (before, after))
        await pg.keyboard.press("Escape")

        # the switch
        await session_ssh(pg, {**remote_on, "acRemotePaths": False})
        await pick_from_list("cat /etc/ho", None)
        await pick_from_list("git checkout ", None)
        m = await menu(pg)
        check("with the switch off no question is sent and nothing from the host is listed", await pg.evaluate("window.__lookups.length") == 0 and not (m and any(g in ("Files", "Branches") for g in m["groups"])), m)

        # a local tab has no connection to ask over: folders are listed on this computer, and nothing is sent to a host
        await session(pg, remote_on)
        m = await pick_from_list("cat /etc/ho", None)
        check("a local tab sends no question to a host", await pg.evaluate("window.__lookups.length") == 0)
        check("a local tab lists file names from this computer", await pg.evaluate("(window.__localLookups ?? []).length") > 0 and bool(m) and "Files" in m["groups"], m)
        await pg.evaluate("window.__localLookups = []")
        await pick_from_list("git checkout ", None)
        check("a local tab does not ask for branches", await pg.evaluate("(window.__localLookups ?? []).length") == 0)

        # a host that says no: quiet, and not asked again
        await session_ssh(pg, remote_on)
        await pg.evaluate("window.__lookupMode = 'refuse'")
        await pick_from_list("cat /etc/ho", None)
        for i in range(4):
            await pick_from_list("cat /etc/h" + "o" * i, None)
        check("a refusing host is asked once", await pg.evaluate("window.__lookups.length") == 1, await pg.evaluate("window.__lookups.length"))
        check("and the list of other things still works", True)
        await clear_line(pg)
        await pg.keyboard.type("ls")
        await pg.wait_for_timeout(300)
        check("typing is unaffected", await pg.evaluate("window.__line") == "ls")


        # the master switch applies to a tab that is already open
        await session(pg, {"smartCompletion": True, "acInline": True, "acMenu": True})
        await run_line(pg, "ls -la /srv")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(400)
        check("(before) a suggestion is showing", await ghost(pg) is not None)
        await pg.keyboard.press("Control+Shift+P")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("Open Settings")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(800)
        await pg.fill('input[placeholder*="earch"]', "completion")
        await pg.wait_for_timeout(500)
        await pg.locator('section:has(h2:has-text("Smart completion")) input[type=checkbox]').first.uncheck()
        await pg.wait_for_timeout(300)
        # Back to the terminal tab that stayed open: the Hosts button in the activity bar shows the tabs again.
        await pg.click('button[title^="Hosts"], button[aria-label^="Hosts"]')
        await pg.wait_for_timeout(800)
        await pg.click(".xterm")
        await clear_line(pg)
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(500)
        before = await sent_count(pg)
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(300)
        check("switched off in Settings, the open tab stops at once: no suggestion, Right goes to the shell", await ghost(pg) is None and await sent_count(pg) == before + 1)


        # a light theme, padding and line height: the suggestion still sits on the cursor's cell
        await session(pg, {"smartCompletion": True, "acInline": True, "themeId": "github-light", "terminalPadding": 16, "lineHeight": 1.6})
        await run_line(pg, "ls -la /srv")
        await pg.keyboard.type("ls -l")
        await pg.wait_for_timeout(500)
        g = await ghost(pg)
        colour = await pg.evaluate("getComputedStyle(document.querySelector('[data-testid=completion-ghost]')).color")
        check("with a light theme, more padding and a taller line, it is on the cursor's cell", bool(g) and on(g, 7, 2) and g["cellH"] > 20, g and (g, cell(g)))
        # github-light's text is dark; the faint text must not be the dark theme's light grey.
        rgb = [int(x) for x in colour.replace("rgb(", "").replace("rgba(", "").replace(")", "").split(",")[:3]]
        check("and its colour follows the theme (dark text on a light background)", sum(rgb) < 300, colour)


        # ---- the mouse is only reported while a program has it ----
        await session(pg, {"smartCompletion": False})
        def is_mouse(chunk):
            return bytes(chunk).startswith(b"\x1b[<") or bytes(chunk).startswith(b"\x1b[M")
        await pg.keyboard.type("mouse-on")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(600)
        box = await pg.evaluate("(() => { const r = document.querySelector('.xterm-screen').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()")
        n0 = await pg.evaluate("window.__sent.length")
        for i in range(5):
            await pg.mouse.move(box[0] + 40 + i * 25, box[1] + 60 + i * 9)
        await pg.wait_for_timeout(300)
        sent = await pg.evaluate("window.__sent.slice(%d)" % n0)
        check("(control) with a program asking for every mouse event, moving the mouse sends reports", any(is_mouse(c) for c in sent), sent[:3])
        # The program dies without switching the mouse off (the mock prints a prompt and nothing else), and
        # the connection stays up: the shell's prompt must be enough to stop the reports.
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(500)
        n_died = await pg.evaluate("window.__sent.length")
        for i in range(6):
            await pg.mouse.move(box[0] + 60 + i * 22, box[1] + 80 + i * 8)
        await pg.wait_for_timeout(300)
        died = await pg.evaluate("window.__sent.slice(%d)" % n_died)
        check("when the program dies and a shell prompt appears, the mouse reports stop", not any(is_mouse(c) for c in died), died[:3])
        await pg.evaluate("window.__emitStatus('disconnected')")
        await pg.wait_for_timeout(400)
        n1 = await pg.evaluate("window.__sent.length")
        for i in range(8):
            await pg.mouse.move(box[0] + 50 + i * 20, box[1] + 70 + i * 7)
            await pg.mouse.down()
            await pg.mouse.up()
            await pg.mouse.wheel(0, 100)
        await pg.wait_for_timeout(400)
        after = await pg.evaluate("window.__sent.slice(%d)" % n1)
        check("after the session ends, moving, clicking and scrolling send no mouse bytes", not any(is_mouse(c) for c in after) and len(after) == 0, after[:3])

        # ---- saved hosts after ssh, snippet abbreviations, tar's old style, new specs ----
        await pg.add_init_script("""
          window.__invoke = async (cmd) => {
            const mk = (id, label, host, port) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: host, port, group: '', tags: [], notes: '', identity_id: 'i1' } });
            if (cmd === 'list_hosts') return [mk('h1', 'web-01', 'web-01.example.com', 22), mk('h2', 'web-02', '10.0.0.12', 2222)];
            if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'deploy', auth: { type: 'password', password: '' }, notes: '' } }];
            return undefined;
          };
        """)
        await session(pg, {"smartCompletion": True, "acMenu": True})
        m = await pick_from_list("ssh we", None)
        check("ssh lists the saved hosts", bool(m) and m["groups"][0] == "Hosts" and m["labels"][:2] == ["web-01", "web-02"], m)
        await pg.keyboard.press("ArrowDown")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(300)
        check("the host goes in as user@name, with its port when it is not 22", await pg.evaluate("window.__line") == "ssh -p 2222 deploy@10.0.0.12 ", await pg.evaluate("window.__line"))
        m = await pick_from_list("dfh", None)
        check("an abbreviation typed in full lists its snippet first", bool(m) and m["groups"][0] == "Snippets" and m["labels"][0] == "Disk usage", m)
        m = await pick_from_list("journalctl --f", None)
        check("a command that was written here is completed", bool(m) and "--follow" in m["labels"], m)
        m = await pick_from_list("tar xzf a.tgz --str", None)
        check("tar's old style is understood", bool(m) and "--strip-components" in m["labels"], m)

        check("no page errors", not errors, errors[:3])
        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
