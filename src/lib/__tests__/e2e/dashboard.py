"""The dashboard's Host Status table in a real (headless) browser with the backend mocked: the monitoring switch on
each host, and the table's filters and sorting.

    pnpm dev &
    python3 src/lib/__tests__/e2e/dashboard.py [chromium-executable]

Exits with an error on the first thing that is not as expected.
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
SHOT = os.environ.get("SHOT")
# dark (default), light or contrast: the theme to draw the screenshot in.
THEME = os.environ.get("THEME")

# web-01 and web-02 are monitored from the start; db-01 has no credentials; lab-box is reachable only through a jump host.
SETUP = """
localStorage.setItem('sshvault.monitored.v1', JSON.stringify({ h1: true, h2: true }));
const mk = (id, label, group, tags, identity) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group, tags, notes: '', identity_id: identity } });
const LOAD = { h1: 'CPU=20\\nMEM=90\\nDISK=40', h2: 'CPU=85\\nMEM=10\\nDISK=40', h4: 'CPU=55\\nMEM=60\\nDISK=95' };
window.__runs = 0;
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return [
    mk('h1', 'web-01', 'Production/Web', ['nginx'], 'i1'),
    mk('h2', 'web-02', 'Production/Web', [], 'i1'),
    mk('h3', 'db-01', 'Production/Db', ['postgres'], null),
    mk('h4', 'cache-01', 'Staging', [], 'i1'),
    mk('h5', 'lab-box', '', [], 'i1'),
  ];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  if (cmd === 'check_hosts') return [
    { host_id: 'h1', state: 'up', latency_ms: 12 },
    { host_id: 'h2', state: 'up', latency_ms: 30 },
    { host_id: 'h3', state: 'down', reason: 'connection refused' },
    { host_id: 'h4', state: 'up', latency_ms: 8 },
  ];
  if (cmd === 'run_on_hosts') {
    window.__runs++;
    for (const job of args.jobs) {
      args.onEvent.onmessage?.({ event: 'finished', host_id: job.host_id, output: { stdout: LOAD[job.host_id] ?? 'CPU=1\\nMEM=1\\nDISK=1', stderr: '', code: 0 } });
    }
    return undefined;
  }
  return undefined;
};
"""

failures = []


def check(ok, what):
    print(("ok    " if ok else "FAIL  ") + what)
    if not ok:
        failures.append(what)


async def order(pg):
    return await pg.locator("tbody tr td:first-child button span.truncate").all_inner_texts()


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 1000})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(SETUP)
        await pg.goto(URL)
        if THEME:
            await pg.evaluate("(t) => localStorage.setItem('sshvault.prefs.v1', JSON.stringify({ appTheme: t }))", THEME)
            await pg.goto(URL)
        await pg.wait_for_timeout(9000)
        await pg.click('button[aria-label="Dashboard"]')
        await pg.wait_for_selector("tbody tr")
        await pg.wait_for_timeout(1500)

        # -- the monitoring switch -------------------------------------------------------------
        sw = lambda label: pg.locator(f'button[role=switch][aria-label="Monitor {label}"]')
        check(await order(pg) == ["cache-01", "db-01", "lab-box", "web-01", "web-02"], "starts sorted by name")
        check(await sw("web-01").get_attribute("aria-checked") == "true", "a monitored host's switch is on")
        check(await sw("cache-01").get_attribute("aria-checked") == "false", "an unmonitored host's switch is off")
        check(await sw("db-01").is_disabled(), "a host with no credentials can't be switched on")
        row = lambda label: pg.locator("tbody tr", has_text=label)
        check(await row("web-01").locator("[role=progressbar]").count() == 3, "a monitored host shows its three bars")
        check("Monitoring is off" in await row("cache-01").inner_text(), "an unmonitored host says monitoring is off")
        check("Needs saved credentials" in await row("db-01").inner_text(), "no credentials is explained")

        await sw("cache-01").click()
        await pg.wait_for_timeout(600)
        check(await sw("cache-01").get_attribute("aria-checked") == "true", "switching on turns it on")
        check(await row("cache-01").locator("[role=progressbar]").count() == 3, "and its bars appear once it is read")
        stored = json.loads(await pg.evaluate("localStorage.getItem('sshvault.monitored.v1')"))
        check(stored.get("h4") is True, "the choice is remembered")

        await sw("web-01").click()
        await pg.wait_for_timeout(300)
        check(await sw("web-01").get_attribute("aria-checked") == "false", "switching off turns it off")
        check(await row("web-01").locator("[role=progressbar]").count() == 0, "and its old reading is not left on screen")
        check("Monitoring is off" in await row("web-01").inner_text(), "and it says so")
        await sw("web-01").click()
        await pg.wait_for_timeout(600)
        check(await row("web-01").locator("[role=progressbar]").count() == 3, "switching back on reads it again")

        # -- sorting -----------------------------------------------------------------------------
        th = lambda name: pg.locator(f"thead th:has(button:text-is('{name}'))")
        head = lambda name: pg.locator(f"thead button:text-is('{name}')")
        check(await th("Host").get_attribute("aria-sort") == "ascending", "the Host column is marked ascending")
        await head("CPU").click()
        check(await order(pg) == ["web-02", "cache-01", "web-01", "db-01", "lab-box"], "CPU sorts highest first, hosts with no reading last")
        check(await th("CPU").get_attribute("aria-sort") == "descending", "and is marked descending")
        check(await th("Host").get_attribute("aria-sort") == "none", "the old column is not marked")
        await head("CPU").click()
        check(await order(pg) == ["web-01", "cache-01", "web-02", "db-01", "lab-box"], "a second click reverses it, still with no reading last")
        await head("Disk").click()
        check((await order(pg))[0] == "cache-01", "Disk sorts highest first")
        await head("Status").click()
        check(await order(pg) == ["cache-01", "web-01", "web-02", "db-01", "lab-box"], "Status puts online first, then offline, then unknown")
        await head("Address").click()
        check((await order(pg))[0] == "cache-01", "Address sorts")
        await head("Host").click()
        await head("Host").click()
        check(await order(pg) == ["web-02", "web-01", "lab-box", "db-01", "cache-01"], "Host reverses by name")
        await head("Host").click()

        # -- filtering -----------------------------------------------------------------------------
        count = pg.locator("[aria-live=polite]")
        check("5 hosts" in await count.inner_text(), "the count says how many hosts there are")
        box = pg.get_by_label("Filter hosts by name, address, group or tag")
        await box.fill("web")
        check(await order(pg) == ["web-01", "web-02"], "text finds hosts by name")
        check("2 of 5 hosts" in await count.inner_text(), "and the count says how many are shown")
        await box.fill("postgres")
        check(await order(pg) == ["db-01"], "text finds hosts by tag")
        await box.fill("production")
        check(await order(pg) == ["db-01", "web-01", "web-02"], "text finds hosts by group")
        await pg.get_by_role("button", name="Clear", exact=True).click()
        check(await box.input_value() == "" and len(await order(pg)) == 5, "Clear empties the filters")

        await pg.get_by_label("Filter by status").select_option("offline")
        check(await order(pg) == ["db-01"], "status: offline")
        await pg.get_by_label("Filter by status").select_option("unknown")
        check(await order(pg) == ["lab-box"], "status: unknown")
        await pg.get_by_label("Filter by status").select_option("all")
        await pg.get_by_label("Filter by monitoring").select_option("on")
        check(await order(pg) == ["cache-01", "web-01", "web-02"], "monitoring: monitored")
        await pg.get_by_label("Filter by monitoring").select_option("off")
        check(await order(pg) == ["db-01", "lab-box"], "monitoring: not monitored")
        await pg.get_by_label("Filter by monitoring").select_option("all")
        await pg.get_by_label("Filter by group").select_option("Production")
        check(await order(pg) == ["db-01", "web-01", "web-02"], "a group takes in the groups inside it")
        await pg.get_by_label("Filter by group").select_option("Production/Web")
        check(await order(pg) == ["web-01", "web-02"], "a nested group")

        await pg.get_by_label("Filter by status").select_option("offline")
        check(await pg.get_by_text("No hosts match these filters.").count() == 1, "filters that match nothing say so")
        check("0 of 5 hosts" in await count.inner_text(), "with a count of none")
        await pg.get_by_role("button", name="Clear filters").click()
        check(len(await order(pg)) == 5, "and offer a way out")

        # Switching a host off while the list is filtered to monitored hosts takes it out of the list.
        await pg.get_by_label("Filter by monitoring").select_option("on")
        await sw("web-02").click()
        await pg.wait_for_timeout(200)
        check("web-02" not in await order(pg), "a host switched off leaves the monitored list")

        # The tiles count every host, whatever the table shows.
        await pg.get_by_label("Filter by status").select_option("offline")
        tiles = await pg.locator("text=Hosts Available").locator("xpath=../..").inner_text()
        check("/ 5" in tiles, "the tiles still count all the hosts when the table is filtered")

        if SHOT:
            await pg.get_by_role("button", name="Clear", exact=True).click()
            await pg.get_by_label("Filter by monitoring").select_option("all")
            await head("CPU").click()
            await pg.screenshot(path=SHOT, full_page=False)
        check(not errors, f"no page errors {errors[:2]}")
        await b.close()
    print(f"\n{len(failures)} failed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
