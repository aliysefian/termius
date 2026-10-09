"""The Topology page, in a real (headless) browser with the backend mocked.

    pnpm build
    python3 src/lib/__tests__/e2e/topology.py [chromium-executable]

Serves the built app (build/) with a small fleet (a bastion, three hosts behind it, a proxy, a tunnel rule, a database reached
through a host, one host with no links), opens the page from the command palette and checks what is drawn: only the saved links,
unlinked hosts only on request, search that keeps a match with its neighbours, and a details panel with the terminal action.
Exits with an error if any of that is wrong. Screenshots go to the folder in TOPOLOGY_SHOTS, if set.
"""
import asyncio, http.server, os, socketserver, sys, threading
from playwright.async_api import async_playwright
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
BUILD = os.path.join(ROOT, "build")
EXE = sys.argv[1] if len(sys.argv) > 1 else None
SHOTS = os.environ.get("TOPOLOGY_SHOTS")
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=BUILD, **k)
    def log_message(self, *a): pass
srv = socketserver.ThreadingTCPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
URL = f"http://127.0.0.1:{srv.server_address[1]}/"
DATA = """
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const host = (id, label, env, extra = {}) => rec(id, { label, hostname: label + '.example', port: 22, group: 'Prod', tags: ['web'], notes: '', environment: env, identity_id: 'i1', favorite: false, ...extra });
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return [host('b1','bastion-eu','production'), host('app1','app-01','production',{jump_host_id:'b1', proxy_id:'p1'}), host('app2','app-02','production',{jump_host_id:'b1'}), host('db1','db-01','staging',{jump_host_id:'b1'}), host('lone','scratch','development')];
  if (cmd === 'list_identities') return [rec('i1', { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' })];
  if (cmd === 'list_proxies') return [rec('p1', { name: 'Corporate SOCKS', spec: { kind: 'socks5', host: 'proxy.corp', port: 1080 } })];
  if (cmd === 'list_forwards') return [rec('f1', { label: 'Metrics', host_id: 'app1', auto_start: false, kind: 'local', bind_addr: '127.0.0.1', bind_port: 9090, dest_host: 'localhost', dest_port: 9090 })];
  if (cmd === 'list_db_connections') return [rec('d1', { name: 'orders', engine: 'postgres', host: 'localhost', port: 5432, username: 'u', database: 'orders', tls: 'verify_full', ssh_host_id: 'app2', group: '', environment: 'production' })];
  return undefined;
};
window.__errs = [];
window.addEventListener('error', (e) => window.__errs.push(String(e.message)));
"""
async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 800})
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)[:160]))
        await pg.add_init_script(path=ROOT + "/src/lib/__tests__/e2e/mock.js")
        await pg.add_init_script(DATA)
        await pg.add_init_script("window.__units = 1;")
        await pg.goto(URL); await pg.wait_for_timeout(2500)
        await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(300)
        await pg.keyboard.type("Go to Topology"); await pg.wait_for_timeout(200); await pg.keyboard.press("Enter"); await pg.wait_for_timeout(1200)
        failures = []
        def expect(what, got, want):
            print(f"{'ok  ' if got == want else 'FAIL'} {what}: {got}" + ("" if got == want else f" (wanted {want})"))
            if got != want: failures.append(what)
        expect("items drawn", await pg.locator("[data-node]").count(), 7)
        expect("links drawn", await pg.locator("svg path[marker-end]").count(), 6)
        expect("links listed as text", await pg.locator("aside ul li").count(), 6)
        if SHOTS: await pg.screenshot(path=os.path.join(SHOTS, "topology.png"))
        await pg.locator('[data-node]:has-text("app-01")').first.click(); await pg.wait_for_timeout(300)
        expect("details heading", await pg.locator("aside h2").first.inner_text(), "app-01")
        expect("terminal action", await pg.locator('aside button:has-text("Open a terminal")').count(), 1)
        await pg.locator('label:has-text("Show hosts with no links") input').check(); await pg.wait_for_timeout(300)
        expect("items with unlinked hosts shown", await pg.locator("[data-node]").count(), 8)
        await pg.fill('input[aria-label="Search"]', "orders"); await pg.wait_for_timeout(300)
        expect("search 'orders' keeps the database and its host", await pg.locator("[data-node]").count(), 2)
        expect("page errors", errs, [])
        if failures:
            print("FAILED:", failures); sys.exit(1)
        await b.close()
    srv.shutdown()
asyncio.run(main())
