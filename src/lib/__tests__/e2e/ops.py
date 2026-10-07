"""Operations (logs and services) in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/ops.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

HOSTS = """
const mk = (id, label, env) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', environment: env, identity_id: 'i1' } });
const prev = window.__invoke;
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return [mk('h1', 'web-01', 'production'), mk('h2', 'db-01', 'staging')];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  return undefined;
};
window.__unitsOut = `  nginx.service   loaded active   running A high performance web server
  cron.service    loaded inactive dead    Regular background processing
● backup.service  loaded failed   failed  Nightly backup
@@FILES
nginx.service enabled enabled
cron.service disabled enabled
backup.service enabled enabled
`;
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 1000})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(HOSTS)
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)

        # the rail has an entry for it, and the page opens
        await pg.click('button[aria-label="Ops"]')
        await pg.wait_for_timeout(500)
        check("Ops is on the rail and opens the Operations page", await pg.locator('h1:has-text("Operations")').count() >= 1)

        # ---- logs ----
        boxes = pg.locator('[role=group][aria-label=Hosts] input[type=checkbox]')
        check("hosts with saved credentials are listed", await boxes.count() == 2)
        check("Follow is off until a host is chosen", await pg.locator('button:has-text("Follow")').is_disabled())
        await boxes.nth(0).check()
        await boxes.nth(1).check()
        await pg.fill("#lg-unit", "nginx.service")
        await pg.select_option("#lg-prio", "warning")
        await pg.click('button:has-text("Follow")')
        await pg.wait_for_timeout(900)
        streams = await pg.evaluate("window.__streams")
        check("one stream per host, running the journal script for the unit and priority", len(streams) == 2 and all("journalctl --follow" in s and "-u 'nginx.service'" in s and "-p warning" in s for s in streams), streams)
        text = await pg.inner_text('[role=log]')
        check("lines from both hosts arrive whole (a line split across chunks is joined)", text.count("ERROR database timeout") == 2 and text.count("started") == 2, text[:300])
        check("with several hosts each line is tagged", "web-01" in text and "db-01" in text)

        await pg.fill('input[aria-label="Filter lines"]', "error")
        await pg.wait_for_timeout(300)
        shown = await pg.locator('[role=log] > div').count()
        marks = await pg.locator('[role=log] mark').count()
        check("the filter keeps only matching lines and marks the match", shown == 2 and marks == 2, (shown, marks))
        await pg.fill('input[aria-label="Filter lines"]', "")
        await pg.click('button[aria-pressed=false]:has-text("warn")')
        await pg.wait_for_timeout(300)
        check("a level button keeps only that level", await pg.locator('[role=log] > div').count() == 2)
        await pg.click('button[aria-pressed=true]:has-text("warn")')

        await pg.click('button[title^="Pause the view"]')
        n_before = await pg.locator('[role=log] > div').count()
        check("pausing freezes the view", n_before >= 4)
        await pg.click('button[title="Resume scrolling"]')

        await pg.click('button:has-text("Stop")')
        await pg.wait_for_timeout(500)
        check("Stop ends the streams and closes the connections", len(await pg.evaluate("window.__streams")) == 2 and await pg.locator('button:has-text("Follow")').count() == 1)

        await pg.select_option("#lg-kind", "file")
        await pg.fill("#lg-path", "relative.log")
        check("a file path that is not absolute is refused", await pg.locator("text=must be absolute").count() == 1 and await pg.locator('button:has-text("Follow")').is_disabled())
        await pg.fill("#lg-path", "/var/log/it's here.log")
        await pg.click('button:has-text("Follow")')
        await pg.wait_for_timeout(500)
        last = (await pg.evaluate("window.__streams"))[-1]
        check("a file is followed with its path as one quoted word", last == "exec tail -n 100 -F -- '/var/log/it'\\''s here.log'", last)
        await pg.click('button:has-text("Stop")')

        # ---- services ----
        await pg.click('[role=tab]:has-text("Services")')
        await pg.wait_for_timeout(300)
        await pg.select_option("#sv-host", "h2")
        await pg.wait_for_timeout(700)
        rows = pg.locator("tbody tr")
        check("the services are listed with state and boot setting", await rows.count() == 3 and "failed" in await pg.inner_text("tbody") and "disabled" in await pg.inner_text("tbody"))
        check("a failed service is marked", await pg.locator("td.text-danger").count() == 1)
        await pg.fill('input[aria-label="Search services"]', "ngi")
        check("search narrows the list", await rows.count() == 1)
        await pg.fill('input[aria-label="Search services"]', "")
        await pg.click('tbody tr:has-text("nginx") button:has-text("restart")')
        await pg.wait_for_timeout(500)
        execs = await pg.evaluate("window.__execs")
        check("restart runs one quoted systemctl command and reloads the list", any(e == "systemctl restart -- 'nginx.service' 2>&1" for e in execs) and sum("list-units" in e for e in execs) >= 2, execs[-3:])
        await pg.check('text=Use sudo')
        await pg.click('tbody tr:has-text("cron") button:has-text("start")')
        await pg.wait_for_timeout(400)
        check("with sudo on, the command uses sudo -n", any(e == "sudo -n systemctl start -- 'cron.service' 2>&1" for e in await pg.evaluate("window.__execs")))
        await pg.evaluate("window.__actionCode = 1")
        await pg.click('tbody tr:has-text("cron") button:has-text("enable")')
        await pg.wait_for_timeout(500)
        check("a failed action says so", await pg.locator("text=enable cron.service").count() >= 1)

        # production asks for the host's name before stopping
        await pg.select_option("#sv-host", "h1")
        await pg.wait_for_timeout(700)
        n = len(await pg.evaluate("window.__execs"))
        await pg.click('tbody tr:has-text("nginx") button:has-text("stop")')
        await pg.wait_for_timeout(400)
        check("stopping on a production host asks first, and runs nothing until it is answered", await pg.locator('input[placeholder], [role=dialog] input').count() >= 1 and len(await pg.evaluate("window.__execs")) == n)
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(200)
        check("cancelling runs nothing", not any("stop" in e and "nginx" in e for e in (await pg.evaluate("window.__execs"))[n:]))

        await pg.click('tbody tr:has-text("nginx") button[aria-label^="Logs of"]')
        await pg.wait_for_timeout(500)
        check("Follow its log jumps to Logs with the host and unit filled in", await pg.input_value("#lg-unit") == "nginx.service")

        # ---- alerts ----
        await pg.click('[role=tab]:has-text("Alerts")')
        await pg.wait_for_timeout(300)
        check("Alerts says they are off, and where to turn them on", await pg.locator("text=Alerts are").count() == 1 and await pg.locator('button:has-text("Settings")').count() == 1)

        check("no page errors", not errors, errors[:3])
        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
