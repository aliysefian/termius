"""Accessibility checks with axe-core, in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/a11y.py [chromium-executable]

Runs axe on the main pages and dialogs in the dark, light and high-contrast themes and prints what it finds.
Exits with an error for any "serious" or "critical" finding. (axe cannot judge everything: focus order, a screen
reader's actual speech and colour in the terminal canvas need a person.)
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
AXE = open(os.path.join(ROOT, "node_modules", "axe-core", "axe.min.js")).read()
SKIP_RULES = os.environ.get("A11Y_SKIP", "").split(",")

HOSTS = """
const mk = (id, label, env, group) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group, tags: ['db'], notes: '', environment: env, identity_id: 'i1', favorite: id === 'h1' } });
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return [mk('h1', 'web-01', 'production', 'Production/Web'), mk('h2', 'db-01', 'staging', 'Staging'), mk('h3', 'dev-box', 'development', '')];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  if (cmd === 'list_snippets') return [{ id: 's1', rev: 1, updated_at: 1, deleted: false, data: { label: 'Disk usage', command: 'df -h', description: 'Free space' } }];
  return undefined;
};
"""

found = []


async def scan(pg, where, theme):
    res = await pg.evaluate("""async (skip) => {
      const r = await axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] }, rules: Object.fromEntries(skip.filter(Boolean).map((r) => [r, { enabled: false }])) });
      return r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.slice(0, 4).map((n) => n.target.join(' ') + ' :: ' + (n.failureSummary || '').split('\\n').slice(1, 2).join(' ')), count: v.nodes.length }));
    }""", SKIP_RULES)
    for v in res:
        found.append({"where": where, "theme": theme, **v})
        print(f"{v['impact'] or '?':>8}  {v['id']:<32} {where} [{theme}]  x{v['count']}")
        for n in v["nodes"]:
            print("            " + n[:170])


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        for theme in ["dark", "light", "contrast"]:
            pg = await b.new_page(viewport={"width": 1400, "height": 900})
            await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
            await pg.add_init_script(HOSTS)
            await pg.add_init_script("window.__units = 1;")
            await pg.goto(URL)
            await pg.evaluate("(t) => localStorage.setItem('sshvault.prefs.v1', JSON.stringify({ appTheme: t }))", theme)
            await pg.goto(URL)
            await pg.wait_for_timeout(2500)
            await pg.add_script_tag(content=AXE)

            await scan(pg, "hosts", theme)
            await pg.click('button[aria-label="Manage"]'); await pg.wait_for_timeout(300)
            await scan(pg, "manage menu", theme)
            await pg.keyboard.press("Escape")
            await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(300)
            await scan(pg, "command palette", theme)
            await pg.keyboard.press("Escape")
            await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(200)
            await pg.keyboard.type("Take the tour"); await pg.keyboard.press("Enter"); await pg.wait_for_timeout(500)
            await scan(pg, "the tour", theme)
            await pg.keyboard.press("Escape"); await pg.wait_for_timeout(200)
            await pg.click('button[title="New host"]'); await pg.wait_for_timeout(500)
            await scan(pg, "host form: connection", theme)
            for tab in ["Route", "Organise", "Automation"]:
                await pg.click(f'[role=tab]:has-text("{tab}")'); await pg.wait_for_timeout(200)
                await scan(pg, f"host form: {tab.lower()}", theme)
            await pg.keyboard.press("Escape"); await pg.wait_for_timeout(300)
            # An untouched form may still ask "Discard your changes?".
            if await pg.locator('button:has-text("Discard")').count():
                await pg.click('button:has-text("Discard")'); await pg.wait_for_timeout(200)
            for label, name in [("Snippets", "snippets"), ("Tunnels", "tunnels"), ("Databases", "databases"), ("Containers", "containers"), ("Ops", "operations"), ("Runbooks", "runbooks"), ("Settings", "settings")]:
                await pg.click(f'button[aria-label="{label}"]'); await pg.wait_for_timeout(500)
                await scan(pg, name, theme)
                if name == "runbooks":
                    for tab in ["History", "Schedules"]:
                        await pg.click(f'[role=tab]:has-text("{tab}")'); await pg.wait_for_timeout(300)
                        await scan(pg, f"runbooks: {tab.lower()}", theme)
                    await pg.click('[data-testid=schedule-new]'); await pg.wait_for_timeout(300)
                    await scan(pg, "runbooks: new schedule", theme)
            await pg.click('button[aria-label="Hosts"]')
            await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(200)
            await pg.keyboard.type("Go to Topology"); await pg.keyboard.press("Enter"); await pg.wait_for_timeout(800)
            await scan(pg, "topology", theme)
            await pg.click('button[aria-label="Hosts"]')
            await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(200)
            await pg.keyboard.type("Import hosts from a cloud"); await pg.keyboard.press("Enter"); await pg.wait_for_timeout(500)
            await scan(pg, "import from a cloud", theme)
            await pg.keyboard.press("Escape"); await pg.wait_for_timeout(200)
            await pg.keyboard.press("Control+Shift+Backquote"); await pg.wait_for_timeout(1500)
            await scan(pg, "a terminal tab", theme)
            await pg.close()
        await b.close()
    bad = [f for f in found if f["impact"] in ("serious", "critical")]
    print(f"\n{len(found)} findings, {len(bad)} serious or critical")
    sys.exit(1 if bad else 0)


asyncio.run(main())
