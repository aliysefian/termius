"""High-velocity file workflows in a real (headless) browser with the backend mocked: either pane can be this computer
or any host (a copy between two hosts shows its relay line), the in-app editor (syntax, metadata, dirty dot, save, the
permission-denied -> sudo -> password flow, a file changed meanwhile), the overwrite dialog (sizes, times, SHA-256, the
identical badge, a diff, backup and replace), and the terminal/browser link (follow the terminal's folder, cd here).

    pnpm dev &
    python3 src/lib/__tests__/e2e/sftpflow.py [chromium-executable]
"""
import asyncio, os, re, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
AXE = open(os.path.join(ROOT, "node_modules", "axe-core", "axe.min.js")).read()
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__calls = [];
window.__transfers = [];
window.__renames = [];
window.__writes = [];
window.__rootWrites = [];
window.__sudoMode = 'denied';   // how the next save behaves
window.__nextWrite = 'denied';
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const base = { port: 22, group: '', tags: [], notes: '' };
const hosts = [
  rec('kish', { ...base, label: 'Kish Server', hostname: '10.0.0.1', identity_id: 'idn', environment: 'production' }),
  rec('claude', { ...base, label: 'Claude', hostname: '10.0.0.2', identity_id: 'idn', environment: 'development' }),
];
const hash = (s) => { let h = 2166136261; for (const c of s) { h ^= c.charCodeAt(0); h = Math.imul(h, 16777619) >>> 0; } return h.toString(16).padStart(8, '0').repeat(8); };
const NGINX_OLD = 'server {\\n  listen 80;\\n  server_name example.com;\\n  root /var/www/html;\\n}\\n';
const NGINX_NEW = 'server {\\n  listen 8080;\\n  server_name example.com;\\n  root /var/www/html;\\n  add_header X-Frame-Options DENY;\\n}\\n';
const file = (path, text, extra = {}) => ({ path, text, size: new TextEncoder().encode(text).length, modified: 1700000000, permissions: 0o644, owner: 'root', group: 'root', ...extra });
const dir = (path) => ({ path, dir: true, size: 0, modified: 1700000000, permissions: 0o755 });
// Each place: path -> entry.
const places = {
  local: { home: '/home/me', files: [file('/home/me/nginx.conf', NGINX_NEW, { modified: 1700005000, owner: 'me', group: 'me' }), file('/home/me/same.txt', 'identical\\n', { owner: 'me', group: 'me' }), file('/home/me/other.txt', 'incoming\\n', { owner: 'me', group: 'me' }), dir('/home/me/proj')] },
  kish: { home: '/etc/nginx', files: [file('/etc/nginx/nginx.conf', NGINX_OLD), dir('/etc/nginx/conf.d'), file('/etc/nginx/conf.d/subdomains.conf', 'server_name a.example.com;\\n'), file('/etc/nginx/app.env', 'KEY=1\\n', { permissions: 0o600 }), file('/etc/nginx/deploy.sh', '#!/bin/sh\\necho hi\\n', { permissions: 0o755 }), dir('/var'), dir('/var/log')] },
  claude: { home: '/srv', files: [file('/srv/nginx.conf', NGINX_OLD), file('/srv/same.txt', 'identical\\n'), file('/srv/other.txt', 'existing\\n'), file('/srv/readme.md', '# hi\\n')] },
};
const sessions = { local: 'local' };
const placeOf = (sid) => places[sessions[sid] ?? 'local'];
const parent = (p) => p.replace(/\\/[^/]*\\/?$/, '') || '/';
const base_ = (p) => p.split('/').filter(Boolean).pop();
const listing = (pl, path) => pl.files.filter((f) => parent(f.path) === path.replace(/\\/$/, '') || (path === '/' && parent(f.path) === '/')).map((f) => ({ name: base_(f.path), path: f.path, is_dir: !!f.dir, is_symlink: false, size: f.size, modified: f.modified, permissions: f.permissions }));
const find = (sid, path) => placeOf(sid).files.find((f) => f.path === path);
window.__invoke = async (cmd, args) => {
  window.__calls.push(cmd);
  if (cmd === 'list_hosts') return hosts;
  if (cmd === 'list_identities') return [rec('idn', { label: 'ops', username: 'alice', auth: { method: 'agent' } })];
  if (cmd === 'local_home') return places.local.home;
  if (cmd === 'sftp_open') { sessions[args.sessionId] = args.hostId; return { home: places[args.hostId].home, new_host_keys: [] }; }
  if (cmd === 'files_close') { delete sessions[args.sessionId]; return null; }
  if (cmd === 'files_caps') return { list: true, mkdir: true, rename: true, delete: true, preview: true, chmod: true, edit: true, resume: true };
  if (cmd === 'files_list') return listing(placeOf(args.sessionId), args.path);
  if (cmd === 'files_rename') { window.__renames.push(args); return null; }
  if (cmd === 'files_read_text') {
    const f = find(args.sessionId, args.path);
    if (!f) throw { code: 'not_found', message: 'no such file' };
    return { text: f.text, crlf: false, size: f.size, modified: f.modified, permissions: f.permissions, owner: f.owner, group: f.group };
  }
  if (cmd === 'files_write_text') {
    window.__writes.push(args);
    const mode = window.__nextWrite;
    if (mode === 'denied') { window.__nextWrite = 'ok'; throw { code: 'permission_denied', message: 'Permission denied' }; }
    if (mode === 'changed') { window.__nextWrite = 'ok'; throw { code: 'changed_on_server', message: 'changed' }; }
    return { size: args.text.length, modified: 1700009999 };
  }
  if (cmd === 'files_write_as_root') {
    window.__rootWrites.push({ ...args });
    if (args.password === null) throw { code: 'sudo_password_required', message: 'sudo needs a password' };
    if (args.password !== 'secret') throw { code: 'sudo_wrong_password', message: 'sudo did not accept that password' };
    return null;
  }
  if (cmd === 'files_checksum') { const f = find(args.sessionId, args.path); await new Promise((r) => setTimeout(r, 120)); return { sha256: hash(f ? f.text : ''), method: sessions[args.sessionId] ? 'host' : 'read' }; }
  if (cmd === 'files_transfer_start') {
    window.__transfers.push(args);
    const ch = args.onProgress; let i = 0;
    const emit = (m) => window['_' + ch.id]({ index: i++, message: m });
    emit({ state: 'started', total_bytes: 100_000_000, total_files: 1 });
    window.__tick = (bytes) => emit({ state: 'progress', bytes, total_bytes: 100_000_000, files_done: 0, total_files: 1, current: 'x' });
    return new Promise(() => {});
  }
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def axe(pg, where, only):
    await pg.evaluate(AXE)
    res = await pg.evaluate(
        """async (only) => {
          const r = await axe.run(document.querySelector(only), { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] } });
          return r.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical').map((v) => v.id + ' x' + v.nodes.length + ' ' + v.nodes[0].target.join(' '));
        }""",
        only,
    )
    check(f"no serious accessibility findings: {where}", not res, res)


def pane(pg, side):
    return pg.locator(f'[aria-label$=" files"]').nth(0 if side == "left" else 1)


async def pick(pg, label, host):
    box = pg.locator(f'input[aria-label="{label}"]')
    await box.click()
    await box.fill(host)
    await pg.keyboard.press("Enter")


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1500, "height": 900})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK)
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)
        await pg.click('button[aria-label="Files"]')
        await pg.wait_for_timeout(600)

        # -- either pane can be any place ---------------------------------------------------------------------------
        left_pick = pg.locator('input[aria-label="Left pane host"]')
        check("the left pane has a host picker, on this computer to begin with", await left_pick.count() == 1 and "This computer" in (await left_pick.input_value()), await left_pick.input_value())
        await pick(pg, "Left pane host", "Kish")
        await pg.wait_for_selector('[aria-label="remote file list"]', timeout=5000)
        await pick(pg, "Remote host", "Claude")
        await pg.wait_for_function("document.querySelectorAll('[aria-label=\"remote file list\"]').length === 2", timeout=5000)
        lists = await pg.evaluate("[...document.querySelectorAll('[aria-label=\"remote file list\"]')].map((l) => l.innerText.replace(/\\s+/g, ' '))")
        check("the left pane shows one host's files and the right another's", "nginx.conf" in lists[0] and "app.env" in lists[0] and "readme.md" in lists[1], lists)
        verbs = await pg.evaluate("[...document.querySelectorAll('button')].map((b) => b.textContent.trim()).filter((t) => /Copy →|← Copy/.test(t))")
        check("copying between two hosts is called Copy, in both directions", "Copy →" in verbs and "← Copy" in verbs, verbs)

        # -- host to host: a relay -----------------------------------------------------------------------------------
        await pg.locator('[aria-label="remote file list"]').first.locator("tbody tr", has_text="deploy.sh").click()
        await pg.get_by_role("button", name="Copy →").click()
        await pg.wait_for_selector('[data-testid="relay-line"]', timeout=5000)
        t = (await pg.evaluate("window.__transfers"))[0]
        check("the copy goes from the left host's session to the right host's, never through this computer", t["sourceId"].startswith("sftp-") and t["destId"].startswith("sftp-") and t["sourceId"] != t["destId"] and "local" not in (t["sourceId"], t["destId"]), t)
        await pg.evaluate("window.__tick(2_000_000)")
        await pg.wait_for_timeout(450)
        await pg.evaluate("window.__tick(42_000_000)")
        await pg.wait_for_timeout(300)
        relay = " ".join((await pg.locator('[data-testid="relay-line"]').inner_text()).split())
        check("the queue shows the relay: source, throughput, buffer, destination", re.search(r"\[Kish Server\] ── \([\d.]+ \w?B/s\) ──▸ \[Relay buffer\] ──▸ \[Claude\]", relay) is not None, relay)
        mono = await pg.locator('[data-testid="relay-line"]').evaluate("(e) => [getComputedStyle(e).fontFamily, getComputedStyle(e).fontVariantNumeric]")
        check("monospace with tabular figures", ("mono" in mono[0].lower() or "JetBrains" in mono[0]) and "tabular-nums" in mono[1], mono)
        row = pg.locator('[data-testid="transfer-row"]')
        check("the row is marked as a relay and carries bytes", await row.get_attribute("data-relay") == "true" and "MB" in await row.inner_text())

        # A copy between this computer and a host is not a relay.
        await pick(pg, "Left pane host", "This computer")
        await pg.wait_for_timeout(500)
        await pg.locator('[aria-label="local file list"]').locator("tbody tr", has_text="proj").first.click()
        await pg.get_by_role("button", name="Upload →").click()
        await pg.wait_for_timeout(500)
        rows = pg.locator('[data-testid="transfer-row"]')
        check("an upload from this computer is not shown as a relay", await rows.count() == 2 and await rows.nth(1).get_attribute("data-relay") == "false")

        # -- the overwrite dialog -------------------------------------------------------------------------------------
        await pg.locator('[aria-label="local file list"]').locator("tbody tr", has_text="same.txt").click()
        await pg.get_by_role("button", name="Upload →").click()
        await pg.wait_for_selector('[data-testid="conflict-dialog"]')
        await pg.wait_for_selector('[data-testid="identical-badge"]', timeout=4000)
        dlg = await pg.locator('[data-testid="conflict-dialog"]').inner_text()
        check("the dialog compares size, time and SHA-256, and says when the files are identical", "Identical file (SHA-256 match)" in dlg and "Size" in dlg and "Modified" in dlg and "SHA-256" in dlg, dlg)
        h1 = await pg.locator('[data-testid="hash-src"]').inner_text()
        h2 = await pg.locator('[data-testid="hash-dst"]').inner_text()
        check("both checksums are 64 hex digits and equal", re.fullmatch(r"[0-9a-f]{64}", h1.strip()) is not None and h1.strip() == h2.strip(), (h1, h2))
        check("no diff is offered for identical files", await pg.locator('[data-testid="compare-button"]').count() == 0)
        await pg.locator('[data-testid="answer-skip"]').click()
        await pg.wait_for_timeout(300)
        check("skipping queues no transfer", await pg.locator('[data-testid="transfer-row"]').count() == 2)

        await pg.locator('[aria-label="local file list"]').locator("tbody tr", has_text="nginx.conf").click()
        await pg.get_by_role("button", name="Upload →").click()
        await pg.wait_for_selector('[data-testid="conflict-dialog"]')
        await pg.wait_for_function("document.querySelector('[data-testid=\"hash-dst\"]') && /[0-9a-f]{64}/.test(document.querySelector('[data-testid=\"hash-dst\"]').innerText)", timeout=4000)
        dlg = await pg.locator('[data-testid="conflict-dialog"]').inner_text()
        check("different files are not called identical", "Identical" not in dlg and "They differ" in dlg, dlg)
        await pg.locator('[data-testid="compare-button"]').click()
        await pg.wait_for_selector('[data-testid="diff-view"]', timeout=4000)
        added = await pg.locator('[data-testid="diff-added"]').inner_text()
        removed = await pg.locator('[data-testid="diff-removed"]').inner_text()
        check("the diff counts the lines that would change", added == "+2" and removed == "−1", (added, removed))
        kinds = await pg.evaluate("[...document.querySelectorAll('[data-testid=\"diff-view\"] [data-kind]')].map((e) => [e.getAttribute('data-kind'), e.className.includes('bg-success') ? 'green' : e.className.includes('bg-danger') ? 'red' : 'plain'])")
        check("additions are green and removals red", ["add", "green"] in kinds and ["del", "red"] in kinds, kinds)
        text = await pg.locator('[data-testid="diff-view"]').inner_text()
        check("with the exact lines", "listen 8080;" in text and "listen 80;" in text and "add_header X-Frame-Options DENY;" in text, text)
        await pg.get_by_role("button", name="Side by side").click()
        check("it can be shown side by side", await pg.locator('[data-testid="diff-view"]').get_attribute("data-layout") == "split")
        await pg.screenshot(path=os.environ.get("SFTP_SHOT", "/tmp/sftp-conflict.png"))
        await axe(pg, "the overwrite dialog", '[data-testid="conflict-dialog"]')
        before = await pg.locator('[data-testid="transfer-row"]').count()
        await pg.locator('[data-testid="answer-backup"]').click()
        await pg.wait_for_timeout(500)
        renames = await pg.evaluate("window.__renames")
        # Two transfers run at once (the mock's never finish), so this one waits its turn in the queue.
        check("Backup & replace renames what is there to .bak, then queues the copy", len(renames) == 1 and renames[0]["path"] == "/srv/nginx.conf" and renames[0]["newName"] == "nginx.conf.bak" and await pg.locator('[data-testid="transfer-row"]').count() == before + 1, renames)

        # Two clashes, one answer for both.
        await pg.locator('[aria-label="local file list"]').locator("tbody tr", has_text="same.txt").click()
        await pg.locator('[aria-label="local file list"]').locator("tbody tr", has_text="other.txt").click(modifiers=["Control"])
        await pg.get_by_role("button", name="Upload →").click()
        await pg.wait_for_selector('[data-testid="conflict-dialog"]')
        check("with several clashes it offers to answer them all at once", "Do this for the other 1 name" in await pg.locator('[data-testid="conflict-dialog"]').inner_text())
        await pg.get_by_label("Do this for the other 1 name").check()
        before = await pg.locator('[data-testid="transfer-row"]').count()
        await pg.locator('[data-testid="answer-skip"]').click()
        await pg.wait_for_timeout(400)
        check("one answer settled both, and a dialog did not come back", await pg.locator('[data-testid="conflict-dialog"]').count() == 0 and await pg.locator('[data-testid="transfer-row"]').count() == before)

        # -- the editor -----------------------------------------------------------------------------------------------
        await pick(pg, "Left pane host", "Kish")
        await pg.wait_for_timeout(600)
        lrow = pg.locator('[aria-label="remote file list"]').first.locator("tbody tr", has_text="nginx.conf")
        await lrow.dblclick()
        await pg.wait_for_selector('[data-testid="remote-editor"]', timeout=5000)
        await pg.wait_for_selector('.cm-content', timeout=5000)
        ed = pg.locator('[data-testid="remote-editor"]')
        check("double-clicking a text file opens it in the editor", await ed.count() == 1)
        check("the header shows the full remote path", await pg.locator('[data-testid="editor-path"]').inner_text() == "/etc/nginx/nginx.conf")
        check("with the environment badge", "PROD" in await ed.locator(".badge").first.inner_text())
        check("permissions and owner", await pg.locator('[data-testid="editor-perms"]').inner_text() == "rw-r--r--" and await pg.locator('[data-testid="editor-owner"]').inner_text() == "root:root")
        check("and the syntax is recognised from the name", await pg.locator('[data-testid="editor-lang"]').inner_text() == "nginx")
        box = await ed.bounding_box()
        check("it is large: 80% of the window wide, 85% tall", abs(box["width"] - 1500 * 0.8) < 4 and abs(box["height"] - 900 * 0.85) < 4, box)
        await pg.wait_for_timeout(600)
        coloured = await pg.evaluate("(() => { const plain = getComputedStyle(document.querySelector('.cm-content')).color; return [...document.querySelectorAll('.cm-line span')].filter((s) => getComputedStyle(s).color !== plain).length; })()")
        check("directives are highlighted in a colour of their own", coloured >= 3, coloured)
        check("nothing unsaved at first", await pg.locator('[data-testid="editor-dirty"]').count() == 0)
        await pg.locator(".cm-content").click()
        await pg.keyboard.press("Control+End")
        await pg.keyboard.type("# edited\n")
        check("typing shows the unsaved dot", await pg.locator('[data-testid="editor-dirty"]').count() == 1)
        await pg.screenshot(path=os.environ.get("SFTP_SHOT2", "/tmp/sftp-editor.png"))
        await pg.keyboard.press("Control+s")
        await pg.wait_for_selector('[data-testid="sudo-bar"]', timeout=4000)
        bar = " ".join((await pg.locator('[data-testid="sudo-bar"]').inner_text()).split())
        check("a refused save offers sudo tee, naming the owner", bar.startswith("Permission denied (owned by root). Save using sudo tee?"), bar)
        w = (await pg.evaluate("window.__writes"))[0]
        check("the save carried the buffer and the time it was opened at", w["text"].endswith("# edited\n") and w["expectModified"] == 1700000000 and w["path"] == "/etc/nginx/nginx.conf", w)
        await axe(pg, "the editor with the sudo bar", '[data-testid="remote-editor"]')
        await pg.get_by_role("button", name="Save with sudo").click()
        await pg.wait_for_selector('[data-testid="sudo-password-bar"]', timeout=4000)
        pw = pg.locator('#sudo-pw')
        check("sudo asks for its password in a masked field", await pw.get_attribute("type") == "password")
        await pw.fill("wrong")
        await pg.keyboard.press("Enter")
        await pg.wait_for_function("document.querySelector('[data-testid=\"sudo-password-bar\"]')?.innerText.includes('did not accept')", timeout=4000)
        check("a wrong password is said so and the field is empty again", await pg.locator('#sudo-pw').input_value() == "")
        await pg.locator('#sudo-pw').fill("secret")
        await pg.keyboard.press("Enter")
        await pg.wait_for_selector('[data-testid="editor-saved"]', timeout=4000)
        root = await pg.evaluate("window.__rootWrites")
        check("sudo is tried without a password first, then with it", [r["password"] for r in root] == [None, "wrong", "secret"], root)
        check("the saved text went through sudo whole", all(r["text"].endswith("# edited\n") for r in root))
        check("saving clears the unsaved dot and the password", await pg.locator('[data-testid="editor-dirty"]').count() == 0 and await pg.locator('#sudo-pw').count() == 0)

        # A file changed on the host meanwhile.
        await pg.evaluate("window.__nextWrite = 'changed'")
        await pg.locator(".cm-content").click()
        await pg.keyboard.press("Control+End")
        await pg.keyboard.type("more")
        await pg.keyboard.press("Control+s")
        await pg.wait_for_selector('[data-testid="changed-bar"]', timeout=4000)
        check("a file changed by someone else is not overwritten unasked", "changed on the host" in await pg.locator('[data-testid="changed-bar"]').inner_text())
        await pg.get_by_role("button", name="Overwrite anyway").click()
        await pg.wait_for_selector('[data-testid="editor-saved"]', timeout=4000)
        last = (await pg.evaluate("window.__writes"))[-1]
        check("Overwrite anyway saves without the check", last["expectModified"] is None)

        # Closing with unsaved changes asks.
        await pg.locator(".cm-content").click()
        await pg.keyboard.type("x")
        await pg.keyboard.press("Escape")
        await pg.wait_for_selector('text=Close without saving')
        await pg.get_by_role("button", name="Discard changes").click()
        await pg.wait_for_timeout(300)
        check("closing with unsaved changes asks first, and discards on yes", await pg.locator('[data-testid="remote-editor"]').count() == 0)

        # Ctrl+E on a selection; YAML is recognised too.
        await pg.locator('[aria-label="remote file list"]').first.locator("tbody tr", has_text="app.env").click()
        await pg.keyboard.press("Control+e")
        await pg.wait_for_selector('[data-testid="remote-editor"]', timeout=4000)
        check("Ctrl+E edits the selected file, and .env is its own syntax", await pg.locator('[data-testid="editor-lang"]').inner_text() == ".env")
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)

        # -- the terminal and the browser -----------------------------------------------------------------------------
        await pg.click('button[aria-label="Hosts"]')
        await pg.wait_for_timeout(300)
        await pg.evaluate("window.__hostsToOpen = 1")
        row = pg.locator('[role="treeitem"]', has_text="Kish Server").first
        await row.dblclick(position={"x": 40, "y": 10})
        await pg.wait_for_selector(".xterm", timeout=8000)
        await pg.wait_for_timeout(1200)
        await pg.click('button[aria-label="Files"]')
        await pg.wait_for_timeout(500)
        follow = pg.locator('[data-testid="follow-cwd"]').first
        check("each open pane has a Follow terminal folder toggle", await pg.locator('[data-testid="follow-cwd"]').count() >= 1 and await follow.get_attribute("aria-pressed") == "false")
        await follow.click()
        await pg.wait_for_timeout(500)
        # The mock shell reports /work/app at its prompt; the browser follows.
        lpath = await pg.evaluate("[...document.querySelectorAll('nav[aria-label=\"Path\"]')].map((n) => n.innerText.replace(/\\s+/g, ''))")
        check("turning Follow on takes the pane to the terminal's folder", any("work/app" in x or "/work/app" in x for x in lpath), lpath)
        await pg.click('button[aria-label="Hosts"]')
        await pg.evaluate("window.__deliver(Array.from(new TextEncoder().encode('\\x1b]7;file://kish/var/log\\x07')))")
        await pg.wait_for_timeout(500)
        await pg.click('button[aria-label="Files"]')
        await pg.wait_for_timeout(500)
        lpath = await pg.evaluate("[...document.querySelectorAll('nav[aria-label=\"Path\"]')].map((n) => n.innerText.replace(/\\s+/g, ''))")
        check("when the terminal changes folder the browser follows", any(x.endswith("var/log") or x.endswith("/var/log") for x in lpath), lpath)
        # Browsing elsewhere is left alone until the terminal moves again.
        await pg.locator('[aria-label="remote files"]').first.locator('nav[aria-label="Path"] button').first.click()
        await pg.wait_for_timeout(700)
        lpath = await pg.evaluate("[...document.querySelectorAll('nav[aria-label=\"Path\"]')].map((n) => n.innerText.replace(/\\s+/g, ''))")
        check("a pane the person browsed elsewhere stays there until the terminal moves again", not any(x.endswith("var/log") for x in lpath[:1]), lpath)

        # cd here
        await pg.evaluate("window.__sent.length = 0")
        confrow = pg.locator('[aria-label="remote file list"]').first.locator("tbody tr", has_text="etc").first
        if await confrow.count() == 0:
            confrow = pg.locator('[aria-label="remote file list"]').first.locator("tbody tr", has_text="var").first
        await confrow.click(button="right")
        await pg.get_by_role("menuitem", name="Open in terminal (cd here)").click()
        await pg.wait_for_timeout(600)
        sent = await pg.evaluate("new TextDecoder().decode(new Uint8Array(window.__sent.flat()))")
        check("cd here types a quoted cd into the terminal of that host", re.search(r"cd '/(etc|var)'\r", sent) is not None, sent)
        check("and brings the terminal forward", await pg.locator(".xterm").first.is_visible())
        check("no page errors", not errors, errors)
        await b.close()


asyncio.run(main())
print("\n%d failure(s)" % len(failures) if failures else "\nall passed")
sys.exit(1 if failures else 0)
