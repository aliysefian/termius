"""Remote desktop (feat2 4.7) in a real (headless) browser with the backend mocked: a VNC host asks for its
password, says when it is wrong, shows the screen and sends input; a host with a MAC address can be woken; the
host form has the VNC and Wake-on-LAN settings.

    pnpm dev &
    python3 src/lib/__tests__/e2e/remote.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__calls = [];
window.__mode = 'password';
window.__invoke = async (cmd, args) => {
  const mk = (id, label, extra) => ({ id, rev: 1, updated_at: 1, deleted: false, data: Object.assign({ label, hostname: label + '.example', port: 5900, group: '', tags: [], notes: '', protocol: 'vnc', identity_id: 'i1', vnc: { ssh_tunnel: true, ssh_port: 22 } }, extra || {}) });
  if (cmd === 'list_hosts') return [mk('v1', 'desk-01'), mk('v2', 'lab-pc', { wol_mac: '00:1A:2B:3C:4D:5E', wol_broadcast: '192.168.1.255' })];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: 'x' }, notes: '' } }];
  if (cmd === 'vnc_connect') {
    window.__calls.push({ cmd, password: args.password });
    if (window.__mode === 'fail') throw { code: 'vnc', message: 'couldn\\'t reach lab-pc.example' };
    if (!args.password) throw { code: 'vnc_password_required', message: 'this server needs a password' };
    if (args.password !== 'right') throw { code: 'vnc_bad_password', message: 'wrong password' };
    window['_' + args.onEvent.id]({ index: 0, message: [1, 4, 0, 3, 0] });
    return null;
  }
  if (cmd === 'vnc_input') { window.__calls.push({ cmd, input: args.input }); return null; }
  if (cmd === 'wake_on_lan') { window.__calls.push({ cmd, mac: args.mac, broadcast: args.broadcast }); return null; }
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b, mode="password"):
    pg = await b.new_page(viewport={"width": 1400, "height": 900})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(MOCK.replace("'password';", f"'{mode}';", 1))
    await pg.goto(URL)
    await pg.evaluate("() => localStorage.clear()")
    await pg.goto(URL)
    await pg.wait_for_timeout(2500)
    return pg


async def open_host(pg, name):
    await pg.keyboard.press("Control+Shift+P")
    await pg.wait_for_timeout(300)
    await pg.keyboard.type(name)
    await pg.wait_for_timeout(300)
    await pg.keyboard.press("Enter")
    await pg.wait_for_timeout(1500)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- the password, the wrong password, the screen, input ----
        pg = await fresh(b)
        await open_host(pg, "desk-01")
        check("a VNC server that wants a password is asked for it", await pg.locator("[data-testid=vnc-password]").count() == 1)
        check("the first try went without one", (await pg.evaluate("window.__calls"))[0]["password"] is None)
        await pg.fill("[data-testid=vnc-password] input", "wrong")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(600)
        check("a wrong password is said to be wrong, and asked again", await pg.locator("[data-testid=vnc-password]").count() == 1 and await pg.locator("text=wasn't accepted").count() == 1)
        await pg.fill("[data-testid=vnc-password] input", "right")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(800)
        check("the right password shows the screen", await pg.locator("[data-testid=vnc-password]").count() == 0 and await pg.locator("canvas").count() >= 1)
        bar = await pg.evaluate("[...document.querySelectorAll('span.mr-auto')].map((e) => e.textContent.trim())")
        check("the toolbar shows the host without the default VNC port", "desk-01.example" in bar, bar)
        await pg.click("[role=application]", position={"x": 20, "y": 20})
        await pg.keyboard.press("KeyA")
        await pg.wait_for_timeout(300)
        calls = await pg.evaluate("window.__calls")
        check("typing is sent to the VNC session, not to RDP", any(c["cmd"] == "vnc_input" and c["input"].get("code") == "KeyA" for c in calls), calls)
        check("no page errors from VNC", not pg.errors, pg.errors)
        await pg.close()

        # ---- a machine that can be woken ----
        pg = await fresh(b, "fail")
        await open_host(pg, "lab-pc")
        check("a failed connection says why", await pg.locator("text=couldn't reach").count() >= 1)
        check("a host with a MAC address offers to wake it", await pg.get_by_role("button", name="Wake it up").count() == 1)
        await pg.get_by_role("button", name="Wake it up").click()
        await pg.wait_for_timeout(400)
        calls = await pg.evaluate("window.__calls")
        check("the packet is for that MAC and network", any(c["cmd"] == "wake_on_lan" and c["mac"] == "00:1A:2B:3C:4D:5E" and c["broadcast"] == "192.168.1.255" for c in calls), calls)
        check("it says it was sent", await pg.locator("text=Wake-up sent").count() == 1)
        await pg.close()

        # ---- the host form ----
        pg = await fresh(b)
        await pg.click('button[title="New host"]')
        await pg.wait_for_timeout(600)
        await pg.select_option("#h-proto", "vnc")
        await pg.wait_for_timeout(300)
        check("choosing VNC sets its port and shows the tunnel option", await pg.input_value("#h-port") == "5900" and await pg.locator("[data-testid=vnc-options]").count() == 1)
        check("the tunnel is on by default", await pg.locator("[data-testid=vnc-options] input[type=checkbox]").first.is_checked())
        await pg.get_by_role("tab", name="Automation").click()
        await pg.wait_for_timeout(300)
        await pg.fill("#h-wol-mac", "nonsense")
        await pg.wait_for_timeout(200)
        check("a bad MAC address can't be sent", await pg.get_by_role("button", name="Send a wake-up now").is_disabled())
        await pg.fill("#h-wol-mac", "001a2b3c4d5e")
        await pg.wait_for_timeout(200)
        check("a good one can", await pg.get_by_role("button", name="Send a wake-up now").is_enabled())
        await pg.screenshot(path="/tmp/remote-form.png")
        check("no page errors from the form", not pg.errors, pg.errors)
        await pg.close()

        await b.close()
    print(f"\n{len(failures)} failed" if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
