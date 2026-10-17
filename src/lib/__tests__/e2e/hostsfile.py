"""The encrypted host export and import dialog in a real (headless) browser with the backend mocked: export asks
for a matching passphrase, saves through the Save dialog and says what it wrote; import says so when the passphrase
is wrong, then lists the hosts whose names clash, lets each be renamed or skipped, sends those answers, and says
what was added.

    pnpm dev &
    python3 src/lib/__tests__/e2e/hostsfile.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__calls = [];
window.__pickedFile = '/tmp/team.svhosts';
window.__invoke = async (cmd, args) => {
  const host = (id, label, group) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group, tags: [], notes: '', identity_id: null } });
  if (cmd === 'list_hosts') return [host('h1', 'web-01', 'Prod'), host('h2', 'db-01', 'Prod/DB')];
  if (cmd === 'plugin:dialog|save') return '/tmp/out';
  if (cmd === 'hosts_export') { window.__calls.push({ cmd, args }); return { hosts: 2, groups: 2, identities: 1, keys: 0, proxies: 0, secrets: args.options.include_secrets }; }
  if (cmd === 'hosts_import_preview') {
    window.__calls.push({ cmd, args });
    if (args.passphrase !== 'right horse') throw { code: 'wrong_passphrase', message: 'Wrong passphrase, or the file is damaged.' };
    return { exported_at: 1700000000, hosts: 3, groups: 2, identities: 1, keys: 0, proxies: 0, secrets: true, notes: ['1 host runs a command on this computer when you connect. You are asked to approve each one the first time.'],
      conflicts: [{ id: 'a', label: 'web-01', group: 'Prod', suggested: 'web-01 (2)' }, { id: 'b', label: 'db-01', group: 'Prod/DB', suggested: 'db-01 (2)' }] };
  }
  if (cmd === 'hosts_import_apply') { window.__calls.push({ cmd, args }); return { hosts_added: 2, hosts_renamed: 1, hosts_skipped: 1, groups_added: 1, identities_added: 1, keys_added: 0, proxies_added: 0, warnings: ['one warning'] }; }
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
        pg = await b.new_page(viewport={"width": 1400, "height": 800})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK)
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)
        await pg.click('button[title="Export or import hosts as an encrypted file"]')
        await pg.wait_for_selector('[role="tablist"]')

        # -- export
        btn = pg.get_by_role("button", name="Export…")
        check("export is off until a passphrase is typed twice", await btn.is_disabled())
        await pg.fill("#hx-pass", "short")
        await pg.fill("#hx-pass2", "short")
        check("a short passphrase is refused", await btn.is_disabled())
        await pg.fill("#hx-pass", "correct horse battery")
        await pg.fill("#hx-pass2", "correct horse")
        check("two different passphrases are refused", await btn.is_disabled() and "different" in await pg.inner_text("body"))
        await pg.fill("#hx-pass2", "correct horse battery")
        check("matching strong passphrases enable it", not await btn.is_disabled())
        await btn.click()
        await pg.wait_for_selector("text=Saved an encrypted file")
        call = await pg.evaluate("window.__calls.find((c) => c.cmd === 'hosts_export')")
        check("it saves to the chosen file with a .svhosts name, secrets included, all hosts", call and call["args"]["path"] == "/tmp/out.svhosts" and call["args"]["options"] == {"include_secrets": True, "group": None} and call["args"]["passphrase"] == "correct horse battery", call)
        check("it says what it wrote", "2 hosts in 2 groups" in await pg.inner_text("body"))

        # -- import
        await pg.get_by_role("tab", name="Import").click()
        await pg.get_by_role("button", name="Choose file…").click()
        await pg.wait_for_selector("text=team.svhosts")
        await pg.fill("#hx-import-pass", "wrong")
        await pg.get_by_role("button", name="Open file").click()
        await pg.wait_for_selector('[role="alert"]')
        check("a wrong passphrase is said plainly", "Wrong passphrase" in await pg.inner_text('[role="alert"]'))
        await pg.fill("#hx-import-pass", "right horse")
        await pg.get_by_role("button", name="Open file").click()
        await pg.wait_for_selector("text=already have the same name")
        body = await pg.inner_text("body")
        check("the clashing names are listed with their groups", "web-01" in body and "in Prod/DB" in body and "db-01" in body)
        check("a note about commands is shown", "runs a command on this computer" in body)
        name = pg.get_by_label("New name for web-01")
        check("each is offered a free new name", await name.input_value() == "web-01 (2)")
        await name.fill("web-01 imported")
        await pg.get_by_label("New name for db-01").fill("DB-01")
        imp = pg.locator("button.btn-primary", has_text="Import")
        check("a name equal to the clashing one blocks the import", await imp.is_disabled() and "already in use" in await pg.inner_text("body"))
        await pg.locator('input[name="c-b"]').nth(1).check()
        check("skipping that host unblocks it, and the button counts one less", not await imp.is_disabled() and "Import 2 hosts" in await imp.inner_text(), await imp.inner_text())
        await imp.click()
        await pg.wait_for_selector("text=Import finished")
        apply = await pg.evaluate("window.__calls.find((c) => c.cmd === 'hosts_import_apply')")
        check("the answers sent are the rename and the skip", apply and apply["args"]["resolutions"] == [{"id": "a", "action": "rename", "label": "web-01 imported"}, {"id": "b", "action": "skip"}], apply)
        done = await pg.inner_text("body")
        check("the result is described, with warnings", "Added 2 hosts, 1 under a new name." in done and "Skipped 1 host." in done and "one warning" in done)
        check("no page errors", not errors, errors)
        await b.close()


asyncio.run(main())
print("\n%d failure(s)" % len(failures) if failures else "\nall passed")
sys.exit(1 if failures else 0)
