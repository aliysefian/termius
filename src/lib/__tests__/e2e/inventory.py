"""Inventory and automation (feat2 4.8) in a real (headless) browser with the backend mocked: importing hosts from a
cloud's CLI output with a plan, a refresh that keeps what was edited, a network scan, and a host that connects with
a command (approved before it runs).

    pnpm dev &
    python3 src/lib/__tests__/e2e/inventory.py [chromium-executable]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

AWS = json.dumps({"Reservations": [{"Instances": [
    {"InstanceId": "i-0abc", "State": {"Name": "running"}, "PublicIpAddress": "54.1.2.3", "Placement": {"AvailabilityZone": "eu-west-1b"}, "Tags": [{"Key": "Name", "Value": "api"}]},
    {"InstanceId": "i-0def", "State": {"Name": "running"}, "PrivateIpAddress": "10.0.0.6", "Placement": {"AvailabilityZone": "eu-west-1a"}, "Tags": [{"Key": "Name", "Value": "worker"}]},
]}]})

MOCK = """
window.__calls = [];
window.__store = [];
window.__awsOutput = %s;
const mkRec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return window.__store.slice();
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: 'x' }, notes: '' } }];
  if (cmd === 'save_host') {
    const rec = mkRec(args.id || 'h' + (window.__store.length + 1), args.host);
    const i = window.__store.findIndex((r) => r.id === rec.id);
    if (i >= 0) window.__store[i] = rec; else window.__store.push(rec);
    window.__calls.push({ cmd, host: args.host });
    return rec;
  }
  if (cmd === 'delete_host') { window.__store = window.__store.filter((r) => r.id !== args.id); window.__calls.push({ cmd, id: args.id }); return null; }
  if (cmd === 'inventory_run') { window.__calls.push({ cmd, source: args.source, option: args.option }); if (window.__fail) throw { code: 'inventory_missing', message: 'aws isn\\'t installed here' }; return window.__awsOutput; }
  if (cmd === 'inventory_scan') { window.__calls.push({ cmd, range: args.range, port: args.port }); return [{ ip: '192.168.1.5', banner: 'SSH-2.0-OpenSSH_9.6' }]; }
  return undefined;
};
""" % json.dumps(AWS)


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b, pre=""):
    pg = await b.new_page(viewport={"width": 1400, "height": 1000})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(MOCK + pre)
    await pg.goto(URL)
    await pg.evaluate("() => localStorage.clear()")
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    return pg


async def palette(pg, text):
    await pg.keyboard.press("Control+Shift+P")
    await pg.wait_for_timeout(300)
    await pg.keyboard.type(text)
    await pg.wait_for_timeout(300)
    await pg.keyboard.press("Enter")
    await pg.wait_for_timeout(500)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- import, then refresh ----
        pg = await fresh(b)
        await pg.click('button[title^="Import from a cloud"]')
        await pg.wait_for_timeout(400)
        check("the import dialog opens", await pg.locator("[data-testid=import-inventory]").count() == 1)
        await pg.select_option("#inv-source", "aws")
        await pg.fill("#inv-option", "eu-west-1")
        line = await pg.locator("[data-testid=inventory-line]").inner_text()
        check("it says exactly what will run, before running it", line == "aws ec2 describe-instances --output json --region eu-west-1", line)
        await pg.click("[data-testid=inventory-run]")
        await pg.wait_for_timeout(600)
        plan = await pg.locator("[data-testid=inventory-plan]").inner_text()
        check("both machines are offered", "api" in plan and "worker" in plan and "To add (2 of 2)" in plan, plan)
        calls = await pg.evaluate("window.__calls")
        check("the program was asked for with the region", calls[0] == {"cmd": "inventory_run", "source": "aws", "option": "eu-west-1"}, calls)
        await pg.select_option("#inv-identity", "i1")
        await pg.click("[data-testid=inventory-apply]")
        await pg.wait_for_timeout(600)
        done = await pg.locator("[data-testid=inventory-done]").inner_text()
        check("they are added", done == "2 added", done)
        saved = [c["host"] for c in await pg.evaluate("window.__calls") if c["cmd"] == "save_host"]
        check("with where they came from, their group and the chosen sign-in", saved[0]["source"] == {"provider": "aws", "id": "i-0abc", "scope": "eu-west-1", "hostname": "54.1.2.3", "label": "api"} and saved[0]["group"] == "AWS/eu-west-1" and saved[0]["identity_id"] == "i1", saved[:1])

        # the person edits one host's name, and the cloud changes both addresses and drops one machine
        await pg.evaluate("""() => {
          const r = window.__store.find((x) => x.data.label === 'api');
          r.data.label = 'my-api';
          window.__awsOutput = JSON.stringify({ Reservations: [{ Instances: [{ InstanceId: 'i-0abc', State: { Name: 'running' }, PublicIpAddress: '54.9.9.9', Placement: { AvailabilityZone: 'eu-west-1b' }, Tags: [{ Key: 'Name', Value: 'api' }] }] }] });
        }""")
        await pg.reload()
        await pg.wait_for_timeout(2500)
        # the store is mocked, so it survives the reload only through window state; reopen from a fresh page state
        await pg.close()

        pg = await fresh(b, """
          window.__store.push(mkRec('a1', { label: 'my-api', hostname: '54.1.2.3', port: 22, group: 'AWS/eu-west-1', tags: [], notes: '', source: { provider: 'aws', id: 'i-0abc', scope: 'eu-west-1', hostname: '54.1.2.3', label: 'api' } }));
          window.__store.push(mkRec('a2', { label: 'worker', hostname: '10.0.0.6', port: 22, group: 'AWS/eu-west-1', tags: [], notes: '', source: { provider: 'aws', id: 'i-0def', scope: 'eu-west-1', hostname: '10.0.0.6', label: 'worker' } }));
          window.__awsOutput = JSON.stringify({ Reservations: [{ Instances: [{ InstanceId: 'i-0abc', State: { Name: 'running' }, PublicIpAddress: '54.9.9.9', Placement: { AvailabilityZone: 'eu-west-1b' }, Tags: [{ Key: 'Name', Value: 'api' }] }] }] });
        """)
        await pg.click('button[title^="Import from a cloud"]')
        await pg.wait_for_timeout(400)
        await pg.select_option("#inv-source", "aws")
        await pg.fill("#inv-option", "eu-west-1")
        await pg.click("[data-testid=inventory-run]")
        await pg.wait_for_timeout(600)
        plan = await pg.locator("[data-testid=inventory-plan]").inner_text()
        check("a refresh shows the changed address", "Changed since the last import" in plan and "54.1.2.3" in plan and "54.9.9.9" in plan, plan)
        check("it shows what is no longer listed, and keeps it by default", "No longer listed (1)" in plan and "worker" in plan, plan)
        await pg.click("[data-testid=inventory-apply]")
        await pg.wait_for_timeout(600)
        store = await pg.evaluate("window.__store.map((r) => ({ label: r.data.label, hostname: r.data.hostname }))")
        check("the address is updated, the edited name is kept, the missing host is kept", {"label": "my-api", "hostname": "54.9.9.9"} in store and any(s["label"] == "worker" for s in store), store)
        check("no page errors from the import", not pg.errors, pg.errors)
        await pg.close()

        # ---- a program that isn't installed ----
        pg = await fresh(b, "window.__fail = true;")
        await pg.click('button[title^="Import from a cloud"]')
        await pg.wait_for_timeout(400)
        await pg.select_option("#inv-source", "aws")
        await pg.click("[data-testid=inventory-run]")
        await pg.wait_for_timeout(500)
        check("a missing program is said to be missing", await pg.locator("[role=alert]", has_text="isn't installed").count() == 1)
        await pg.close()

        # ---- scanning ----
        pg = await fresh(b)
        await palette(pg, "Scan the network")
        check("the scan opens on its own source", await pg.input_value("#inv-source") == "scan")
        await pg.fill("#inv-range", "192.168.1.0/24")
        await pg.click("[data-testid=inventory-run]")
        await pg.wait_for_timeout(500)
        calls = await pg.evaluate("window.__calls")
        check("it asks for that range and port", calls[-1] == {"cmd": "inventory_scan", "range": "192.168.1.0/24", "port": 22}, calls)
        check("what answered is offered", "192.168.1.5" in await pg.locator("[data-testid=inventory-plan]").inner_text())
        await pg.close()

        # ---- a host that connects with a command ----
        pg = await fresh(b, """
          window.__store.push(mkRec('c1', { label: 'ssm-box', hostname: 'i-0abc.example', port: 22, group: '', tags: [], notes: '', protocol: 'command', connect_command: 'aws ssm start-session --target {id}', source: { provider: 'aws', id: 'i-0abc', scope: '', hostname: 'x', label: 'x' } }));
          window.__store.push(mkRec('c2', { label: 'bad-box', hostname: 'a b', port: 22, group: '', tags: [], notes: '', protocol: 'command', connect_command: 'ssh {host}' }));
        """)
        await palette(pg, "ssm-box")
        await pg.wait_for_timeout(500)
        dialog = await pg.locator("[role=dialog], [role=alertdialog]").last.inner_text()
        check("it shows the exact command and asks first", "aws ssm start-session --target i-0abc" in dialog and "Run it" in dialog, dialog)
        check("nothing has run yet", await pg.locator(".xterm").count() == 0)
        await pg.get_by_role("button", name="Run it").click()
        await pg.wait_for_timeout(1500)
        check("approved, it opens a local terminal", await pg.locator(".xterm").count() >= 1)
        await pg.close()

        pg = await fresh(b, """
          window.__store.push(mkRec('c2', { label: 'bad-box', hostname: 'a b', port: 22, group: '', tags: [], notes: '', protocol: 'command', connect_command: 'ssh {host}' }));
        """)
        await palette(pg, "bad-box")
        await pg.wait_for_timeout(500)
        check("an address that can't safely go on a command line is refused, with the reason", await pg.locator("text=can't safely go on a command line").count() >= 1 and await pg.locator(".xterm").count() == 0)
        await pg.close()

        await b.close()
    print(f"\n{len(failures)} failed" if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
