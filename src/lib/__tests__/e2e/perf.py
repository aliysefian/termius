"""Does smart completion slow the terminal? Real bash, 10 MB of output and 200 characters of typing,
with the feature off and on.

    pnpm dev &
    python3 src/lib/__tests__/e2e/perf.py [chromium-executable]
"""
import asyncio, os, sys, time, importlib.util
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("shells_helpers", os.path.join(HERE, "shells.py"))
# Reuse the pty plumbing without running the shell tests: load the source up to its entry point.
src = open(os.path.join(HERE, "shells.py")).read().split("async def run_shell")[0]
ns = {"__file__": os.path.join(HERE, "shells.py"), "__name__": "shells_helpers"}
sys.argv = [sys.argv[0]] + sys.argv[1:2]
exec(compile(src, "shells.py", "exec"), ns)
Pty, find_shells, URL, EXE = ns["Pty"], ns["find_shells"], ns["URL"], ns["EXE"]


async def one(browser, prefs, label, big):
    page = await browser.new_page(viewport={"width": 1000, "height": 700})
    sh = Pty("bash", find_shells()["bash"], page)
    await page.expose_function("__ptySpawn", lambda c, r: sh.start(c, r))
    await page.expose_function("__ptyWrite", lambda d: sh.write(d))
    await page.expose_function("__ptyResize", lambda c, r: sh.resize(c, r))
    await page.add_init_script(path=os.path.join(HERE, "mock.js"))
    await page.goto(URL)
    await page.evaluate("(p) => localStorage.setItem('sshvault.prefs.v1', JSON.stringify(p))", prefs)
    await page.goto(URL)
    await page.wait_for_timeout(2500)
    await page.keyboard.press("Control+Shift+Backquote")
    await page.wait_for_timeout(2000)
    await page.click(".xterm")
    k = page.keyboard

    async def until(marker, start, limit=120):
        t0 = time.time()
        while marker not in sh.text()[start:]:
            if time.time() - t0 > limit:
                return None
            await page.wait_for_timeout(20)
        return time.time() - t0

    # history to suggest from
    for c in ["echo alpha-one", "echo alpha-two"]:
        await k.type(c)
        await k.press("Enter")
        await page.wait_for_timeout(500)

    # 10 MB of output
    start = len(sh.text())
    await k.type(f"cat {big}; echo DONE$((20+22))")
    await k.press("Enter")
    cat = await until("DONE42\r", start)

    # 200 characters typed as fast as the browser allows, with a suggestion being computed on every one
    await page.wait_for_timeout(500)
    text = "echo alpha-" + "x" * 189
    start = len(sh.text())
    t0 = time.time()
    await k.type(text, delay=0)
    await k.press("Enter")
    # The command's own output is one unbroken line; the typed echo is broken up where the line wraps.
    await until("alpha-" + "x" * 189 + "\r\n", start)
    total = time.time() - t0
    print(f"{label:>10}: 10 MB cat {cat:6.2f} s | 200 characters typed and run, output back after {total*1000:6.0f} ms")
    sh.stop()
    await page.close()
    return cat, total


async def main():
    big = os.path.join("/tmp", "sshvault-perf-10mb.txt")
    line = ("The quick brown fox jumps over the lazy dog 0123456789 " * 2)[:99] + "\n"
    with open(big, "w") as f:
        for _ in range(10_000_000 // len(line)):
            f.write(line)
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        for round_ in range(3):
            await one(b, {"smartCompletion": False}, "off", big)
            await one(b, {"smartCompletion": True, "acInline": True, "acMenu": True}, "on", big)
        await b.close()
    os.remove(big)


asyncio.run(main())
