"""The terminal's metrics toolbar and side panel in a real (headless) browser with the backend mocked: that nothing is
read until asked, that the panel opens, moves, resizes and closes WITHOUT touching the terminal or its connection, that
readings are shared and slow down when the panel is closed, and that the choices are remembered.

    pnpm dev &
    python3 src/lib/__tests__/e2e/terminalmetrics.py [chromium-executable]

SHOT=/some/dir saves a screenshot. Exits with an error on the first thing that is not as expected.
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


async def open_host(pg, label):
    # The rail's Hosts button hides the list when it is already showing, so only press it when the list is not there.
    if await pg.locator("[role=treeitem]").count() == 0:
        await pg.click('button[aria-label="Hosts"]')
        await pg.wait_for_timeout(500)
    row = pg.locator(f'[role=treeitem]:has-text("{label}")').first
    await row.hover()
    await row.locator('button[aria-label="Connect"]').click()
    await pg.wait_for_timeout(1500)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1500, "height": 900})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(path=os.path.join(HERE, "hostmock.js"))
        await pg.goto(URL)
        await pg.wait_for_timeout(9000)

        calls = lambda k: pg.evaluate(f"window.__calls.{k}")
        toggle = pg.locator("[data-testid=term-metrics-toggle]")
        panel = pg.locator("[data-testid=term-metrics-panel]")
        chips = pg.locator("[data-testid=term-metrics-chips]")

        # A host without saved credentials can't be read unattended: no control at all.
        await open_host(pg, "no-creds")
        check(await toggle.count() == 0, "a host with no saved credentials has no metrics control")
        await pg.keyboard.press("Control+Shift+W")
        await pg.wait_for_timeout(500)

        await open_host(pg, "claude")
        await pg.wait_for_timeout(500)
        check(await toggle.count() == 1, "a saved SSH host offers the metrics control")
        check(await panel.count() == 0 and await chips.count() == 0, "nothing is shown until it is asked for")
        await pg.wait_for_timeout(3000)
        check(await calls("sample") == 0, "and nothing is read from the host")

        # Mark the terminal and count connections: opening the panel must touch neither.
        await pg.evaluate("document.querySelector('.xterm').__mark = 'same'")
        connects = await calls("connects")
        check(connects == 1, f"one SSH connection for the terminal ({connects})")

        await toggle.click()
        await pg.wait_for_timeout(500)
        check(await panel.count() == 1, "the panel opens beside the terminal")
        check(await chips.count() == 1, "and the toolbar appears in the status bar")
        check(await pg.evaluate("document.querySelector('.xterm').__mark") == "same", "the terminal is the same element: it was not rebuilt")
        check(await calls("connects") == connects, "no new SSH connection for the terminal")
        await pg.wait_for_timeout(4500)
        t = await panel.inner_text()
        check("CPU" in t and "Memory" in t and "Disk (/)" in t and "RX" in t and "TX" in t, "the panel shows CPU, memory, disk and network RX/TX")
        check("MB/s" in t, "network speeds are in MB/s")
        check("Temperature" in t and "48 °C" in t, "the panel shows the CPU temperature")
        check("NVIDIA GeForce RTX 3060" in t, "and the GPU")
        check("Top processes" in t and "nginx" in t and "18.4%" in t and "sshd" in t, "and the top processes")
        check(await pg.evaluate("window.__calls.proc.length") >= 1, "the process list is read while the panel is open")
        ct = await chips.inner_text()
        check("CPU" in ct and "RAM" in ct and "Disk" in ct, "the toolbar shows CPU, RAM and Disk")
        check("%" in ct and "Unavailable" not in ct, "with figures once they are measured")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "term-panel.png"))

        # One timer: at the 2 s live rate there are 2-3 readings in 4 s, whether the toolbar and panel both show them.
        c0 = await calls("sample")
        await pg.wait_for_timeout(4100)
        n = await calls("sample") - c0
        check(2 <= n <= 3, f"the toolbar and panel share one reading timer ({n} readings in 4 s)")
        check(await calls("history") == 1, "kept history was loaded once")

        # Typing still reaches the host while the panel is open.
        await pg.click(".xterm")
        await pg.keyboard.type("hi")
        await pg.wait_for_timeout(300)
        sent = await pg.evaluate("window.__sent.flat()")
        check(104 in sent and 105 in sent, "typing in the terminal still works with the panel open")

        # Resize by keyboard and by dragging; remembered.
        handle = panel.locator('[role=separator]')
        w0 = int(await handle.get_attribute("aria-valuenow"))
        await handle.focus()
        await pg.keyboard.press("ArrowLeft")
        w1 = int(await handle.get_attribute("aria-valuenow"))
        check(w1 == w0 + 16, f"the arrow toward the terminal widens the panel ({w0} to {w1})")
        box = await handle.bounding_box()
        await pg.mouse.move(box["x"] + 1, box["y"] + 300)
        await pg.mouse.down()
        await pg.mouse.move(box["x"] - 79, box["y"] + 300, steps=5)
        await pg.mouse.up()
        w2 = int(await handle.get_attribute("aria-valuenow"))
        check(w2 == w1 + 80, f"dragging the edge resizes it ({w1} to {w2})")
        check(abs((await panel.bounding_box())["width"] - w2) <= 1, "to the width it says")
        await handle.dblclick()
        check(int(await handle.get_attribute("aria-valuenow")) == 288, "double-click resets the width")
        await handle.focus()
        for _ in range(40):
            await pg.keyboard.press("Shift+ArrowLeft")
        check(int(await handle.get_attribute("aria-valuenow")) <= 520, "the width has an upper limit")
        await handle.dblclick()

        # Move to the left; the terminal is still the same element and the layout puts the panel first.
        await panel.locator('[aria-label="Move the panel to the left"]').click()
        await pg.wait_for_timeout(300)
        px = (await panel.bounding_box())["x"]
        tx = (await pg.locator(".xterm").first.bounding_box())["x"]
        check(px < tx, "the panel is on the left of the terminal")
        check(await pg.evaluate("document.querySelector('.xterm').__mark") == "same", "moving it did not rebuild the terminal")
        await panel.locator('[aria-label="Move the panel to the right"]').click()
        await pg.wait_for_timeout(300)

        # Close the panel: the toolbar stays, and reading slows to the toolbar's own rate.
        await panel.locator('[aria-label="Close the metrics panel"]').click()
        await pg.wait_for_timeout(500)
        check(await panel.count() == 0 and await chips.count() == 1, "closing the panel leaves the toolbar")
        check(await pg.evaluate("document.querySelector('.xterm').__mark") == "same" and await calls("connects") == connects, "closing it did not touch the terminal or its connection")
        await pg.wait_for_timeout(1000)
        c0 = await calls("sample")
        await pg.wait_for_timeout(11000)
        n = await calls("sample") - c0
        check(1 <= n <= 3, f"with the panel closed readings slow to the toolbar rate ({n} in 11 s)")

        c0 = await pg.evaluate("window.__calls.proc.length")
        await pg.wait_for_timeout(7000)
        check(await pg.evaluate("window.__calls.proc.length") == c0, "with the panel closed the processes are not read")

        # Reopen: remembered side and width; stored per host.
        await toggle.click()
        await pg.wait_for_timeout(400)
        check(await panel.count() == 1, "reopens")
        stored = await pg.evaluate("localStorage.getItem('sshvault.hostmetrics.terminal.v1')")
        check('"h1":{"enabled":true,"panel":true}' in stored and '"side":"right"' in stored, "the choice is stored for this host only")

        # The panel is not shown while another page is.
        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_timeout(500)
        check(await panel.count() == 0, "the panel is not drawn behind another page")
        c0 = await calls("sample")
        await pg.wait_for_timeout(11000)
        check(await calls("sample") - c0 <= 3, "and live reading is paused while it is hidden")
        await pg.click('button[aria-label="Hosts"]')
        await pg.wait_for_timeout(800)
        if await panel.count() == 0:
            await pg.click('button[aria-label="Hosts"]')
            await pg.wait_for_timeout(800)
        check(await panel.count() == 1, "it is back when the terminal is")
        check(await pg.evaluate("document.querySelector('.xterm').__mark") == "same", "the terminal was never rebuilt")

        # Turn off: no more readings.
        await panel.locator('button:has-text("Turn off")').click()
        await pg.wait_for_timeout(500)
        check(await panel.count() == 0 and await chips.count() == 0, "turning it off removes the panel and toolbar")
        c0 = await calls("sample")
        await pg.wait_for_timeout(6500)
        check(await calls("sample") == c0, "and stops reading the host")
        check(await calls("connects") == connects, "the terminal's connection was never touched")

        # A failed reading: the toolbar says so, with the reason.
        await toggle.click()
        await pg.wait_for_timeout(2500)
        await pg.evaluate("window.__fail = { code: 'metrics_exec', message: 'connection refused' }")
        await pg.locator('[aria-label="Refresh now"]').click()
        await pg.wait_for_timeout(500)
        check("last reading failed (connection refused)" in await panel.inner_text(), "a failed reading is shown in the panel, with the last good one kept")
        await pg.evaluate("window.__fail = null")

        check(not errors, f"no page errors {errors}")
        await b.close()
    if failures:
        print(f"\n{len(failures)} failed")
        sys.exit(1)
    print("\nall passed")


asyncio.run(main())
