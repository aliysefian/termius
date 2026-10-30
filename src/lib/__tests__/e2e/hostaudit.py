"""The security audit from a host's monitor window, in a real (headless) browser with the backend mocked.

    pnpm dev &
    python3 src/lib/__tests__/e2e/hostaudit.py [chromium-executable]

Opens a host's monitor from the Topology page and checks that nothing is collected until the button is pressed; that
what a real (recorded) run of the script shows is the score, the findings worst first with the serious ones open, the
checks that could not be made, and the passes; that the report copies as text; and that a host that is not Linux and a
failed run are said plainly.
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
URL = "http://127.0.0.1:1420/"
RECORDED = open(os.path.join(HERE, "..", "hostaudit", "ubuntu-user.txt"), encoding="utf-8").read()

DATA = """
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const host = (id, label, extra = {}) => rec(id, { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', environment: 'staging', identity_id: 'i1', favorite: false, ...extra });
window.__auditCalls = 0;
window.__copied = [];
window.__auditMode = 'ok';
const RECORDED = %s;
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return [host('b1', 'bastion'), host('a1', 'web-01', { jump_host_id: 'b1' })];
  if (cmd === 'list_identities') return [rec('i1', { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' })];
  if (cmd === 'monitor_open') return 'sess-1';
  if (cmd === 'monitor_exec') {
    if ((args.script || '').includes('@@SSHD')) {
      window.__auditCalls++;
      if (window.__auditMode === 'mac') return { stdout: '@@HOST\\nos=Darwin\\nuid=501\\n@@END\\n', stderr: '', code: 0 };
      if (window.__auditMode === 'fail') throw { code: 'ssh', message: 'the connection dropped' };
      return { stdout: RECORDED, stderr: '', code: 0 };
    }
    return { stdout: '@@OS\\nLinux\\n@@TA\\n10\\n@@TB\\n11\\n', stderr: '', code: 0 };
  }
  if (cmd === 'plugin:clipboard-manager|write_text') { window.__copied.push(args.text); return null; }
  return undefined;
};
""" % json.dumps(RECORDED)


async def main():
    failures = []

    def expect(what, got, want):
        ok = got == want
        print(f"{'ok  ' if ok else 'FAIL'} {what}: {got!r}" + ("" if ok else f" (wanted {want!r})"))
        if not ok:
            failures.append(what)

    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 1100})
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)[:160]))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(DATA)
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)
        await pg.keyboard.press("Control+Shift+P")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("Go to Topology")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(900)
        await pg.locator('[data-node]:has-text("web-01")').first.click()
        await pg.wait_for_timeout(300)
        await pg.locator('aside button:has-text("Monitor")').click()
        await pg.wait_for_timeout(1500)
        await pg.locator("[role=tab]:has-text('Security')").click()
        await pg.wait_for_timeout(300)
        audit = pg.locator('[data-testid="host-audit"]')

        expect("nothing is collected before the button is pressed", await pg.evaluate("window.__auditCalls"), 0)
        expect("the button is offered, and says it is read-only", [await audit.locator('button:has-text("Run the audit")').count(), "Nothing is changed" in await audit.inner_text()], [1, True])
        expect("the script can be read before it is run", await audit.locator("details:has(summary:has-text('What it runs'))").count(), 1)
        expect("and is not open until asked", await audit.locator("details:has(summary:has-text('What it runs'))").evaluate("e => e.open"), False)

        await audit.locator('button:has-text("Run the audit")').click()
        await pg.wait_for_timeout(900)
        expect("one run", await pg.evaluate("window.__auditCalls"), 1)
        expect("the score", await audit.locator('[data-testid="audit-score"]').inner_text(), "69%")
        expect("passed, failed, not checked", [await audit.locator(f'[data-testid="audit-{k}"]').inner_text() for k in ("passed", "failed", "unchecked")], ["20", "9", "11"])
        text = await audit.inner_text()
        expect("it says who it was read as, and that the rest needs root", ["Read as" in text and "alice" in text, "Connect as root" in text], [True, True])
        titles = await audit.locator("section[aria-label=Findings] summary span.font-medium").all_inner_texts()
        expect("the worst finding is first", titles[0], "Waiting security updates")
        expect("nine findings", len(titles), 9)
        opened = await audit.locator("section[aria-label=Findings] details").evaluate_all("els => els.map(e => e.open)")
        expect("only the serious one is open", opened, [True] + [False] * 8)
        first = await audit.locator("section[aria-label=Findings] details").first.inner_text()
        for want in ["40 security updates are waiting", "Component", "Risk", "Remediation", "apt-get"]:
            expect(f"the first finding shows {want!r}", want in first, True)
        unchecked = await audit.locator("section[aria-label='Not checked'] summary span.font-medium").all_inner_texts()
        expect("the checks it could not make are listed, not counted as passed", [len(unchecked), {"Root login over SSH", "Passwordless sudo"} <= set(unchecked)], [11, True])
        expect("the passes are there but folded", await audit.locator("details[aria-label='Passed checks']").evaluate("e => e.open"), False)
        await pg.screenshot(path=os.environ.get("SHOT", "/tmp/hostaudit.png"), full_page=False)

        await audit.locator('button:has-text("Copy as text")').click()
        await pg.wait_for_timeout(300)
        copied = await pg.evaluate("window.__copied")
        expect("the report copies as text", [len(copied), "Security audit of web-01" in copied[0] and "[HIGH] Waiting security updates" in copied[0] and "Remediation:" in copied[0]], [1, True])

        await audit.locator('button:has-text("Run the audit again")').click()
        await pg.wait_for_timeout(600)
        expect("running again runs again", await pg.evaluate("window.__auditCalls"), 2)

        await pg.evaluate("window.__auditMode = 'mac'")
        await audit.locator('button:has-text("Run the audit again")').click()
        await pg.wait_for_timeout(600)
        expect("a host that is not Linux is told so", "reads Linux hosts; this one reports Darwin" in await audit.inner_text(), True)

        await pg.evaluate("window.__auditMode = 'fail'")
        await audit.locator('button:has-text("Run the audit again")').click()
        await pg.wait_for_timeout(600)
        expect("a failed run is shown as a warning", await audit.locator('[role=alert]').count() >= 1 and "connection dropped" in await audit.inner_text(), True)
        expect("page errors", errs, [])
        await b.close()
    if failures:
        print("FAILED:", failures)
        sys.exit(1)


asyncio.run(main())
