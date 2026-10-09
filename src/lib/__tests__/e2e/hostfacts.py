"""Collecting host facts from the monitor window, in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/hostfacts.py [chromium-executable]

Opens a host's monitor from the Topology page, checks that nothing is collected until the button is pressed, collects, and
checks what is shown and that it is labelled as observed at a time.
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
URL = "http://127.0.0.1:1420/"
DATA = """
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const host = (id, label, extra = {}) => rec(id, { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', environment: 'staging', identity_id: 'i1', favorite: false, ...extra });
window.__factsCalls = 0;
const FACTS = 'HOSTNAME=web-01\\nKERNEL=Linux 6.8.0-1\\nARCH=x86_64\\nOS=Ubuntu 24.04.1 LTS\\nCPU=AMD EPYC\\nCORES=8\\nMEM_KB=16384000\\nSWAP_KB=0\\nUPTIME=86400\\nDISK=/|103080888|41230355|42%\\nDISK=/data|1000|950|95%\\nINIT=systemd\\nTOOL=docker|Docker version 27.1.1\\n';
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return [host('b1', 'bastion'), host('a1', 'web-01', { jump_host_id: 'b1' })];
  if (cmd === 'list_identities') return [rec('i1', { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' })];
  if (cmd === 'monitor_open') return 'sess-1';
  if (cmd === 'monitor_exec') {
    if ((args.script || '').includes('HOSTNAME=')) { window.__factsCalls++; return { stdout: FACTS, stderr: '', code: 0 }; }
    return { stdout: '@@OS\\nLinux\\n@@TA\\n10\\n@@TB\\n11\\n', stderr: '', code: 0 };
  }
  return undefined;
};
"""

async def main():
    failures = []
    def expect(what, got, want):
        ok = got == want
        print(f"{'ok  ' if ok else 'FAIL'} {what}: {got!r}" + ("" if ok else f" (wanted {want!r})"))
        if not ok: failures.append(what)
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 900})
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)[:160]))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(DATA)
        await pg.add_init_script("window.__units = 1;")
        await pg.goto(URL); await pg.wait_for_timeout(2500)
        await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(300)
        await pg.keyboard.type("Go to Topology"); await pg.keyboard.press("Enter"); await pg.wait_for_timeout(900)
        await pg.locator('[data-node]:has-text("web-01")').first.click(); await pg.wait_for_timeout(300)
        await pg.locator('aside button:has-text("Monitor")').click(); await pg.wait_for_timeout(1500)
        await pg.locator('[role=tab]:has-text("System")').click(); await pg.wait_for_timeout(300)
        expect("nothing collected before the button", await pg.evaluate("window.__factsCalls"), 0)
        expect("the button is offered", await pg.locator('button:has-text("Collect facts")').count(), 1)
        await pg.locator('button:has-text("Collect facts")').click(); await pg.wait_for_timeout(800)
        expect("collected once", await pg.evaluate("window.__factsCalls"), 1)
        body = await pg.locator("[role=dialog]").first.inner_text() if await pg.locator("[role=dialog]").count() else await pg.inner_text("body")
        for want in ["Ubuntu 24.04.1 LTS", "AMD EPYC · 8 cores", "15.6 GiB", "systemd", "Docker version 27.1.1", "95% used", "Observed"]:
            expect(f"shows {want!r}", want in body, True)
        expect("offers collecting again and copying", [await pg.locator('button:has-text("Collect again")').count(), await pg.locator('button:has-text("Copy as text")').count()], [1, 1])
        expect("page errors", errs, [])
        await b.close()
    if failures:
        print("FAILED:", failures); sys.exit(1)

asyncio.run(main())
