"""The MobaXterm import window in a real (headless) browser with the backend mocked: a `.mobaconf` file can be chosen,
the master-password box is empty and says it is only for people who set one, and what the window sends to the backend.

    pnpm dev &
    python3 src/lib/__tests__/e2e/mobaximport.py [chromium-executable]

The decryption itself is Rust and is tested there (src-tauri/src/mobaxterm.rs); this checks the window around it.
Exits with an error on the first thing that is not as expected.
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
SHOT = os.environ.get("SHOT")

FILE = "C:\\\\Users\\\\me\\\\MobaXterm configuration (2026-10-28 18.20.34).mobaconf"

SETUP = f"""
window.__pickedFile = '{FILE}';
window.__calls = [];
const host = (alias, hostname, over) => ({{ alias, hostname, port: 22, user: 'ops', identity_file: null, proxy_jump: null, forward_agent: false, forward_x11: false, group: 'Prod', has_password: false, ...over }});
window.__previewHosts = [host('web-01', '10.0.0.5', {{ has_password: true }}), host('web-02', '10.0.0.6', {{ has_password: true }}), host('db-01', '10.0.0.9', {{}})];
window.__importError = null;
window.__invoke = async (cmd, args) => {{
  if (cmd === 'mobaxterm_preview') {{
    window.__calls.push({{ cmd, path: args.path }});
    return {{ path: args.path, hosts: window.__previewHosts, warnings: window.__previewWarnings ?? [], existing: [] }};
  }}
  if (cmd === 'mobaxterm_import') {{
    window.__calls.push({{ cmd, importPasswords: args.importPasswords, masterPassword: args.masterPassword, hosts: args.hosts.map((h) => h.alias) }});
    if (window.__importError) throw window.__importError;
    return {{ hosts_created: args.hosts.length, identities_created: 2, keys_imported: 0, proxies_created: 0, forwards_created: 0, passwords_imported: args.importPasswords ? 2 : 0, skipped_existing: [], warnings: [] }};
  }}
  return undefined;
}};
"""

failures = []


def check(ok, what):
    print(("ok    " if ok else "FAIL  ") + what)
    if not ok:
        failures.append(what)


async def open_dialog(pg):
    await pg.click('button[title^="Import from"]')
    await pg.get_by_role("button", name="MobaXterm…").click()
    await pg.wait_for_selector("text=web-01")


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1300, "height": 950})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(SETUP)
        await pg.goto(URL)
        await pg.wait_for_timeout(9000)

        await open_dialog(pg)
        calls = await pg.evaluate("window.__calls")
        check(bool(calls) and calls[0]["cmd"] == "mobaxterm_preview" and calls[0]["path"].endswith(".mobaconf"), "a .mobaconf file is previewed")
        check(await pg.get_by_text("Import MobaXterm bookmarks").count() >= 1, "the window is the MobaXterm one")
        check(await pg.get_by_text("Import the 2 saved passwords too").count() == 1, "it says how many hosts have a saved password")
        master = pg.locator("#imp-master")
        check(await master.input_value() == "", "the master password box starts empty (no user@computer guess)")
        check("only if you set one" in (await pg.locator("label[for=imp-master]").inner_text()).lower(), "and says it is only for people who set one")
        body = await pg.locator("body").inner_text()
        check("user@computer" not in body, "nothing tells people to type user@computer any more")
        if SHOT:
            await pg.screenshot(path=SHOT)

        # A file with no master password: nothing typed in.
        await pg.get_by_role("button", name="Import 3 hosts").click()
        await pg.wait_for_selector("text=Done")
        sent = (await pg.evaluate("window.__calls"))[-1]
        check(sent["cmd"] == "mobaxterm_import" and sent["importPasswords"] is True and sent["masterPassword"] is None, f"passwords on, no master password sent: {sent}")

        # A master password typed in is sent as typed.
        await pg.get_by_role("button", name="Done").click()
        await open_dialog(pg)
        await master.fill("hunter2")
        await pg.get_by_role("button", name="Import 3 hosts").click()
        await pg.wait_for_selector("text=Done")
        sent = (await pg.evaluate("window.__calls"))[-1]
        check(sent["importPasswords"] is True and sent["masterPassword"] == "hunter2", f"the master password is sent: {sent['masterPassword']!r}")

        # Unticked: the backend is told not to read passwords, and no master password goes with it.
        await pg.get_by_role("button", name="Done").click()
        await open_dialog(pg)
        await pg.get_by_label("Import the 2 saved passwords too").uncheck()
        check(await pg.locator("#imp-master").count() == 0, "unticking hides the master password box")
        await pg.get_by_role("button", name="Import 3 hosts").click()
        await pg.wait_for_selector("text=Done")
        sent = (await pg.evaluate("window.__calls"))[-1]
        check(sent["importPasswords"] is False, f"importing passwords can be switched off: {sent}")

        # What the backend says when the file needs a master password is shown, and the window stays open to retry.
        await pg.get_by_role("button", name="Done").click()
        await pg.evaluate("window.__importError = { code: 'master_password_needed', message: 'These saved passwords are protected by your MobaXterm master password. Enter it and import again.' }")
        await open_dialog(pg)
        await pg.get_by_role("button", name="Import 3 hosts").click()
        await pg.wait_for_selector("text=protected by your MobaXterm master password")
        check(await pg.get_by_role("button", name="Import 3 hosts").is_enabled(), "after that error the import can be tried again")
        await pg.evaluate("window.__importError = null")
        await pg.locator("#imp-master").fill("right")
        await pg.get_by_role("button", name="Import 3 hosts").click()
        await pg.wait_for_selector("text=Done")
        check((await pg.evaluate("window.__calls"))[-1]["masterPassword"] == "right", "and it works with the password typed in")
        await pg.get_by_role("button", name="Done").click()

        # A file with no passwords says what it does have.
        await pg.evaluate("window.__previewHosts = window.__previewHosts.map((h) => ({ ...h, has_password: false })); window.__previewWarnings = ['No saved passwords were found in this file. Its sections: Misc, Display.']")
        await open_dialog(pg)
        check(await pg.get_by_text("Its sections: Misc, Display.").count() == 1, "a file with no passwords lists its sections")
        check(await pg.get_by_text("This file holds no saved passwords").count() == 1, "and the window says there are none")
        check(await pg.locator("#imp-master").count() == 0, "with no master password box")
        check(not errors, f"no page errors {errors[:2]}")
        await b.close()
    print(f"\n{len(failures)} failed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
