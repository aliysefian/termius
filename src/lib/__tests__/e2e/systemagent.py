"""Hardware keys in Settings, in a real (headless) browser with the backend mocked: the section is found by search,
shows the agent in use and its keys (a security key is marked as needing a touch, an unknown type as not supported),
and choosing another socket is sent to the backend and shown.

    pnpm dev &
    python3 src/lib/__tests__/e2e/systemagent.py [chromium-executable] [url]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = sys.argv[1] if len(sys.argv) > 1 else None
URL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:1420/"
failures = []

MOCK = """
const key = (algorithm, comment, extra) => ({ algorithm, comment, fingerprint: 'SHA256:' + comment, security_key: false, certificate: false, usable: true, ...extra });
const places = [{ path: '/run/user/1000/ssh-agent.socket', from: 'ssh-agent' }, { path: '/run/user/1000/gnupg/S.gpg-agent.ssh', from: 'gpg-agent' }];
window.__agent = { setting: null, calls: [], asked: 0 };
const status = () => {
  const s = window.__agent.setting;
  if (s === '/nowhere') return { source: null, keys: [], error: '/nowhere (Settings): No such file or directory', places: [{ path: s, from: 'Settings' }], setting: s };
  if (s) return { source: s, keys: [key('ssh-ed25519', 'card-key')], error: null, places: [{ path: s, from: 'Settings' }], setting: s };
  return {
    source: places[0].path, error: null, places, setting: null,
    keys: [key('sk-ssh-ed25519@openssh.com', 'yubikey-5c', { security_key: true }), key('ssh-ed25519', 'laptop'), key('ssh-future@example.com', 'odd', { usable: false })],
  };
};
window.__invoke = async (cmd, args) => {
  if (cmd === 'system_agent_status') { window.__agent.asked++; return status(); }
  if (cmd === 'system_agent_set_socket') { window.__agent.calls.push(args.path); window.__agent.setting = args.path; return status(); }
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 900})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK)
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)

        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_timeout(400)
        check("the agent is not asked until its section shows", await pg.evaluate("window.__agent.asked") == 0)
        await pg.fill('input[placeholder="Search…"]', "yubikey")
        await pg.wait_for_timeout(600)
        sec = pg.locator('[data-testid="system-agent"]')
        check("searching 'yubikey' finds the section", await sec.count() == 1)
        text = await sec.inner_text()
        check("the agent in use is named", "/run/user/1000/ssh-agent.socket" in text, text[:300])
        rows = sec.locator('[data-testid="system-agent-keys"] li')
        check("every key is listed", await rows.count() == 3)
        first, third = await rows.nth(0).inner_text(), await rows.nth(2).inner_text()
        check("a security key says it needs a touch", "yubikey-5c" in first and "touch" in first and "SHA256:yubikey-5c" in first, first)
        check("an ordinary key has no such mark", "touch" not in await rows.nth(1).inner_text())
        check("an unknown key type is marked not supported", "not supported" in third, third)
        check("asked once, not in a loop", await pg.evaluate("window.__agent.asked") == 1)
        if len(sys.argv) > 3:
            await sec.screenshot(path=sys.argv[3])

        # Other agents that were found are offered as one-click choices.
        await sec.get_by_role("button", name="gpg-agent").click()
        await pg.wait_for_timeout(400)
        check("a found agent is chosen with one click", await pg.evaluate("window.__agent.calls") == ["/run/user/1000/gnupg/S.gpg-agent.ssh"])
        text = await sec.inner_text()
        check("its keys replace the list", "card-key" in text and "yubikey-5c" not in text, text[:300])
        check("the field shows the chosen socket", await sec.locator("#agent-socket").input_value() == "/run/user/1000/gnupg/S.gpg-agent.ssh")

        # A socket that isn't there: the reason is shown, and the choice can be undone.
        await sec.locator("#agent-socket").fill("  /nowhere ")
        await sec.get_by_role("button", name="Use", exact=True).click()
        await pg.wait_for_timeout(400)
        check("the path is sent trimmed", (await pg.evaluate("window.__agent.calls"))[-1] == "/nowhere")
        err = sec.locator('[data-testid="system-agent-error"]')
        check("a socket that does not answer says why", await err.count() == 1 and "No such file" in await err.inner_text())
        await sec.get_by_role("button", name="Find automatically").click()
        await pg.wait_for_timeout(400)
        check("going back to automatic sends an empty choice", (await pg.evaluate("window.__agent.calls"))[-1] is None)
        check("and the first agent is back", "yubikey-5c" in await sec.inner_text())
        check("no page errors", not errors, "; ".join(errors)[:300])
        await b.close()
    print("\n" + ("ALL CHECKS PASSED" if not failures else f"{len(failures)} FAILED: {failures}"))
    sys.exit(1 if failures else 0)


asyncio.run(main())
