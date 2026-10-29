"""Two-factor codes in a real (headless) browser with the backend mocked: off by default and invisible until switched
on in Settings, then a secret field on a credential with a live code to check it, and what is sent when it is saved.

    pnpm dev &
    python3 src/lib/__tests__/e2e/twofactor.py [chromium-executable]
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

# i1 has a secret saved (a list carries "" for it), i2 has none. The switch lives in window.__totp.
MOCK = """
window.__totp = { enabled: false };
window.__saved = [];
const ident = (id, label, totp) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, username: label, auth: { type: 'password', password: '' }, notes: '', ...(totp === undefined ? {} : { totp }) } });
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_identities') return [ident('i1', 'with-secret', ''), ident('i2', 'no-secret')];
  if (cmd === 'totp_status') return { enabled: window.__totp.enabled };
  if (cmd === 'totp_set_enabled') { window.__totp.enabled = args.enabled; return { enabled: args.enabled }; }
  if (cmd === 'totp_preview') {
    if (/bad/i.test(args.secret)) throw { code: 'validation', message: 'The secret is not valid Base32 (letters A to Z and digits 2 to 7)' };
    return { code: '123456', seconds_left: 17 };
  }
  if (cmd === 'save_identity') {
    // As the real transport sends it: JSON, which leaves out an undefined field.
    window.__saved.push(JSON.parse(JSON.stringify(args.identity)));
    return { id: args.id || 'new', rev: 2, updated_at: 2, deleted: false, data: args.identity };
  }
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def fresh(b):
    pg = await b.new_page(viewport={"width": 1400, "height": 900})
    pg.errors = []
    pg.on("pageerror", lambda e: pg.errors.append(str(e)))
    await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
    await pg.add_init_script(MOCK)
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


async def open_settings_section(pg):
    await pg.click('button[aria-label="Settings"]')
    await pg.wait_for_timeout(400)
    await pg.fill('input[placeholder="Search…"]', "two-factor")
    await pg.wait_for_timeout(300)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()

        # ---- off by default: the setting is there, and nothing else shows ----
        pg = await fresh(b)
        await open_settings_section(pg)
        check("Settings has a Two-factor codes section", await pg.locator('h2:has-text("Two-factor codes")').count() == 1)
        check("it is off: the button offers to turn it on", await pg.locator('section:has(h2:has-text("Two-factor codes")) button:has-text("Turn on")').count() == 1)
        check("and says nothing is sent until turned on", await pg.locator("text=Off by default").count() >= 1)
        check("no instructions or warnings while it is off", await pg.locator("text=Open a credential under").count() == 0 and await pg.locator("text=no longer protects").count() == 0)
        await pg.keyboard.press("Escape")
        await palette(pg, "New credential")
        check("a new credential has no secret field", await pg.locator("#i-totp").count() == 0 and await pg.locator("text=One-time code secret").count() == 0)
        await pg.click('button:has-text("Cancel")')
        await palette(pg, "Credential: with-secret")
        check("a saved secret is not shown, only noted as unused", await pg.locator("#i-totp").count() == 0 and await pg.locator("text=It is not used while two-factor codes are off").count() == 1)
        await pg.click('button.btn-primary:has-text("Save")')
        await pg.wait_for_timeout(400)
        saved = await pg.evaluate("window.__saved")
        check("saving with it off keeps the saved secret (sends an empty one)", len(saved) == 1 and saved[0].get("totp") == "", saved)
        check("no page errors (off)", not pg.errors, pg.errors[:2])
        await pg.close()

        # ---- switched on in Settings ----
        pg = await fresh(b)
        await open_settings_section(pg)
        await pg.click('section:has(h2:has-text("Two-factor codes")) button:has-text("Turn on")')
        await pg.wait_for_timeout(400)
        check("it turns on", await pg.evaluate("window.__totp.enabled") is True and await pg.locator('section:has(h2:has-text("Two-factor codes")) button:has-text("Turn off")').count() == 1)
        check("and explains what it needs and what it costs", await pg.locator("text=Open a credential under").count() == 1 and await pg.locator("text=no longer protects").count() == 1)
        await pg.keyboard.press("Escape")

        await palette(pg, "New credential")
        check("a new credential now has the secret field", await pg.locator("#i-totp").count() == 1)
        await pg.fill("#i-totp", "gezd gnbv gy3t")
        await pg.wait_for_timeout(1300)
        check("the code it gives is shown to compare with the app", await pg.locator("text=Code now:").count() == 1 and await pg.locator("text=123456").count() == 1 and await pg.locator("text=changes in 17 s").count() == 1)
        await pg.fill("#i-totp", "bad secret")
        await pg.wait_for_timeout(1300)
        check("a bad secret shows why instead", await pg.locator("text=not valid Base32").count() == 1 and await pg.locator("text=Code now:").count() == 0)
        await pg.fill("#i-totp", "GEZDGNBVGY3TQOJQ")
        await pg.fill("#i-label", "bastion")
        await pg.fill("#i-user", "alice")
        await pg.fill("#i-pw", "pw")
        await pg.click('button.btn-primary:has-text("Save")')
        await pg.wait_for_timeout(400)
        saved = await pg.evaluate("window.__saved")
        check("the typed secret is what is saved", len(saved) == 1 and saved[0].get("totp") == "GEZDGNBVGY3TQOJQ", saved)

        await palette(pg, "New credential")
        await pg.click('button:has-text("SSH agent")')
        check("an ssh-agent credential has no secret field", await pg.locator("#i-totp").count() == 0)
        await pg.click('button:has-text("Password")')
        check("it comes back for a password", await pg.locator("#i-totp").count() == 1)
        await pg.fill("#i-label", "nosecret")
        await pg.fill("#i-user", "bob")
        await pg.fill("#i-pw", "pw")
        await pg.click('button.btn-primary:has-text("Save")')
        await pg.wait_for_timeout(400)
        saved = await pg.evaluate("window.__saved")
        check("a credential left without one sends none", len(saved) == 2 and "totp" not in saved[1], saved[1:])

        await palette(pg, "Credential: with-secret")
        check("a saved secret is offered for reveal and removal, and the field says it is saved", await pg.locator('button:has-text("Reveal saved")').count() >= 1 and await pg.locator('button:has-text("Remove")').count() == 1 and "Saved. Leave empty to keep it." in (await pg.get_attribute("#i-totp", "placeholder") or ""))
        await pg.click('button.btn-primary:has-text("Save")')
        await pg.wait_for_timeout(400)
        saved = await pg.evaluate("window.__saved")
        check("leaving it empty keeps it", len(saved) == 3 and saved[2].get("totp") == "", saved[2:])

        await palette(pg, "Credential: with-secret")
        await pg.click('button:has-text("Remove")')
        check("Remove says it happens on save, and can be undone", await pg.locator("text=removed when you save").count() == 1 and await pg.locator('button:has-text("Undo")').count() == 1)
        await pg.click('button.btn-primary:has-text("Save")')
        await pg.wait_for_timeout(400)
        saved = await pg.evaluate("window.__saved")
        check("removing sends no secret", len(saved) == 4 and "totp" not in saved[3], saved[3:])
        check("no page errors (on)", not pg.errors, pg.errors[:2])
        await pg.close()

        await b.close()
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
