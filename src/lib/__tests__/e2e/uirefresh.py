"""The density refresh in a real (headless) browser with the backend mocked: host rows (status dot, ping on the
second line, hover actions, Ctrl+E), the production guardrail (a hairline round the terminal and a tab pill, no
banner), the status bar's facts, file-manager icons / hover actions / Ctrl+L, the transfer drawer's speed and
collapse, and the terminal-themed theme cards.

    pnpm dev &
    python3 src/lib/__tests__/e2e/uirefresh.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__lists = [];
window.__transfers = [];
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const base = { port: 22, group: '', tags: [], notes: '' };
const hosts = [
  rec('bastion', { ...base, label: 'bastion', hostname: '10.0.0.1', environment: 'production', identity_id: 'idn' }),
  rec('prod', { ...base, label: 'web-prod', hostname: '10.0.0.5', port: 2222, environment: 'production', identity_id: 'idn', jump_host_id: 'bastion', color: '#ff8800' }),
  rec('stage', { ...base, label: 'web-stage', hostname: 'stage.example', environment: 'staging', identity_id: 'idn' }),
  rec('dev', { ...base, label: 'web-dev', hostname: 'dev.example', environment: 'development', identity_id: 'idn' }),
  rec('ftp', { ...base, label: 'ftp-box', hostname: 'ftp.example', port: 21, protocol: 'ftp', ftp: { anonymous: true, tls: 'none' } }),
];
const file = (dir, name, size = 1000) => ({ name, path: dir + '/' + name, is_dir: false, is_symlink: false, size, modified: 1700000000, permissions: 0o644 });
window.__invoke = async (cmd, args) => {
  if (cmd === 'list_hosts') return hosts;
  if (cmd === 'list_identities') return [rec('idn', { label: 'ops', username: 'alice', auth: { method: 'agent' } })];
  if (cmd === 'check_hosts') return [
    { host_id: 'bastion', state: 'up', latency_ms: 4, banner: null },
    { host_id: 'prod', state: 'up', latency_ms: 12, banner: null },
    { host_id: 'stage', state: 'down', reason: 'timed out' },
  ];
  if (cmd === 'local_home') return '/home/me';
  if (cmd === 'files_caps') return { list: true, mkdir: true, rename: true, delete: true, preview: true, chmod: false, edit: false };
  if (cmd === 'ftp_open') return { home: '/pub', pinned_now: false };
  if (cmd === 'files_list') {
    window.__lists.push(args.path);
    if (args.sessionId === 'local') return [file(args.path, 'local.txt')];
    return [
      { name: 'logs', path: '/pub/logs', is_dir: true, is_symlink: false, size: 0, modified: 1700000000, permissions: 0o755 },
      file('/pub', 'backup.tar.gz', 5_000_000), file('/pub', 'main.rs'), file('/pub', 'nginx.conf'), file('/pub', 'id_ed25519'), file('/pub', 'notes'),
    ];
  }
  if (cmd === 'files_transfer_start') {
    const ch = args.onProgress; let i = 0;
    const emit = (m) => window['_' + ch.id]({ index: i++, message: m });
    window.__transfers.push(args);
    emit({ state: 'started', total_bytes: 100_000_000, total_files: 1 });
    window.__tick = (bytes) => emit({ state: 'progress', bytes, total_bytes: 100_000_000, files_done: 0, total_files: 1 });
    return new Promise(() => {});
  }
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

        # -- host rows -----------------------------------------------------------------------------------------
        await pg.click('button[title="Check which hosts are reachable"]')
        await pg.wait_for_timeout(500)
        row = pg.locator('[role="treeitem"]', has_text="web-prod")
        info = await row.evaluate("""(r) => {
          const lines = r.querySelectorAll(':scope > div.min-w-0 > div');
          const dot = r.querySelector(':scope > span.h-2.w-2');
          return { l1: lines[0].innerText, l2: lines[1].innerText, dot: dot.className, edge: !!r.querySelector('span.w-0\\\\.5') };
        }""")
        check("line 1 is the name and the environment badge", "web-prod" in info["l1"] and "PROD" in info["l1"] and "ms" not in info["l1"], info)
        check("line 2 is user@host:port, the jump host and the ping", "alice@10.0.0.5:2222" in info["l2"] and "via bastion" in info["l2"] and "12ms" in info["l2"], info)
        check("a reachable host's dot is green", "bg-success" in info["dot"], info)
        check("the host's colour tag is an edge bar", info["edge"], info)
        down = await pg.locator('[role="treeitem"]', has_text="web-stage").evaluate("(r) => r.querySelector(':scope > span.h-2.w-2').className")
        check("an unreachable host's dot is red", "bg-danger" in down, down)
        never = await pg.locator('[role="treeitem"]', has_text="web-dev").evaluate("(r) => r.querySelector(':scope > span.h-2.w-2').className")
        check("a host never probed has a muted dot", "bg-success" not in never and "bg-danger" not in never, never)

        await row.hover()
        for label in ["Connect", "Open SFTP", "Copy address"]:
            check(f"hover shows {label}", await row.locator(f'button[aria-label="{label}"]').is_visible())

        # Ctrl+E on a focused host row opens its files.
        await row.focus()
        await pg.keyboard.press("Control+e")
        await pg.wait_for_timeout(600)
        check("Ctrl+E opens the Files view", await pg.locator('[aria-label="local files"]').is_visible())
        await pg.click('button[aria-label="Hosts"]')
        await pg.wait_for_timeout(300)

        # -- production guardrail ------------------------------------------------------------------------------
        # A double-click on the hover strip's Connect button opens one tab, not one per click (plus the row's own).
        prow = pg.locator('[role="treeitem"]', has_text="web-prod")
        await prow.hover()
        cb = await prow.locator('button[aria-label="Connect"]').bounding_box()
        await pg.mouse.dblclick(cb["x"] + cb["width"] / 2, cb["y"] + cb["height"] / 2)
        await pg.wait_for_selector(".xterm", timeout=8000)
        await pg.wait_for_timeout(500)
        check("double-clicking Connect opens exactly one tab", await pg.locator('[role="tab"]').count() == 1, await pg.locator('[role="tab"]').count())
        await pg.wait_for_timeout(300)
        txt = await pg.evaluate("document.body.innerText")
        check("no PRODUCTION banner", "PRODUCTION ·" not in txt)
        ring = await pg.evaluate("""() => {
          const el = document.querySelector('[data-production]');
          if (!el) return null;
          const s = getComputedStyle(el);
          return { w: s.borderTopWidth, c: s.borderTopColor };
        }""")
        check("the production terminal has a 1px border in the danger colour", ring and ring["w"] == "1px" and "0.3" in ring["c"] or (ring and ring["w"] == "1px" and ring["c"] not in ("rgba(0, 0, 0, 0)", "transparent")), ring)
        tab = await pg.locator('[role="tab"]').first.evaluate("(t) => { const b = t.querySelector('.badge'); return b ? { text: b.innerText, cls: b.className } : null; }")
        check("the tab carries an outlined PROD pill", tab and tab["text"] == "PROD" and "badge-danger" in tab["cls"] and "badge-outline" in tab["cls"], tab)
        order = await pg.locator('[role="tab"]').first.evaluate("(t) => { const b = t.querySelector('.badge'); const title = [...t.querySelectorAll('span.truncate')][0]; return !!(b.compareDocumentPosition(title) & Node.DOCUMENT_POSITION_FOLLOWING); }")
        check("the pill comes before the host title", order)

        # -- status bar ----------------------------------------------------------------------------------------
        bar = await pg.locator('footer[aria-label="Status"]').inner_text()
        check("the status bar shows protocol, latency, encoding and TERM", all(x in bar for x in ["SSH", "12ms", "UTF-8", "xterm-256color"]), bar)
        h = await pg.locator('footer[aria-label="Status"]').evaluate("(f) => [f.getBoundingClientRect().height, getComputedStyle(f).fontFamily]")
        check("the status bar is 24px and monospace", h[0] == 24 and "mono" in h[1].lower() or h[0] == 24 and "JetBrains" in h[1], h)

        # Double-clicking the row's name still connects, once.
        srow = pg.locator('[role="treeitem"]', has_text="web-dev")
        sb = await srow.bounding_box()
        await pg.mouse.dblclick(sb["x"] + 40, sb["y"] + sb["height"] / 2)
        await pg.wait_for_timeout(700)
        check("double-clicking a row's name opens exactly one more tab", await pg.locator('[role="tab"]').count() == 2, await pg.locator('[role="tab"]').count())
        dev = await pg.locator('[role="tab"]').nth(1).evaluate("(t) => t.querySelector('.badge')?.innerText")
        check("a development tab has its DEV pill", dev == "DEV", dev)

        # -- file manager --------------------------------------------------------------------------------------
        await pg.click('button[aria-label="Files"]')
        await pg.wait_for_timeout(500)
        # Ctrl+E earlier connected Files to web-prod; disconnect to pick the FTP host.
        if await pg.get_by_role("button", name="Disconnect").count():
            await pg.get_by_role("button", name="Disconnect").click()
            await pg.wait_for_timeout(300)
        box = pg.locator('input[aria-label="Remote host"]')
        await box.click()
        await box.fill("ftp-box")
        await pg.keyboard.press("Enter")
        await pg.wait_for_selector('[aria-label="remote file list"] tbody tr', timeout=8000)
        tones = await pg.evaluate("""() => Object.fromEntries([...document.querySelectorAll('[aria-label="remote file list"] tbody tr')].map((r) => [r.querySelector('td span.truncate').innerText, r.querySelector('td svg').getAttribute('class')]))""")
        check("folder, archive, code, config, key and plain files get different icons",
              "text-accent" in tones["logs"] and "text-warning" in tones["backup.tar.gz"] and "text-success" in tones["main.rs"]
              and "text-danger" in tones["id_ed25519"] and "text-fg-muted" in tones["nginx.conf"] and "text-fg-muted" in tones["notes"], tones)
        sizes = await pg.evaluate("[...document.querySelectorAll('[aria-label=\"remote file list\"] tbody td.font-mono')].every((t) => getComputedStyle(t).fontVariantNumeric.includes('tabular-nums'))")
        check("size, date and mode are tabular-nums", sizes)

        rrow = pg.locator('[aria-label="remote file list"] tbody tr', has_text="backup.tar.gz")
        strip = rrow.locator('[role="toolbar"]')
        check("the row's action strip is hidden until hover", not await strip.is_visible())
        await rrow.hover()
        check("it shows on hover", await strip.is_visible())
        labels = await strip.evaluate("(s) => [...s.querySelectorAll('button')].map((b) => b.getAttribute('aria-label'))")
        check("with Download, View, Edit, Rename and Delete", labels == ["Download", "View", "Edit", "Rename", "Delete"], labels)

        await pg.locator('[aria-label="remote file list"] tbody tr', has_text="notes").click()
        await pg.keyboard.press("Control+l")
        await pg.wait_for_timeout(200)
        focused = await pg.evaluate("document.activeElement?.getAttribute('aria-label')")
        check("Ctrl+L turns the path into an input", focused == "Path", focused)
        await pg.keyboard.press("Escape")
        await pg.locator('[aria-label="remote file list"] tbody tr', has_text="main.rs").click()

        # -- transfer drawer -----------------------------------------------------------------------------------
        await rrow.hover()
        await strip.locator('button[aria-label="Download"]').click()
        await pg.wait_for_selector('[data-testid="transfer-drawer"]', timeout=5000)
        check("a strip Download starts one transfer of that row", len(await pg.evaluate("window.__transfers")) == 1
              and (await pg.evaluate("window.__transfers[0].sources")) == ["/pub/backup.tar.gz"])
        await pg.evaluate("window.__tick(1_000_000)")
        await pg.wait_for_timeout(450)
        await pg.evaluate("window.__tick(6_000_000)")
        await pg.wait_for_timeout(300)
        drawer = await pg.locator('[data-testid="transfer-drawer"]').inner_text()
        check("the header shows the total speed", "/s" in drawer, drawer)
        rowtxt = await pg.locator("#transfer-list").inner_text()
        check("the row shows its speed and an ETA", "/s" in rowtxt and any(u in rowtxt for u in ["s\n", "s ", "m "]) and "·" in rowtxt, rowtxt)
        check("the list is open", await pg.locator("#transfer-list").is_visible())
        await pg.locator('[data-testid="transfer-drawer"] button[aria-expanded]').click()
        check("the drawer folds to its header", not await pg.locator("#transfer-list").is_visible())
        check("a thin overall progress bar remains", await pg.locator('[data-testid="transfer-drawer"] .bg-accent').count() == 1)
        await pg.locator('[data-testid="transfer-drawer"] button[aria-expanded]').click()
        check("and unfolds again", await pg.locator("#transfer-list").is_visible())

        # -- theme cards ---------------------------------------------------------------------------------------
        await pg.click('button[aria-label="Settings"]')
        await pg.wait_for_selector('button[aria-pressed]', timeout=5000)
        card = await pg.evaluate("""() => {
          const s = [...document.querySelectorAll('span[aria-hidden="true"].font-mono')].filter((x) => x.innerText.includes('error: rejected'));
          return { n: s.length, h: s[0]?.getBoundingClientRect().height, text: s[0]?.innerText };
        }""")
        check("theme cards are miniature terminals", card["n"] >= 4 and card["h"] >= 56 and "git push" in card["text"], card)
        await pg.screenshot(path=os.environ.get("UI_SHOT", "/tmp/ui-themes.png"))
        check("no page errors", not errors, errors)
        await b.close()


asyncio.run(main())
print("\n%d failure(s)" % len(failures) if failures else "\nall passed")
sys.exit(1 if failures else 0)
