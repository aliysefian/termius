"""Automation (feat2 4.9) in a real (headless) browser with the backend mocked: runbooks (checking as you type, a
dry run, a run with live results, a typed confirmation on production, the history), schedules, a host's
"wait for this, send that", and hooks that are approved before they run.

    pnpm dev &
    python3 src/lib/__tests__/e2e/automation.py [chromium-executable]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

RUNBOOK = json.dumps({
    "name": "Restart a service",
    "params": [{"name": "service", "label": "Service", "default": "nginx"}],
    "steps": [
        {"name": "Is it running?", "id": "up", "run": "systemctl is-active {{service|q}}", "on_error": "continue"},
        {"name": "Restart", "run": "sudo systemctl restart {{service|q}}"},
    ],
}, indent=2)

MOCK = """
window.__calls = [];
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const host = (id, label, extra) => rec(id, Object.assign({ label, hostname: label + '.example', port: 22, group: 'Web', tags: [], notes: '', identity_id: 'i1' }, extra || {}));
window.__hosts = [host('h1', 'web-01'), host('h2', 'web-02'), host('h3', 'prod-01', { environment: 'production' })].concat(window.__extraHosts || []);
window.__runbooks = [rec('r1', { name: 'Restart a service', body: %s })];
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return window.__hosts;
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: 'x' }, notes: '' } }];
  if (cmd === 'list_runbooks') return window.__runbooks;
  if (cmd === 'save_runbook') { const r = rec(args.id || 'r' + (window.__runbooks.length + 1), args.runbook); window.__runbooks = window.__runbooks.filter((x) => x.id !== r.id).concat([r]); window.__calls.push({ cmd, runbook: args.runbook }); return r; }
  if (cmd === 'runbook_check') {
    let doc; try { doc = JSON.parse(args.body); } catch (e) { return { name: null, description: '', params: [], steps: 0, problems: [{ step: null, message: 'not a valid runbook: ' + e.message }] }; }
    const problems = (doc.steps || []).flatMap((s, i) => (JSON.stringify(s).includes('{{nope}}') ? [{ step: i, message: '{{nope}} isn\\'t a parameter of this runbook' }] : []));
    return { name: doc.name || null, description: '', params: (doc.params || []).map((p) => Object.assign({ label: '', default: null, optional: false, choices: [], kind: 'text' }, p)), steps: (doc.steps || []).length, problems };
  }
  if (cmd === 'runbook_plan') {
    window.__calls.push({ cmd, params: args.params, host: args.host });
    const doc = JSON.parse(args.body);
    return doc.steps.map((s, i) => ({ index: i, name: s.name || 'Step ' + (i + 1), kind: 'run', text: s.run.replace('{{service|q}}', args.params.service || ''), condition: null, on_error: s.on_error || 'stop' }));
  }
  if (cmd === 'runbook_start') {
    window.__calls.push({ cmd, params: args.params, hostIds: args.hostIds, scheduled: args.scheduled });
    const emit = (m) => window['_' + args.onEvent.id]({ index: window.__n = (window.__n ?? -1) + 1, message: m });
    const out = (stdout, code) => ({ stdout, stderr: '', exit_code: code, truncated: false, duration_ms: 120 });
    setTimeout(() => {
      for (const h of args.hostIds) {
        emit({ event: 'host_started', host_id: h });
        emit({ event: 'step', host_id: h, result: { index: 0, name: 'Is it running?', status: h === 'h2' ? 'failed' : 'ok', output: out('active\\n', h === 'h2' ? 3 : 0), note: h === 'h2' ? 'exited with 3' : '' } });
        if (h === 'h2') emit({ event: 'host_done', host_id: h, ok: false, error: null });
        else {
          emit({ event: 'step', host_id: h, result: { index: 1, name: 'Restart', status: 'ok', output: out('', 0), note: '' } });
          emit({ event: 'host_done', host_id: h, ok: true, error: null });
        }
      }
      emit({ event: 'done', cancelled: false });
    }, 150);
    return null;
  }
  if (cmd === 'runbook_history_list') return [{ id: '11111111-1111-1111-1111-111111111111', runbook: 'Restart a service', started_at: 1790000000, scheduled: true, cancelled: false, running: false, hosts: 2, ok: 1, failed: 1 }];
  if (cmd === 'runbook_history_get') return { id: args.id, runbook: 'Restart a service', started_at: 1790000000, finished_at: 1790000005, scheduled: true, cancelled: false, params: { service: 'nginx' }, hosts: [{ host_id: 'h1', label: 'web-01', ok: true, error: null, steps: [{ index: 0, name: 'Restart', status: 'ok', output: { stdout: 'restarted\\n', stderr: '', exit_code: 0, truncated: false, duration_ms: 90 }, note: '' }] }] };
  if (cmd === 'run_hook') { window.__calls.push({ cmd, command: args.command }); return { exit_code: window.__hookExit || 0, output: 'vpn said hi', timed_out: false }; }
  return undefined;
};
""" % json.dumps(RUNBOOK)


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b, pre="", prefs=None):
    pg = await b.new_page(viewport={"width": 1500, "height": 1000})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(pre + MOCK)
    await pg.goto(URL)
    await pg.evaluate("(p) => { localStorage.clear(); localStorage.setItem('sshvault.prefs.v1', JSON.stringify(p)); }", prefs or {})
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    return pg


async def open_runbooks(pg):
    await pg.keyboard.press("Control+Shift+P")
    await pg.wait_for_timeout(300)
    await pg.keyboard.type("Go to Runbooks")
    await pg.wait_for_timeout(300)
    await pg.keyboard.press("Enter")
    await pg.wait_for_timeout(700)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- runbooks ----
        pg = await fresh(b)
        await open_runbooks(pg)
        check("the runbooks page opens with the saved runbook selected", await pg.locator("[data-testid=runbooks]").count() == 1 and await pg.locator("#rb-body").input_value() == RUNBOOK)
        check("a valid runbook says so", "2 steps, 1 parameter" in await pg.locator("[data-testid=runbook-ok]").inner_text())
        check("its parameter is offered with the default", await pg.input_value("#rp-service") == "nginx")
        await pg.fill("#rb-body", RUNBOOK.replace("{{service|q}}", "{{nope}}", 1))
        await pg.wait_for_timeout(500)
        check("a problem is shown against its step as you type", "Step 1: {{nope}} isn't a parameter" in await pg.locator("[data-testid=runbook-problems]").inner_text())
        check("and nothing can run until it is fixed", await pg.locator("[data-testid=runbook-run]").is_disabled() and await pg.locator("[data-testid=runbook-dry]").is_disabled())
        await pg.fill("#rb-body", "{ not json")
        await pg.wait_for_timeout(500)
        check("text that isn't JSON is explained", "not a valid runbook" in await pg.locator("[data-testid=runbook-problems]").inner_text())
        await pg.fill("#rb-body", RUNBOOK)
        await pg.wait_for_timeout(500)

        for label in ["web-01", "web-02"]:
            await pg.locator("[data-testid=runbook-hosts] label", has_text=label).locator("input").check()
        await pg.fill("#rp-service", "redis")
        await pg.click("[data-testid=runbook-dry]")
        await pg.wait_for_timeout(400)
        plan = await pg.locator("[data-testid=runbook-plan]").inner_text()
        check("a dry run shows the steps filled in, and runs nothing", "systemctl is-active redis" in plan and not any(c["cmd"] == "runbook_start" for c in await pg.evaluate("window.__calls")), plan)

        await pg.click("[data-testid=runbook-run]")
        await pg.wait_for_timeout(900)
        calls = await pg.evaluate("window.__calls")
        start = [c for c in calls if c["cmd"] == "runbook_start"][0]
        check("it runs on the chosen hosts with the values", start["hostIds"] == ["h1", "h2"] and start["params"] == {"service": "redis"} and start["scheduled"] is False, start)
        states = await pg.evaluate("[...document.querySelectorAll('[data-host]')].map((e) => [e.dataset.host, e.querySelector('[data-testid=host-state]').textContent.trim()])")
        check("each host shows how it went", states[0][1].startswith("done") and states[1][1].startswith("failed"), states)
        check("the summary counts failures", "1 of 2 hosts failed" in await pg.locator("[data-testid=runbook-live]").inner_text())
        await pg.locator("[data-host=web-02]").click()
        await pg.wait_for_timeout(200)
        check("a host opens to its steps and what they printed", "exited with 3" in await pg.locator("[data-testid=run-results]").inner_text())

        # production needs the runbook's name typed
        await pg.locator("[data-testid=runbook-hosts] label", has_text="prod-01").locator("input").check()
        await pg.click("[data-testid=runbook-run]")
        await pg.wait_for_timeout(400)
        dialog = pg.locator("[role=dialog], [role=alertdialog]").last
        check("a production host asks first and names it", "prod-01" in await dialog.inner_text())
        run_btn = dialog.get_by_role("button", name="Run")
        check("the run button waits for the runbook's name to be typed", await run_btn.is_disabled())
        await dialog.locator("input").fill("Restart a service")
        check("and then it can go", await run_btn.is_enabled())
        await dialog.get_by_role("button", name="Cancel").click()

        # saving and the history tab
        await pg.fill("#rb-body", RUNBOOK.replace("Restart a service", "Restart it"))
        await pg.wait_for_timeout(500)
        await pg.get_by_role("button", name="Save").click()
        await pg.wait_for_timeout(400)
        saved = [c for c in await pg.evaluate("window.__calls") if c["cmd"] == "save_runbook"]
        check("saving keeps the document as written, named from it", saved and saved[0]["runbook"]["name"] == "Restart it" and "Restart it" in saved[0]["runbook"]["body"], saved)
        await pg.get_by_role("tab", name="History").click()
        await pg.wait_for_timeout(500)
        check("the history lists runs, marking scheduled ones", "scheduled" in await pg.locator("[data-testid=runbook-history]").inner_text())
        await pg.locator("[data-testid=runbook-history] button", has_text="Restart a service").first.click()
        await pg.wait_for_timeout(400)
        await pg.locator("[data-host=web-01]").click()
        check("a past run shows its output", "restarted" in await pg.locator("[data-testid=run-results]").inner_text())
        check("no page errors from runbooks", not pg.errors, pg.errors)

        # ---- schedules ----
        await pg.get_by_role("tab", name="Schedules").click()
        await pg.wait_for_timeout(300)
        check("the schedules page says it only runs while the app is open", "only while SSHVault is open" in await pg.locator("[data-testid=schedules]").inner_text())
        await pg.click("[data-testid=schedule-new]")
        await pg.select_option("#sc-rb", "r1")
        await pg.wait_for_timeout(300)
        await pg.locator("[data-testid=schedule-form] label", has_text="web-01").locator("input").check()
        await pg.select_option("#sc-kind", "weekly")
        await pg.fill("#sc-at", "03:15")
        await pg.click("[data-testid=schedule-save]")
        await pg.wait_for_timeout(400)
        row = await pg.locator("[data-testid=schedule-row]").inner_text()
        check("a schedule is made and described in words", "on 1 host" in row and "03:15" in row and "next" in row, row)
        prefs = await pg.evaluate("JSON.parse(localStorage.getItem('sshvault.prefs.v1')).schedules")
        check("it is kept on this computer, off production by default", len(prefs) == 1 and prefs[0]["allowProduction"] is False and prefs[0]["when"]["kind"] == "weekly", prefs)
        await pg.click("[data-testid=schedule-new]")
        await pg.select_option("#sc-rb", "r1")
        await pg.wait_for_timeout(300)
        await pg.locator("[data-testid=schedule-form] label", has_text="web-01").locator("input").check()
        await pg.select_option("#sc-kind", "every")
        await pg.fill("#sc-min", "2")
        await pg.click("[data-testid=schedule-save]")
        await pg.wait_for_timeout(300)
        check("a timing that is too short is refused with the reason", await pg.locator("[data-testid=schedule-form] [role=alert]").count() == 1 and len(await pg.locator("[data-testid=schedule-row]").all()) == 1)
        await pg.close()

        # a schedule that is due runs by itself, scheduled, and one with a production host doesn't
        long_ago = 1000
        pg = await fresh(b, "", {"schedules": [
            {"id": "s1", "runbookId": "r1", "params": {"service": "nginx"}, "hostIds": ["h1"], "when": {"kind": "every", "minutes": 5}, "enabled": True, "allowProduction": False, "lastRun": long_ago},
            {"id": "s2", "runbookId": "r1", "params": {}, "hostIds": ["h3"], "when": {"kind": "every", "minutes": 5}, "enabled": True, "allowProduction": False, "lastRun": long_ago},
        ]})
        await open_runbooks(pg)
        await pg.get_by_role("tab", name="Schedules").click()
        await pg.wait_for_timeout(300)
        rows = await pg.locator("[data-testid=schedule-row]").all_inner_texts()
        check("a schedule that can't run says why", any("production host" in r for r in rows), rows)
        await pg.close()

        # ---- wait for this, send that ----
        pg = await fresh(b, "window.__extraHosts = [];", None)
        await pg.close()
        pg = await b.new_page(viewport={"width": 1500, "height": 1000})
        pg.errors = []
        pg.on("pageerror", lambda e: pg.errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script("window.__extraHosts = [];" + MOCK.replace("window.__hosts = [", "window.__hosts = [host('e1', 'menu-box', { expect: [{ wait: '$', send: 'echo hi' }] }), host('p1', 'secret-box', { expect: [{ wait: 'Password:', send: 'x' }] }), "))
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)
        await pg.keyboard.press("Control+Shift+P")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("menu-box")
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(2500)
        sent = await pg.evaluate("window.__sent.map((b) => String.fromCharCode(...b)).join('')")
        check("after the prompt appears the host's step types its answer and Enter", "echo hi\r" in sent, sent)
        await pg.close()

        # ---- hooks ----
        pg = await b.new_page(viewport={"width": 1500, "height": 1000})
        pg.errors = []
        pg.on("pageerror", lambda e: pg.errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK.replace("window.__hosts = [", "window.__hosts = [host('k1', 'vpn-box', { hook_before: 'vpn up {host}', hook_after: 'vpn down' }), "))
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)
        await pg.keyboard.press("Control+Shift+P")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("vpn-box")
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(600)
        dialog = pg.locator("[role=dialog], [role=alertdialog]").last
        text = await dialog.inner_text()
        check("a hook shows its exact command and asks before it runs", "vpn up vpn-box.example" in text and not any(c["cmd"] == "run_hook" for c in await pg.evaluate("window.__calls")), text)
        await dialog.get_by_role("button", name="Run it").click()
        await pg.wait_for_timeout(500)
        dialog = pg.locator("[role=dialog], [role=alertdialog]").last
        check("the command for after closing is approved at the same time", "vpn down" in await dialog.inner_text())
        await dialog.get_by_role("button", name="Run it").click()
        await pg.wait_for_timeout(1500)
        calls = await pg.evaluate("window.__calls")
        check("then the before-command runs, and the host opens", any(c["cmd"] == "run_hook" and c["command"] == "vpn up vpn-box.example" for c in calls) and await pg.locator(".xterm").count() >= 1, calls)
        await pg.locator('button[title^="Close ("]').first.click(force=True)
        await pg.wait_for_timeout(500)
        confirm = pg.locator("[role=dialog], [role=alertdialog]").last
        if await confirm.count() and "ends" in await confirm.inner_text():
            await confirm.get_by_role("button", name="Close").click()
        await pg.wait_for_timeout(800)
        calls = await pg.evaluate("window.__calls")
        check("closing the tab runs the after-command", any(c["cmd"] == "run_hook" and c["command"] == "vpn down" for c in calls), calls)
        check("no page errors from hooks", not pg.errors, pg.errors)
        await pg.close()

        await b.close()
    print(f"\n{len(failures)} failed" if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
