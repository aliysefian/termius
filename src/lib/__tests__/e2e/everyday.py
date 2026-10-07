"""Everyday experience (feat2 4.12) in a real (headless) browser with the backend mocked: the tour, languages,
the sidebar's layout, finding settings and more from the palette, snippet packs, and the themes.

    pnpm dev &
    python3 src/lib/__tests__/e2e/everyday.py [chromium-executable]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

HOSTS = """
const mk = (id, label, env, group) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group, tags: [], notes: '', environment: env, identity_id: 'i1' } });
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return [mk('h1', 'web-01', 'production', 'Production/Web'), mk('h2', 'db-01', 'staging', 'Staging')];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  if (cmd === 'list_keys') return [{ id: 'k1', rev: 1, updated_at: 1, deleted: false, data: { name: 'deploy-key', algorithm: 'ssh-ed25519', public_key: 'x', fingerprint: 'SHA256:abc' } }];
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b, prefs=None, init=""):
    pg = await b.new_page(viewport={"width": 1400, "height": 900})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(HOSTS + init)
    await pg.goto(URL)
    await pg.evaluate("(p) => localStorage.setItem('sshvault.prefs.v1', JSON.stringify(p))", prefs or {})
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    return pg


async def palette(pg, text):
    await pg.keyboard.press("Control+Shift+P")
    await pg.wait_for_timeout(300)
    await pg.keyboard.type(text)
    await pg.wait_for_timeout(300)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- the tour ----
        pg = await fresh(b)
        await palette(pg, "Take the tour")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(500)
        check("the tour opens from the palette", await pg.locator('[data-testid=tour]').count() == 1 and await pg.locator("text=Step 1 of 6").count() == 1)
        spot = await pg.locator('[data-testid=tour-spot]').bounding_box()
        rail = await pg.locator('nav[aria-label]').first.bounding_box()
        check("step 1 lights the sidebar", spot and abs(spot["x"] - (rail["x"] - 6)) < 3 and spot["height"] >= rail["height"] - 20, (spot, rail))
        check("the card is a labelled dialog with focus", await pg.locator('[role=dialog][aria-labelledby=tour-title]').count() == 1 and await pg.evaluate("document.activeElement?.getAttribute('role')") == "dialog")
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(500)
        spot = await pg.locator('[data-testid=tour-spot]').bounding_box()
        panel = await pg.locator('section[aria-label="List panel"]').bounding_box()
        check("step 2 lights the host list", await pg.locator("text=Step 2 of 6").count() == 1 and spot and abs(spot["x"] - (panel["x"] - 6)) < 3, (spot, panel))
        card = await pg.locator('[role=dialog][aria-labelledby=tour-title]').bounding_box()
        check("the card sits beside what it points at, inside the window", card["x"] >= spot["x"] + spot["width"] and card["x"] + card["width"] <= 1400 and card["y"] >= 0 and card["y"] + card["height"] <= 900, card)
        await pg.keyboard.press("ArrowLeft")
        await pg.wait_for_timeout(300)
        check("the left arrow goes back", await pg.locator("text=Step 1 of 6").count() == 1)
        for _ in range(5):
            await pg.click('[role=dialog] button.btn-primary')
            await pg.wait_for_timeout(250)
        check("the last step has no spotlight and says Done", await pg.locator("text=That's the tour").count() == 1 and await pg.locator('[data-testid=tour-spot]').count() == 0 and await pg.locator('[role=dialog] button.btn-primary:has-text("Done")').count() == 1)
        await pg.click('[role=dialog] button.btn-primary')
        await pg.wait_for_timeout(300)
        check("Done closes it", await pg.locator('[data-testid=tour]').count() == 0)
        await palette(pg, "Take the tour")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(400)
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        check("Esc closes it", await pg.locator('[data-testid=tour]').count() == 0)
        check("no page errors (tour)", not pg.errors, pg.errors[:2])
        await pg.close()

        # ---- languages ----
        pg = await fresh(b)
        check("English by default", await pg.locator('button[aria-label="Files"]').count() == 1 and await pg.evaluate("document.documentElement.lang") == "en")
        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_timeout(500)
        await pg.select_option("#s-language", "de")
        await pg.wait_for_timeout(400)
        check("German renames the sidebar", await pg.locator('button[aria-label="Dateien"]').count() == 1 and await pg.locator('button[aria-label="Verwalten"]').count() == 1)
        check("and sets the page language", await pg.evaluate("document.documentElement.lang") == "de")
        await pg.click('button[aria-label="Hosts"]')
        await pg.wait_for_timeout(400)
        await pg.keyboard.press("Control+Shift+P"); await pg.keyboard.press("Escape")
        await pg.click('button[aria-label="Einstellungen"]')
        await pg.wait_for_timeout(300)
        await pg.select_option("#s-language", "ja")
        await pg.wait_for_timeout(300)
        check("Japanese too", await pg.locator('button[aria-label="ファイル"]').count() == 1)
        await pg.select_option("#s-language", "fa")
        await pg.wait_for_timeout(300)
        check("Persian: the text is Persian and the page language is fa", await pg.locator('button[aria-label="فایل‌ها"]').count() == 1 and await pg.evaluate("document.documentElement.lang") == "fa")
        prefs = json.loads(await pg.evaluate("localStorage.getItem('sshvault.prefs.v1')"))
        check("the choice is remembered", prefs.get("language") == "fa", prefs.get("language"))
        await pg.select_option("#s-language", "auto")
        await pg.wait_for_timeout(300)
        check("Match the system goes back to English here", await pg.locator('button[aria-label="Files"]').count() == 1)
        check("no page errors (languages)", not pg.errors, pg.errors[:2])
        await pg.close()
        pg = await fresh(b, {"language": "es"})
        check("a saved language is used at start-up", await pg.locator('button[aria-label="Archivos"]').count() == 1 and await pg.locator("text=Buscar servidores…").count() == 0 and await pg.locator('input[placeholder="Buscar servidores…"]').count() == 1)
        await pg.close()

        # ---- the sidebar's layout ----
        pg = await fresh(b)
        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_timeout(400)
        await pg.fill('input[placeholder="Search…"]', "sidebar")
        await pg.wait_for_timeout(300)
        await pg.uncheck('input[aria-label="Show Databases in the sidebar"]')
        await pg.wait_for_timeout(300)
        check("a hidden entry leaves the rail", await pg.locator('nav button[aria-label="Databases"]').count() == 0)
        await pg.click('nav button[aria-label="Manage"]')
        await pg.wait_for_timeout(300)
        check("and is in the Manage menu instead", await pg.locator('[role=menu] >> text=Put away from the sidebar').count() == 1 and await pg.locator('[role=menuitem]:has-text("Databases")').count() == 1)
        await pg.keyboard.press("Escape")
        await pg.click('button[aria-label="Move Tunnels up"]')
        await pg.click('button[aria-label="Move Tunnels up"]')
        await pg.wait_for_timeout(300)
        order = await pg.evaluate("[...document.querySelectorAll('nav > button[aria-label]')].map(b => b.getAttribute('aria-label'))")
        check("moving an entry up changes the order on the rail", order.index("Tunnels") < order.index("Snippets") < order.index("Files"), order)
        check("Hosts can't be put away", await pg.locator('input[aria-label="Show Hosts in the sidebar"]').is_disabled())
        await pg.uncheck('text=Show a name under each icon')
        await pg.wait_for_timeout(300)
        check("labels off: the rail is icon-only, with names still on the buttons", await pg.locator('nav >> text=Snippets').count() == 0 and await pg.locator('nav button[aria-label="Snippets"]').count() == 1)
        await pg.click('button:has-text("Reset")')
        await pg.wait_for_timeout(300)
        check("Reset puts it all back", await pg.locator('nav button[aria-label="Databases"]').count() == 1 and await pg.locator('nav >> text=Snippets').count() >= 1)
        check("no page errors (sidebar)", not pg.errors, pg.errors[:2])
        await pg.close()

        # ---- the palette finds more ----
        pg = await fresh(b)
        await palette(pg, "Setting: font")
        check("settings are in the palette", await pg.locator('[role=option]:has-text("Setting: Font and font size")').count() == 1)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(600)
        check("choosing one opens Settings with it searched for", await pg.input_value('input[placeholder="Search…"]') == "font" and await pg.locator('h2:has-text("Terminal appearance")').count() == 1)
        await pg.click('button[aria-label="Hosts"]')
        await palette(pg, "Credential: ops")
        check("credentials are found", await pg.locator('[role=option]:has-text("Credential: ops")').count() == 1)
        await pg.keyboard.press("Escape")
        await palette(pg, "Group: Production/Web")
        check("groups are found, including nested ones", await pg.locator('[role=option]:has-text("Group: Production/Web")').count() == 1)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(400)
        check("a group shows its hosts", await pg.input_value('input[placeholder="Search hosts…"]') == "Production/Web" and await pg.locator("text=web-01").count() >= 1)
        await palette(pg, "Key: deploy")
        check("keys are found", await pg.locator('[role=option]:has-text("Key: deploy-key")').count() == 1)
        await pg.keyboard.press("Escape")
        await palette(pg, "x")
        combo = pg.locator('[role=combobox]')
        check("the palette is a labelled combobox over a listbox", await combo.count() == 1 and await pg.locator('[role=listbox]').count() == 1 and await combo.get_attribute("aria-controls") == "palette-list")
        await pg.keyboard.press("Escape")
        check("no page errors (palette)", not pg.errors, pg.errors[:2])
        await pg.close()

        # ---- snippet packs ----
        pack = {"format": "sshvault-snippets", "version": 1, "snippets": [
            {"label": "From file", "command": "uptime", "description": "d", "folder": "Mine", "tags": ["x"]},
            {"label": "Bad folder", "command": "ls", "folder": "../etc"},
            {"label": "Control", "command": "ls\u001b[31m"},
        ]}
        pg = await fresh(b, init=f"window.__pickedFile = '/tmp/pack.json'; window.__fileText = {json.dumps(json.dumps(pack))};")
        await pg.click('button[aria-label="Snippets"]')
        await pg.wait_for_timeout(500)
        await pg.click('button[title^="Add the starter snippets"]')
        await pg.wait_for_timeout(400)
        boxes = pg.locator('[role=dialog] input[type=checkbox]')
        n = await boxes.count()
        check("the starter set is offered, all ticked", n >= 20 and await pg.locator('[role=dialog] input[type=checkbox]:checked').count() == n, n)
        check("it says nothing was run and what they are", await pg.locator("text=none of them changes anything").count() == 1)
        await boxes.first.uncheck()
        await pg.click('[role=dialog] button.btn-primary')
        await pg.wait_for_timeout(1500)
        saved = await pg.evaluate("window.__savedSnippets")
        check("the ticked ones are saved, in folders, and the unticked one is not", len(saved) == n - 1 and all(s["folder"].startswith("Starter/") for s in saved) and all(s["command"] for s in saved), len(saved))
        await pg.click('button[title^="Add the starter snippets"]')
        await pg.wait_for_timeout(400)
        check("asking again offers only the one left out", await pg.locator('[role=dialog] input[type=checkbox]').count() == 1)
        await pg.click('[role=tab]:has-text("From a file")')
        await pg.click('button:has-text("Choose a file")')
        await pg.wait_for_timeout(600)
        check("a pack file is read and previewed", await pg.locator("text=From file").count() >= 1 and await pg.locator("text=2 left out as unusable").count() == 1, await pg.inner_text('[role=dialog]'))
        check("what was left out says why", await pg.locator("text=its folder is not usable").count() == 1 and await pg.locator("text=no usable command").count() == 1)
        await pg.click('[role=dialog] button.btn-primary')
        await pg.wait_for_timeout(600)
        saved = await pg.evaluate("window.__savedSnippets")
        check("only the good snippet from the file is added", saved[-1]["label"] == "From file" and saved[-1]["folder"] == "Mine" and not any(s["label"] in ("Bad folder", "Control") for s in saved))
        check("no page errors (snippets)", not pg.errors, pg.errors[:2])
        await pg.close()

        # ---- themes ----
        pg = await fresh(b)
        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_timeout(500)
        cards = await pg.locator('button[aria-pressed]').count()
        check("the theme gallery has the new themes", await pg.locator('button:has-text("Catppuccin Mocha")').count() == 1 and await pg.locator('button:has-text("High contrast (dark)")').count() == 1 and cards >= 19, cards)
        await pg.click('button:has-text("Rosé Pine")')
        prefs = json.loads(await pg.evaluate("localStorage.getItem('sshvault.prefs.v1')"))
        check("choosing one is remembered", prefs.get("themeId") == "rose-pine")
        await pg.select_option("#s-app-theme", "contrast")
        await pg.wait_for_timeout(300)
        look = await pg.evaluate("({ theme: document.documentElement.dataset.theme, base: getComputedStyle(document.documentElement).getPropertyValue('--color-base').trim(), line: getComputedStyle(document.documentElement).getPropertyValue('--color-line').trim() })")
        check("high contrast applies its colours", look == {"theme": "contrast", "base": "#000000", "line": "#9a9a9a"}, look)
        await pg.select_option("#s-app-theme", "dark")
        await pg.wait_for_timeout(300)
        check("and takes them off again", await pg.evaluate("getComputedStyle(document.documentElement).getPropertyValue('--color-base').trim()") != "#000000" and await pg.evaluate("document.documentElement.style.getPropertyValue('--color-base')") == "")
        check("no page errors (themes)", not pg.errors, pg.errors[:2])
        await pg.close()

        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
