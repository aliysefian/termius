"""Runbook rollback in a real (headless) browser with the backend mocked: the switch only for a runbook that has rollback
steps, the dry run showing them as "only on a host where the run failed", the choice reaching the backend, and the results
showing what was rolled back.

    pnpm dev &
    python3 src/lib/__tests__/e2e/rollback.py [chromium-executable]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

WITH = json.dumps({"name": "Deploy", "steps": [{"name": "Switch", "run": "switch"}, {"name": "Check", "run": "check"}],
                   "rollback": [{"name": "Switch back", "run": "switch-back"}]})
WITHOUT = json.dumps({"name": "Plain", "steps": [{"name": "One", "run": "one"}]})

MOCK = """
window.__calls = [];
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return [rec('h1', { label: 'web-01', hostname: 'web-01.example', port: 22, group: '', tags: [], notes: '', identity_id: 'i1' })];
  if (cmd === 'list_identities') return [rec('i1', { label: 'ops', username: 'ops', auth: { type: 'password', password: 'x' }, notes: '' })];
  if (cmd === 'list_runbooks') return [rec('r1', { name: 'Deploy', body: %s }), rec('r2', { name: 'Plain', body: %s })];
  if (cmd === 'runbook_check') {
    const doc = JSON.parse(args.body);
    return { name: doc.name, description: '', params: [], steps: doc.steps.length, rollback_steps: (doc.rollback || []).length, problems: [] };
  }
  if (cmd === 'runbook_plan') {
    const doc = JSON.parse(args.body);
    const all = (doc.steps || []).map((s) => ['run', s]).concat((doc.rollback || []).map((s) => ['rollback', s]));
    return all.map(([phase, s], i) => ({ phase, index: i, name: s.name, kind: 'run', text: s.run, condition: phase === 'rollback' ? 'only on a host where the run failed, with rollback on' : null, on_error: 'stop' }));
  }
  if (cmd === 'runbook_start') {
    window.__calls.push({ cmd, rollback: args.rollback });
    // Each run has its own channel, and a channel's messages are numbered from zero.
    const key = '__n' + args.onEvent.id;
    const emit = (m) => window['_' + args.onEvent.id]({ index: window[key] = (window[key] ?? -1) + 1, message: m });
    const out = (code) => ({ stdout: '', stderr: '', exit_code: code, truncated: false, duration_ms: 50 });
    setTimeout(() => {
      emit({ event: 'host_started', host_id: 'h1' });
      emit({ event: 'step', host_id: 'h1', result: { phase: 'run', index: 0, name: 'Switch', status: 'ok', output: out(0), note: '' } });
      emit({ event: 'step', host_id: 'h1', result: { phase: 'run', index: 1, name: 'Check', status: 'failed', output: out(1), note: 'exited with 1' } });
      if (args.rollback) emit({ event: 'step', host_id: 'h1', result: { phase: 'rollback', index: 2, name: 'Switch back', status: 'ok', output: out(0), note: '' } });
      emit({ event: 'host_done', host_id: 'h1', ok: false, error: null });
      emit({ event: 'done', cancelled: false });
    }, 150);
    return null;
  }
  return undefined;
};
""" % (json.dumps(WITH), json.dumps(WITHOUT))


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1500, "height": 1000})
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK)
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL); await pg.wait_for_timeout(2500)
        await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(300)
        await pg.keyboard.type("Go to Runbooks"); await pg.wait_for_timeout(300); await pg.keyboard.press("Enter")
        await pg.wait_for_selector("[data-testid=runbooks]", timeout=8000); await pg.wait_for_timeout(600)

        await pg.locator("aside button", has_text="Deploy").click(); await pg.wait_for_timeout(600)
        check("a runbook with rollback steps offers the switch, off", await pg.locator("[data-testid=runbook-rollback]").count() == 1 and not await pg.locator("[data-testid=runbook-rollback] input").is_checked())
        await pg.locator("[data-testid=runbook-hosts] label", has_text="web-01").locator("input").check()
        await pg.click("[data-testid=runbook-dry]"); await pg.wait_for_timeout(400)
        plan = await pg.locator("[data-testid=runbook-plan]").inner_text()
        check("the dry run shows the rollback step and when it would run", "Switch back" in plan and "rollback" in plan and "only on a host where the run failed" in plan, plan)

        await pg.click("[data-testid=runbook-run]"); await pg.wait_for_timeout(900)
        calls = [c for c in await pg.evaluate("window.__calls") if c["cmd"] == "runbook_start"]
        check("without the switch the run asks for no rollback", calls[-1]["rollback"] is False, calls)
        await pg.locator("[data-host=web-01]").click(); await pg.wait_for_timeout(300)
        check("and a failed host is not rolled back", "Switch back" not in await pg.locator("[data-testid=run-results]").inner_text())

        await pg.locator("[data-testid=runbook-rollback] input").check()
        await pg.click("[data-testid=runbook-run]"); await pg.wait_for_timeout(900)
        calls = [c for c in await pg.evaluate("window.__calls") if c["cmd"] == "runbook_start"]
        check("with the switch on, the run asks for it", calls[-1]["rollback"] is True, calls)
        if await pg.locator("[data-host=web-01]").get_attribute("aria-expanded") != "true":
            await pg.locator("[data-host=web-01]").click(); await pg.wait_for_timeout(300)
        results = await pg.locator("[data-testid=run-results]").inner_text()
        check("the host shows its steps and then the rollback step, marked", "Switch back" in results and "rollback" in results, results)
        check("the host's line counts the run's own steps and says it was rolled back", "2 of 2 steps, then rolled back" in results, results)

        # A runbook without rollback steps does not offer it.
        await pg.locator("aside button", has_text="Plain").click(); await pg.wait_for_timeout(600)
        offered = await pg.locator("[data-testid=runbook-rollback]").count()
        check("a runbook with no rollback steps offers no switch", offered == 0, offered)
        check("no page errors", errs == [], errs)
        await b.close()
    if failures:
        print("FAILED:", failures); sys.exit(1)
    print("all passed")

asyncio.run(main())
