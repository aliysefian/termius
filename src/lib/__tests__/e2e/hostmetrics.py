"""The Host Metrics window in a real (headless) browser with the backend mocked: the tabs, unavailable-not-zero on the
first reading, the network interface selector and units, shared timer, failures, and an unsupported host.

    pnpm dev &
    python3 src/lib/__tests__/e2e/hostmetrics.py [chromium-executable]

SHOT=/some/dir saves a screenshot of each tab. Exits with an error on the first thing that is not as expected.
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
SHOT = os.environ.get("SHOT")

failures = []


def check(ok, what):
    print(("ok    " if ok else "FAIL  ") + what)
    if not ok:
        failures.append(what)


async def reopen(pg):
    """Back to the Fleet page and the host's metrics window, from wherever a link took us."""
    btn = pg.locator('button[aria-label="Open the host metrics of claude"]')
    if await btn.count() == 0:
        await pg.click('button[aria-label="Fleet"]')
        await pg.wait_for_timeout(400)
        if await pg.locator('button[aria-label="Open the full Fleet page"]').count():
            await pg.click('button[aria-label="Open the full Fleet page"]')
            await pg.wait_for_timeout(500)
    await btn.click()
    await pg.wait_for_timeout(800)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1500, "height": 1000})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(path=os.path.join(HERE, "hostmock.js"))
        await pg.goto(URL)
        await pg.wait_for_timeout(9000)
        await pg.click('button[aria-label="Fleet"]')
        await pg.click('button[aria-label="Open the full Fleet page"]')
        await pg.wait_for_timeout(800)
        await pg.click('button[aria-label="Open the host metrics of claude"]')
        modal = pg.locator('[data-testid=host-metrics]')
        await modal.wait_for()
        await pg.wait_for_timeout(600)

        txt = lambda: modal.inner_text()
        # First reading: values that need no baseline are there; rates are unavailable, never zero.
        t = await txt()
        check("Unavailable" in t, "the first reading shows unmeasured figures as Unavailable")
        check("First reading" in t, "and says why")
        check("0.0%" not in t.split("CPU usage")[0], "CPU is not shown as 0% before it can be measured")
        check("9.9 GiB / 16.0 GiB" in t, "memory, which needs no baseline, is shown at once")
        check(await pg.locator('[data-testid=host-metrics] [role=tab][aria-selected=true]').inner_text() == "Overview", "starts on Overview")
        # The window asks for readings a little fresher than its interval, so other views' recent ones are shared.
        check(await pg.evaluate("window.__calls.maxAges[0]") == 1500, "it asks for a reading no older than 1.5 s at the 2 s rate")

        await pg.wait_for_timeout(4500)
        t = await txt()
        check("First reading" not in t, "after the next reading the note is gone")
        check("MB/s" in t and "↓" in t, "download speed appears, in MB/s")
        check(await pg.evaluate("window.__calls.history") == 1, "kept history was loaded once")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "overview.png"))

        # Only one timer: sample calls over 4.5 s at 2 s is about 2-3, not doubled by the several panels.
        c0 = await pg.evaluate("window.__calls.sample")
        await pg.wait_for_timeout(4100)
        c1 = await pg.evaluate("window.__calls.sample")
        check(2 <= c1 - c0 <= 3, f"one timer feeds every panel ({c1 - c0} readings in 4 s at 2 s)")

        for name, needle in [("CPU", "Per core"), ("Memory", "Swap"), ("Disks", "Physical disks"), ("Network", "Interfaces")]:
            await pg.locator(f'[role=tab]:text-is("{name}")').click()
            await pg.wait_for_timeout(400)
            check(needle in await txt(), f"the {name} tab shows its panel")
            if SHOT:
                await pg.screenshot(path=os.path.join(SHOT, f"{name.lower()}.png"))
        # Disks: the two kinds of measure are separate, partitions/layers are not mixed in.
        await pg.locator('[role=tab]:text-is("Disks")').click()
        t = await txt()
        check("how full" in t and "activity" in t, "filesystem fullness and disk activity are separate sections")
        check("dm-0" not in t, "device-mapper layers are hidden by default")
        await pg.locator('label:has-text("Show RAID")').click()
        check("dm-0" in await txt(), "and can be shown")

        # Network: interface selector and units.
        await pg.locator('[role=tab]:text-is("Network")').click()
        await pg.wait_for_timeout(300)
        sel = pg.locator('select[aria-label="Network interface"]')
        opts = await sel.locator("option").all_inner_texts()
        check(opts[0].startswith("All") and any("eth0" in o and "default route" in o for o in opts), "the selector offers the total and the default route")
        await sel.select_option("docker0")
        await pg.wait_for_timeout(300)
        check("Not in the total" in await txt(), "a bridge says why it is not in the total")
        saved = await pg.evaluate("localStorage.getItem('sshvault.hostmetrics.view.v1')")
        check('"h1":"docker0"' in saved, "the chosen interface is remembered for this host")
        await sel.select_option("eth0")
        await pg.locator('button:text-is("Mbps")').click()
        await pg.wait_for_timeout(300)
        t = await pg.locator("[role=tabpanel]").inner_text()
        check("Mbps" in t and "MB/s" not in t, "bits mode writes Mbps, not MB/s")
        await pg.locator('button:text-is("MB/s")').click()
        await pg.locator('button:text-is("RX and TX apart")').click()
        await pg.wait_for_timeout(300)
        check("Download (RX)" in await txt() and "Upload (TX)" in await txt(), "RX and TX can be charted apart")

        # Hover a chart for the tooltip.
        await pg.locator('[role=tab]:text-is("Overview")').click()
        await pg.wait_for_timeout(300)
        svg = pg.locator('figure svg[role=img]').first
        box = await svg.bounding_box()
        await pg.mouse.move(box["x"] + box["width"] * 0.95, box["y"] + box["height"] * 0.5)
        await pg.wait_for_timeout(200)
        check(await pg.locator('[role=status]').count() >= 1, "hovering a chart shows a tooltip")
        await pg.mouse.move(5, 5)

        # A failed reading keeps the last good one on screen and says so.
        await pg.evaluate("window.__fail = { code: 'metrics_exec', message: 'connection refused' }")
        await pg.wait_for_timeout(2600)
        t = await txt()
        check("The last reading failed (connection refused)" in t, "a failure is shown")
        check("Last update" in t and "CPU usage" in t, "while the last good reading stays")
        await pg.evaluate("window.__fail = null")
        await pg.locator('button:has-text("Try again")').click()
        await pg.wait_for_timeout(600)
        check("last reading failed" not in await txt(), "and goes away when a reading works")

        # Live off: no more timer.
        await pg.locator('select[aria-label="Live refresh"]').select_option("0")
        await pg.wait_for_timeout(300)
        c0 = await pg.evaluate("window.__calls.sample")
        await pg.wait_for_timeout(5000)
        check(await pg.evaluate("window.__calls.sample") == c0, "with Live off no readings are taken")
        await pg.locator('select[aria-label="Live refresh"]').select_option("2000")

        # -- temperature, GPU, processes, ports, logins -----------------------------------------------------
        await pg.locator('select[aria-label="Live refresh"]').select_option("2000")
        tabs = await pg.locator('[data-testid=host-metrics] [role=tab]').all_inner_texts()
        check(tabs == ["Overview", "CPU", "Memory", "Disks", "Network", "GPU", "Temperature", "Processes", "Ports", "Logins"], f"a host with a GPU and sensors offers every tab: {tabs}")
        await pg.locator('[role=tab]:text-is("Overview")').click()
        await pg.wait_for_timeout(300)
        t = await txt()
        check("Temperature" in t and "48 °C" in t, "the overview shows the CPU temperature")
        check("NVIDIA GeForce RTX 3060" in t, "and the GPU")
        check(await pg.evaluate("(window.__calls.proc || []).length") == 0, "nothing is read for processes until the Processes tab is open")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "overview2.png"))

        await pg.locator('[role=tab]:text-is("Temperature")').click()
        await pg.wait_for_timeout(300)
        t = await txt()
        cpu_group = t.split("Storage")[0]
        check("CPU temperature" in t and "48 °C" in t, "the Temperature tab gives the CPU figure")
        check("Package id 0" in cpu_group and "Core 0" in cpu_group and "acpitz" not in cpu_group and "Composite" not in cpu_group, "only the processor's own sensors are under Processor")
        check("Composite" in t and "SYSTIN" in t and "Unavailable" in t, "other sensors are listed, an unreadable one as Unavailable")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "temps.png"))

        await pg.locator('[role=tab]:text-is("GPU")').click()
        await pg.wait_for_timeout(300)
        t = await txt()
        check("NVIDIA GeForce RTX 3060" in t and "535.104.05" in t and "62.0 W" in t and "170" in t, "the GPU tab shows the card, driver and power")
        check("python3" in t and "VRAM" in t, "and the programs using it")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "gpu.png"))

        await pg.locator('[role=tab]:text-is("Processes")').click()
        await pg.wait_for_timeout(700)
        pt = pg.locator("[data-testid=process-table]")
        rows = lambda: pt.locator("tbody tr").all_inner_texts()
        r = await rows()
        check(len(r) == 5 and "nginx: worker process" in r[0], f"the busiest process is first ({len(r)} rows)")
        check("—" in r[-1] and "postgres" in r[-1], "a process with no CPU figure is last and shows a dash, not 0")
        await pt.locator('input[aria-label="Search processes"]').fill("php")
        r = await rows()
        check(len(r) == 1 and "php-fpm" in r[0], "search narrows the list")
        await pt.locator('input[aria-label="Search processes"]').fill("")
        await pt.locator('button:has-text("Tree")').click()
        r = await rows()
        check(len(r) == 5 and "/sbin/init" in r[0], "the tree starts at init")
        await pt.locator('button:has-text("Tree")').click()
        await pt.locator('thead button:text-is("PID")').click()
        r = await rows()
        check(r[0].split()[0] == "1", "sorting by PID puts 1 first")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "processes.png"))

        # A signal: asked first, and nothing is sent until it is confirmed.
        await pt.locator('button[aria-label="Terminate nginx 34721"]').click()
        await pg.wait_for_timeout(300)
        dlg = pg.locator("[role=alertdialog]")
        dt = await dlg.inner_text()
        check("nginx (process 34721, user root) on claude" in dt and "SIGTERM" in dt and "still the same program" in dt, "the confirmation names the process, the host and what the signal does")
        await dlg.locator('button:text-is("Cancel")').click()
        check(await pg.evaluate("window.__signals.length") == 0, "cancelling sends nothing")
        await pt.locator('button[aria-label="Terminate nginx 34721"]').click()
        await dlg.locator('button:text-is("Terminate")').click()
        await pg.wait_for_timeout(800)
        sig = await pg.evaluate("window.__signals")
        check(len(sig) == 1 and sig[0]["pid"] == 34721 and sig[0]["startTicks"] == 900 and sig[0]["name"] == "nginx" and sig[0]["signal"] == "TERM", f"the identity (name and start time) goes with the signal: {sig}")
        check("Sent SIGTERM to nginx (34721)" in await pg.locator("body").inner_text(), "the outcome is reported")
        check("34721" not in " ".join(await rows()), "and the list is read again")
        # The host refuses: its words are shown. A reused pid: nothing was sent, and it says so.
        await pg.evaluate("window.__signalOutcome = { kind: 'refused', reason: 'kill: Operation not permitted' }")
        await pt.locator('button[aria-label="Force kill sshd 812"]').click()
        await dlg.locator('button:text-is("Force kill")').click()
        await pg.wait_for_timeout(600)
        check("did not allow it: kill: Operation not permitted" in await pg.locator("body").inner_text(), "a refusal is shown in the host's words")
        await pg.evaluate("window.__signalOutcome = { kind: 'unknown' }")
        await pt.locator('select[aria-label="Other signals for sshd 812"]').select_option("HUP")
        await dlg.locator('button:text-is("Hang up")').click()
        await pg.wait_for_timeout(600)
        check("not sent again" in await pg.locator("body").inner_text(), "a host that does not answer is reported as unknown and not retried")
        check(await pg.evaluate("window.__signals.length") == 3, "each confirmed signal was sent exactly once")

        # Reading stops when the tab is left.
        await pg.locator('[role=tab]:text-is("Ports")').click()
        await pg.wait_for_timeout(500)
        c0 = await pg.evaluate("window.__calls.proc.length")
        await pg.wait_for_timeout(7000)
        check(await pg.evaluate("window.__calls.proc.length") == c0, "leaving the Processes tab stops reading processes")

        pr = pg.locator("[data-testid=port-table]")
        r = await pr.locator("tbody tr").all_inner_texts()
        check(len(r) == 4, f"four listeners are shown ({len(r)})")
        await pr.locator('select[aria-label="Scope"]').select_option("local")
        check(len(await pr.locator("tbody tr").all_inner_texts()) == 1, "the scope filter keeps only loopback listeners")
        await pr.locator('select[aria-label="Scope"]').select_option("all")
        await pr.locator('select[aria-label="Protocol"]').select_option("udp")
        check(len(await pr.locator("tbody tr").all_inner_texts()) == 1, "the protocol filter works")
        await pr.locator('select[aria-label="Protocol"]').select_option("all")
        await pr.locator('select[aria-label="Process"]').select_option("nginx")
        check(len(await pr.locator("tbody tr").all_inner_texts()) == 1, "and the process filter")
        await pr.locator('select[aria-label="Process"]').select_option("")
        pt2 = await pr.inner_text()
        check("owner not shown" in pt2 and "Listening is not the same as reachable" in pt2 and "needs root" in pt2, "missing owners and what 'listening' means are explained")
        check(await pr.locator('button[aria-label="Create an SSH tunnel to port 51820"]').count() == 0, "a UDP port offers no SSH tunnel")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "ports.png"))
        await pr.locator('button[aria-label="Create an SSH tunnel to port 5432"]').click()
        await pg.wait_for_timeout(600)
        vals = await pg.evaluate("[...document.querySelectorAll('[role=dialog] input')].map((i) => i.value)")
        check(any(v.startswith("claude :5432 (postgres)") for v in vals) and vals.count("5432") >= 2 and "127.0.0.1" in vals, f"the tunnel form opens filled in for that port: {vals}")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        # The form is prefilled, so closing it asks before discarding.
        try:
            await pg.locator('[role=alertdialog] button:text-is("Discard")').click(timeout=3000)
        except Exception:
            pass
        await pg.wait_for_timeout(400)
        if await pg.locator('[data-testid=host-metrics]').count() == 0:
            await reopen(pg)
        await pg.locator('[role=tab]:text-is("Logins")').click()
        await pg.wait_for_timeout(700)
        lv = pg.locator("[data-testid=logins-view]")
        t = await lv.inner_text()
        check("Current sessions" in t and "Recent logins" in t and "2m ago" in t and "1h 22m ago" in t, "sessions and recent logins are shown with how long ago")
        check("Failed login attempts" in t and "only root can read" in t, "what could not be read is explained")
        check("Failed attempts by source" in t and "185.211.57.152" in t and "× 2" in t, "failed attempts are counted by source")
        check("Nothing here is marked suspicious" in t, "and nothing is labelled suspicious")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "logins.png"))
        await lv.locator('button:has-text("SSH logs")').click()
        await pg.wait_for_timeout(800)
        check(await pg.locator("[data-testid=operations]").count() == 1, "SSH logs opens Operations → Logs")
        await reopen(pg)

        # An unsupported host.
        await pg.locator('select[aria-label="Host"]').select_option("h2")
        await pg.wait_for_timeout(1200)
        check("not available for Darwin" in await txt(), "a non-Linux host says it is unsupported, with no made-up figures")
        await pg.locator('select[aria-label="Host"]').select_option("h1")
        await pg.wait_for_timeout(800)
        check("CPU usage" in await txt(), "switching back to a Linux host shows its metrics")

        # Keyboard: arrow keys move between tabs.
        await pg.locator('[role=tab][aria-selected=true]').focus()
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(200)
        check(await pg.locator('[data-testid=host-metrics] [role=tab][aria-selected=true]').inner_text() == "CPU", "arrow keys move between tabs")

        # Closing it stops the timer.
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(500)
        c0 = await pg.evaluate("window.__calls.sample")
        await pg.wait_for_timeout(4500)
        check(await pg.evaluate("window.__calls.sample") == c0, "closing the window stops reading")
        check(not errors, f"no page errors {errors}")
        await b.close()
    if failures:
        print(f"\n{len(failures)} failed")
        sys.exit(1)
    print("\nall passed")


asyncio.run(main())
