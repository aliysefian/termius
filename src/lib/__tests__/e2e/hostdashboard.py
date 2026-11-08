"""The customizable host dashboard in a real (headless) browser with the backend mocked: moving, resizing, hiding and
changing widgets by keyboard, pointer and drag; undo and redo; saving per host; templates; and that nothing but the layout
is touched.

    pnpm dev &
    python3 src/lib/__tests__/e2e/hostdashboard.py [chromium-executable]

SHOT=/some/dir saves screenshots. Exits with an error on the first thing that is not as expected.
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
SHOT = os.environ.get("SHOT")
DASH = "sshvault.hostmetrics.dashboards.v1"

failures = []


def check(ok, what):
    print(("ok    " if ok else "FAIL  ") + what)
    if not ok:
        failures.append(what)


async def open_metrics(pg, label):
    btn = pg.locator(f'button[aria-label="Open the host metrics of {label}"]')
    if await btn.count() == 0:
        await pg.click('button[aria-label="Fleet"]')
        await pg.wait_for_timeout(400)
        if await pg.locator('button[aria-label="Open the full Fleet page"]').count():
            await pg.click('button[aria-label="Open the full Fleet page"]')
            await pg.wait_for_timeout(500)
    await btn.click()
    await pg.wait_for_selector("[data-testid=dashboard-grid] [data-cell]")
    await pg.wait_for_timeout(500)


async def templates_menu(pg):
    """The Templates menu open (it stays open after an action in it, so only open it when it is shut)."""
    if await pg.locator("[role=menu][aria-label=Templates]").count() == 0:
        await pg.get_by_role("button", name="Templates").click()


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

        keys = lambda: pg.evaluate("[...document.querySelectorAll('[data-testid=dashboard-grid] [data-cell]')].map((c) => c.dataset.key)")
        size = lambda k: pg.evaluate("(k) => { const c = document.querySelector(`[data-cell][data-key=\"${k}\"]`); return [c.style.getPropertyValue('--w'), c.style.getPropertyValue('--h')].join('x'); }", k)
        stored = lambda: pg.evaluate("(k) => JSON.parse(localStorage.getItem(k) || 'null')", DASH)
        grip = lambda k: pg.locator(f"#grip-{k}")

        await open_metrics(pg, "claude")
        ks = await keys()
        check(ks == ["cpu-1", "memory-1", "filesystems-1", "network-1", "gpu-1", "temperature-1", "cpu-2", "network-2", "memory-2", "filesystems-2", "system-1", "diskio-1", "uptime-1"], f"the default layout is shown: {ks}")
        st0 = await stored()
        check(st0 is None or st0["hosts"] == {}, "and nothing is saved for a host that has not been customised")
        sizes = [await size("cpu-1"), await size("cpu-2")]
        check(sizes == ["2x2", "6x4"], f"widgets have their default sizes {sizes}")
        check(await pg.locator("[data-widget=cpu][data-view=card]").count() == 1 and await pg.locator("[data-widget=cpu][data-view=chart]").count() == 1, "a CPU card and a CPU chart")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "dash-default.png"))

        # -- editing --------------------------------------------------------------------------------------------------------------
        await pg.click("[data-testid=customise]")
        await pg.wait_for_timeout(300)
        check(await pg.locator('select[aria-label="Host"]').is_disabled(), "the host cannot be changed while editing")
        check(await pg.locator('[data-testid=host-metrics] [role=tab]:text-is("CPU")').is_disabled(), "nor the tab")
        check(await grip("cpu-1").count() == 1, "every widget has a grip")

        # keyboard: arrows move, shift+arrows resize, and the grip keeps the focus
        await grip("cpu-1").focus()
        await pg.keyboard.press("ArrowRight")
        await pg.wait_for_timeout(200)
        ks = await keys()
        check(ks[:2] == ["memory-1", "cpu-1"], f"ArrowRight moves a widget one place later: {ks[:3]}")
        check(await pg.evaluate("document.activeElement?.id") == "grip-cpu-1", "and the grip keeps the keyboard focus")
        check("moved to position 2 of 13" in await pg.locator("[role=status]").last.inner_text(), "the move is announced")
        await pg.keyboard.press("Shift+ArrowRight")
        await pg.keyboard.press("Shift+ArrowDown")
        check(await size("cpu-1") == "3x3", f"Shift+arrows resize by one column and one row ({await size('cpu-1')})")
        await pg.keyboard.press("Shift+ArrowLeft")
        await pg.keyboard.press("Shift+ArrowLeft")
        await pg.keyboard.press("Shift+ArrowLeft")
        check(await size("cpu-1") == "2x3", "a widget cannot be made smaller than it can show")

        # undo and redo
        undo, redo = pg.locator('button[aria-label="Undo"]'), pg.locator('button[aria-label="Redo"]')
        check(await undo.is_enabled() and await redo.is_disabled(), "undo is on and redo is off after edits")
        for _ in range(4):
            await undo.click()
        check(await keys() == ["cpu-1", "memory-1", "filesystems-1", "network-1", "gpu-1", "temperature-1", "cpu-2", "network-2", "memory-2", "filesystems-2", "system-1", "diskio-1", "uptime-1"] and await size("cpu-1") == "2x2", "undo takes every edit back")
        check(await undo.is_disabled() and await redo.is_enabled(), "and then redo is on")
        await redo.click()
        check((await keys())[:2] == ["memory-1", "cpu-1"], "redo puts the first one again")
        await undo.click()

        # hide and show
        await pg.locator('button[aria-label="Hide Uptime"]').click()
        check("uptime-1" not in await keys(), "a widget can be hidden")
        await pg.get_by_role("button", name="Widgets").click()
        menu = pg.locator('[role=menu][aria-label=Widgets]')
        mt = await menu.inner_text()
        mt = mt.lower()
        check("hidden" in mt and "uptime" in mt and "top processes" in mt and "reads the host while shown" in mt, "hidden widgets are listed, and the ones that read the host say so")
        await menu.locator('button:has-text("Uptime")').first.click()
        check("uptime-1" in await keys(), "and shown again")
        check(await pg.evaluate("(window.__calls.proc || []).length") == 0, "a widget that reads the host reads nothing while it is hidden")

        # change the presentation
        await pg.locator('select[aria-label="How CPU is shown"]').first.select_option("cores")
        check(await pg.locator("[data-widget=cpu][data-view=cores]").count() == 1 and "Core 0" in await pg.locator("[data-widget=cpu][data-view=cores]").inner_text(), "a widget can be shown another way")
        await undo.click()

        # add widgets: another instance, and one that reads the host
        await pg.get_by_role("button", name="Widgets").click()
        await menu.locator('button:has-text("Uptime")').last.click()
        check((await keys())[-1] == "uptime-2", "another widget of a kind can be added")
        await pg.get_by_role("button", name="Widgets").click()
        await menu.locator('button:has-text("Top processes")').first.click()
        await pg.wait_for_timeout(1500)
        check("processes-1" in await keys() and await pg.evaluate("(window.__calls.proc || []).length") >= 1, "showing Top processes starts reading the processes")

        # drag by the grip
        before = await keys()
        await grip("network-2").drag_to(pg.locator('[data-cell][data-key="cpu-2"]'))
        await pg.wait_for_timeout(300)
        after = await keys()
        check(after.index("network-2") < after.index("cpu-2") and after != before, f"dragging a grip onto a widget moves it there: {after[6:9]}")

        # resize by the corner
        w0 = (await size("cpu-2")).split("x")
        hdl = await pg.locator('[data-testid="resize-cpu-2"]').bounding_box()
        grid = await pg.locator("[data-testid=dashboard-grid]").bounding_box()
        step = (grid["width"] + 12) / 12
        await pg.mouse.move(hdl["x"] + 6, hdl["y"] + 6)
        await pg.mouse.down()
        await pg.mouse.move(hdl["x"] + 6 + step * 2, hdl["y"] + 6 + 80, steps=6)
        await pg.mouse.up()
        w1 = (await size("cpu-2")).split("x")
        check(int(w1[0]) == int(w0[0]) + 2 and int(w1[1]) >= int(w0[1]) + 1, f"dragging the corner resizes in whole columns and rows ({w0} to {w1})")
        await undo.click()
        check(await size("cpu-2") == "x".join(w0), "and the whole drag is one undo step")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "dash-edit.png"))

        # nothing is saved until Save; Cancel asks first
        st0 = await stored()
        check(st0 is None or st0["hosts"] == {}, "nothing is saved while editing")
        await pg.get_by_role("button", name="Cancel").click()
        await pg.wait_for_timeout(300)
        check("Discard the changes" in await pg.locator("[role=alertdialog]").inner_text(), "cancelling edits asks before discarding")
        await pg.locator('[role=alertdialog] button:text-is("Discard")').click()
        await pg.wait_for_timeout(300)
        check(await keys() == ["cpu-1", "memory-1", "filesystems-1", "network-1", "gpu-1", "temperature-1", "cpu-2", "network-2", "memory-2", "filesystems-2", "system-1", "diskio-1", "uptime-1"], "and the layout is as it was")

        # -- save per host ---------------------------------------------------------------------------------------------------------
        await pg.click("[data-testid=customise]")
        await grip("cpu-1").focus()
        await pg.keyboard.press("ArrowRight")
        await pg.locator('button[aria-label="Hide Filesystem usage"]').first.click()
        await pg.get_by_role("button", name="Save layout").click()
        await pg.wait_for_timeout(400)
        st = await stored()
        check(st is not None and list(st["hosts"].keys()) == ["h1"], f"saving keeps the layout for this host only: {list((st or {}).get('hosts', {}))}")
        mine = await keys()
        check(mine[:2] == ["memory-1", "cpu-1"] and "filesystems-1" not in mine, "and it is what is shown")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        await open_metrics(pg, "claude")
        check(await keys() == mine, "it is still there when the window is opened again")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        await open_metrics(pg, "web-02")
        check((await keys())[0] == "cpu-1" and "filesystems-1" in await keys(), "another host still has the default layout")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)

        # -- templates ---------------------------------------------------------------------------------------------------------------
        sampling_before = await pg.evaluate("[localStorage.getItem('sshvault.hostmetrics.view.v1'), localStorage.getItem('sshvault.hostmetrics.terminal.v1'), localStorage.getItem('sshvault.monitored.v1')]")
        await open_metrics(pg, "claude")
        await pg.click("[data-testid=customise]")
        await pg.get_by_role("button", name="Templates").click()
        await pg.locator('[role=menu][aria-label=Templates] button:has-text("Save this layout as a template")').click()
        await pg.wait_for_timeout(300)
        await pg.locator("[role=alertdialog] input").fill("Compact")
        await pg.locator('[role=alertdialog] button:text-is("Save")').click()
        await pg.wait_for_timeout(300)
        st = await stored()
        check([t["name"] for t in st["templates"].values()] == ["Compact"], "a layout can be saved as a named template")
        await pg.get_by_role("button", name="Templates").click()
        await pg.locator('[role=menu][aria-label=Templates] button:has-text("Save this layout as a template")').click()
        await pg.locator("[role=alertdialog] input").fill("compact")
        await pg.locator('[role=alertdialog] button:text-is("Save")').click()
        await pg.wait_for_timeout(300)
        check("already exists" in await pg.locator("body").inner_text() and len((await stored())["templates"]) == 1, "a name that is taken (any case) is refused")
        await templates_menu(pg)
        await pg.locator('[aria-label="Rename Compact"]').click()
        await pg.locator("[role=alertdialog] input").fill("Lean")
        await pg.locator('[role=alertdialog] button:text-is("Rename")').click()
        await pg.wait_for_timeout(300)
        check([t["name"] for t in (await stored())["templates"].values()] == ["Lean"], "a template can be renamed")
        await templates_menu(pg)
        await pg.locator('[role=menu][aria-label=Templates] button:text-is("Apply to hosts…")').click()
        await pg.wait_for_timeout(300)
        dlg = pg.locator("[role=dialog]").last
        check("sampling intervals, permissions and alert settings are not touched" in (await dlg.inner_text()).replace("\n", " "), "the apply dialog says only the layout changes")
        check(await dlg.locator("button:has-text('Apply to')").is_disabled(), "applying to no host is not offered")
        await dlg.locator("label:has-text('web-02') input").check()
        await dlg.locator("button:has-text('Apply to 1 host')").click()
        await pg.wait_for_timeout(400)
        st = await stored()
        check(sorted(st["hosts"].keys()) == ["h1", "h4"], f"the template is applied to the chosen host: {sorted(st['hosts'])}")
        check(st["hosts"]["h4"] == st["templates"][list(st["templates"])[0]]["layout"], "as its own copy of the layout")
        sampling_after = await pg.evaluate("[localStorage.getItem('sshvault.hostmetrics.view.v1'), localStorage.getItem('sshvault.hostmetrics.terminal.v1'), localStorage.getItem('sshvault.monitored.v1')]")
        check(sampling_before == sampling_after, "sampling, terminal and monitoring settings are exactly as they were")
        await pg.get_by_role("button", name="Cancel").click()
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        await open_metrics(pg, "web-02")
        k4 = await keys()
        check(k4[:2] == ["memory-1", "cpu-1"] and "filesystems-1" not in k4, "the other host now shows the template's layout")
        await pg.click("[data-testid=customise]")
        await templates_menu(pg)
        await pg.locator('[aria-label="Delete Lean"]').click()
        await pg.locator('[role=alertdialog] button:text-is("Delete")').click()
        await pg.wait_for_timeout(300)
        st = await stored()
        check(st["templates"] == {} and "h4" in st["hosts"], "deleting a template keeps the layouts already applied")
        await pg.get_by_role("button", name="Cancel").click()

        # -- reset ----------------------------------------------------------------------------------------------------------------
        await pg.click("[data-testid=customise]")
        await pg.get_by_role("button", name="Reset").click()
        check((await keys())[0] == "cpu-1" and "filesystems-1" in await keys(), "Reset shows the default layout")
        await pg.get_by_role("button", name="Save layout").click()
        await pg.wait_for_timeout(300)
        check("h4" not in (await stored())["hosts"], "and saving it returns the host to following the default")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)

        # -- closing the window while editing asks ---------------------------------------------------------------------------------------
        await open_metrics(pg, "claude")
        await pg.click("[data-testid=customise]")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        check("editing the dashboard layout" in await pg.locator("[role=alertdialog]").inner_text(), "closing the window while editing asks first")
        await pg.locator('[role=alertdialog] button:text-is("Cancel")').click()
        await pg.get_by_role("button", name="Cancel").first.click()
        await pg.wait_for_timeout(300)
        if await pg.locator("[role=alertdialog]").count():
            await pg.locator('[role=alertdialog] button:text-is("Discard")').click()

        # -- a narrow window ---------------------------------------------------------------------------------------------------------------
        await pg.set_viewport_size({"width": 640, "height": 900})
        await pg.wait_for_timeout(600)
        gw = (await pg.locator("[data-testid=dashboard-grid]").bounding_box())["width"]
        cw = (await pg.locator('[data-cell][data-key="memory-1"]').bounding_box())["width"]
        check(0.4 < cw / gw < 0.6, f"in a narrow window a compact card takes half the width, not a sliver ({cw:.0f} of {gw:.0f})")
        if SHOT:
            await pg.screenshot(path=os.path.join(SHOT, "dash-narrow.png"))
        await pg.set_viewport_size({"width": 1500, "height": 1000})

        # -- hosts without the hardware do not get its widgets ----------------------------------------------------------------------------
        check(not errors, f"no page errors {errors}")
        await b.close()
    if failures:
        print(f"\n{len(failures)} failed")
        sys.exit(1)
    print("\nall passed")


asyncio.run(main())
