"""The file panes in a real (headless) browser with the backend mocked: a long folder scrolls inside its pane, the
pane's footer (count and Upload/Download) stays on screen, a slow older listing never replaces a newer one, and a
folder that comes back empty says so.

    pnpm dev &
    python3 src/lib/__tests__/e2e/files.py [chromium-executable]
"""
import asyncio, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__lists = [];
const entry = (dir, i, isDir) => ({ name: (isDir ? 'dir' : 'file') + String(i).padStart(3, '0') + '.txt', path: dir.replace(/\\/$/, '') + '/' + (isDir ? 'dir' : 'file') + String(i).padStart(3, '0') + '.txt', is_dir: isDir, is_symlink: false, size: 1000 + i, modified: 1700000000, permissions: 0o644 });
window.__invoke = async (cmd, args) => {
  const host = { id: 'h1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ftp-box', hostname: 'ftp.example', port: 21, group: '', tags: [], notes: '', protocol: 'ftp', ftp: { anonymous: true, tls: 'none' } } };
  if (cmd === 'list_hosts') return [host];
  if (cmd === 'local_home') return '/home/me';
  if (cmd === 'files_caps') return { list: true, mkdir: true, rename: true, delete: true, preview: true, chmod: false, edit: false };
  if (cmd === 'ftp_open') return { home: '/pub', pinned_now: false };
  if (cmd === 'files_list') {
    window.__lists.push(args.path);
    if (args.sessionId === 'local') return Array.from({ length: 5 }, (_, i) => entry(args.path, i, false));
    if (args.path === '/pub') return [...Array.from({ length: 6 }, (_, i) => entry('/pub', i, true)), ...Array.from({ length: 80 }, (_, i) => entry('/pub', i, false))];
    if (args.path === '/slow') { await new Promise((r) => setTimeout(r, 1200)); return [entry('/slow', 1, false)]; }
    if (args.path === '/fast') return [entry('/fast', 2, false)];
    if (args.path === '/empty') return [];
    if (args.path === '/docs') return [{ name: 'README.md', path: '/docs/README.md', is_dir: false, is_symlink: false, size: 400, modified: 1700000000, permissions: 0o644 }, { name: 'a.txt', path: '/docs/a.txt', is_dir: false, is_symlink: false, size: 9, modified: 1700000000, permissions: 0o644 }];
    return [];
  }
  if (cmd === 'files_preview') {
    if (args.path.endsWith('README.md')) return { kind: 'text', base64: null, truncated: false, text: '# Title\\n\\nSome **bold** text and `code`.\\n\\n1. first\\n2. second\\n\\n> a quote\\n\\n| Name | Size |\\n|:--|--:|\\n| a | 1 |\\n\\n<script>window.__pwned = 1</script>\\n' };
    return { kind: 'text', base64: null, truncated: false, text: '# not markdown\\n' };
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
        pg = await b.new_page(viewport={"width": 1400, "height": 700})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK)
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)
        await pg.click('button[aria-label="Files"]')
        await pg.wait_for_timeout(500)
        box = pg.locator('input[aria-label="Remote host"]')
        await box.click()
        await box.fill("ftp-box")
        await pg.keyboard.press("Enter")
        await pg.wait_for_selector('[aria-label="remote file list"] tbody tr', timeout=8000)
        await pg.wait_for_timeout(300)

        m = await pg.evaluate("""() => {
          const list = document.querySelector('[aria-label="remote file list"]');
          const r = list.getBoundingClientRect();
          const dl = [...document.querySelectorAll('button')].find((x) => x.textContent.includes('Download'));
          const d = dl.getBoundingClientRect();
          return { top: r.top, bottom: r.bottom, client: list.clientHeight, scroll: list.scrollHeight, dlBottom: d.bottom, win: innerHeight };
        }""")
        check("the remote list is no taller than the window", m["bottom"] <= m["win"] + 1, m)
        check("the remote list has more rows than fit, and scrolls", m["scroll"] > m["client"] + 50, m)
        check("the Download button is on screen", m["dlBottom"] <= m["win"] + 1, m)
        await pg.mouse.move(900, 350)
        await pg.mouse.wheel(0, 600)
        await pg.wait_for_timeout(200)
        top = await pg.evaluate("document.querySelector('[aria-label=\"remote file list\"]').scrollTop")
        check("the wheel scrolls the remote list", top > 100, top)
        await pg.evaluate("() => { const l = document.querySelector('[aria-label=\"remote file list\"]'); l.scrollTop = l.scrollHeight; }")
        await pg.wait_for_timeout(150)
        last = await pg.evaluate("""() => { const l = document.querySelector('[aria-label="remote file list"]'); const rows = l.querySelectorAll('tbody tr'); const r = rows[rows.length - 1].getBoundingClientRect(); return r.bottom <= l.getBoundingClientRect().bottom + 1 && r.bottom > 0; }""")
        check("the last row can be brought into view", last)
        local = await pg.evaluate("""() => { const l = document.querySelector('[aria-label="local file list"]'); return l.getBoundingClientRect().bottom <= innerHeight + 1; }""")
        check("the local pane fits the window too", local)

        # A slow listing must not replace the newer one.
        async def goto(path):
            await pg.locator('nav[aria-label="Path"] button[aria-label="Type a path"]').nth(1).click()
            inp = pg.locator('input[aria-label="Path"]')
            await inp.fill(path)
            await pg.keyboard.press("Enter")
        await goto("/slow")
        await pg.wait_for_timeout(100)
        await goto("/fast")
        await pg.wait_for_timeout(1800)
        names = await pg.evaluate("[...document.querySelectorAll('[aria-label=\"remote file list\"] tbody tr')].map((r) => r.innerText)")
        check("the newest folder's listing wins over a slow older one", len(names) == 1 and "file002" in names[0], names)
        await goto("/empty")
        await pg.wait_for_timeout(400)
        txt = await pg.evaluate("document.querySelector('[aria-label=\"remote file list\"]').innerText")
        check("an empty folder says so", "Empty folder" in txt, txt)

        # Quick look renders Markdown, can show the source, and leaves other text alone.
        await goto("/docs")
        await pg.wait_for_selector('[aria-label="remote file list"] tbody tr')
        await pg.locator('[aria-label="remote file list"] tbody tr', has_text="README.md").click()
        await pg.keyboard.press("Space")
        await pg.wait_for_selector('[data-testid="markdown-preview"]', timeout=5000)
        html = await pg.inner_html('[data-testid="markdown-preview"]')
        check("a .md file is shown rendered", "<h1>Title</h1>" in html and "<strong>bold</strong>" in html and "<ol>" in html and "<table>" in html and "<blockquote>" in html, html)
        check("markup in the file is not run", await pg.evaluate("window.__pwned === undefined") and "<script" not in html, html)
        h1 = await pg.evaluate("getComputedStyle(document.querySelector('[data-testid=\"markdown-preview\"] h1')).fontSize")
        li = await pg.evaluate("getComputedStyle(document.querySelector('[data-testid=\"markdown-preview\"] ol')).listStyleType")
        check("it is styled (a big heading, numbered list)", float(h1[:-2]) > 20 and li == "decimal", (h1, li))
        await pg.screenshot(path=os.environ.get("MD_SHOT", "/tmp/md-preview.png"))
        await pg.get_by_role("button", name="Source").click()
        src = await pg.evaluate("document.querySelector('pre')?.innerText ?? ''")
        check("Source shows the raw text", src.startswith("# Title") and await pg.locator('[data-testid="markdown-preview"]').count() == 0, src[:60])
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(200)
        await pg.locator('[aria-label="remote file list"] tbody tr', has_text="a.txt").click()
        await pg.keyboard.press("Space")
        await pg.wait_for_selector('pre', timeout=5000)
        check("other text files have no Rendered/Source switch", await pg.get_by_role("button", name="Rendered").count() == 0)
        check("no page errors", not errors, errors)
        await b.close()


asyncio.run(main())
print("\n%d failure(s)" % len(failures) if failures else "\nall passed")
sys.exit(1 if failures else 0)
