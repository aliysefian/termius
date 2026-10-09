"""The content security policy, in a real (headless) browser with the backend mocked.

    pnpm build
    python3 src/lib/__tests__/e2e/csp.py [chromium-executable]

Serves the built app (build/) with the policy from src-tauri/tauri.conf.json as a response header, adds the sha256 of
the page's inline start-up script to script-src the way Tauri does when it bundles, drives the main pages and dialogs
as a11y.py does, and fails on any policy violation or error in the console. It proves that the app works under the
policy; it does not prove the policy keeps an attacker out.
"""
import asyncio, base64, hashlib, http.server, json, os, re, socketserver, sys, threading
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
BUILD = os.path.join(ROOT, "build")
EXE = sys.argv[1] if len(sys.argv) > 1 else None

policy = json.load(open(os.path.join(ROOT, "src-tauri", "tauri.conf.json")))["app"]["security"]["csp"]
if not policy:
    sys.exit("tauri.conf.json has no csp")
html = open(os.path.join(BUILD, "index.html"), encoding="utf-8").read()
hashes = [
    "'sha256-" + base64.b64encode(hashlib.sha256(m.encode()).digest()).decode() + "'"
    for m in re.findall(r"<script>(.*?)</script>", html, re.S)
]
header = policy.replace("script-src 'self'", "script-src 'self' " + " ".join(hashes), 1)


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=BUILD, **k)

    def end_headers(self):
        # CSP_OFF=1 serves no policy, to tell an error the policy causes from one the mock or the app has anyway.
        if not os.environ.get("CSP_OFF"):
            self.send_header("Content-Security-Policy", header)
        super().end_headers()

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True
server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
URL = f"http://127.0.0.1:{server.server_address[1]}/"

HOSTS = """
const mk = (id, label, env, group) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group, tags: ['db'], notes: '', environment: env, identity_id: 'i1', favorite: id === 'h1' } });
window.__invoke = async (cmd) => {
  if (cmd === 'list_hosts') return [mk('h1', 'web-01', 'production', 'Production/Web'), mk('h2', 'db-01', 'staging', 'Staging'), mk('h3', 'dev-box', 'development', '')];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  return undefined;
};
window.__csp = [];
document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' blocked ' + (e.blockedURI || 'inline') + ' at ' + e.sourceFile + ':' + e.lineNumber));
"""


async def main():
    problems = []
    notes = []  # errors that are not about the policy (the mock backend leaves some commands unanswered)
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1400, "height": 900})
        def sort(kind, text):
            (problems if re.search(r"Content Security Policy|violates the following|Refused to", text) else notes).append(f"{kind}: {text[:200]}")

        pg.on("console", lambda m: m.type == "error" and sort("console", m.text))
        pg.on("pageerror", lambda e: sort("page error", str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(HOSTS)
        await pg.add_init_script("window.__units = 1;")
        resp = await pg.goto(URL)
        seen = resp.headers.get("content-security-policy", "") if resp else ""
        if not os.environ.get("CSP_OFF") and "script-src 'self' 'sha256-" not in seen:
            problems.append("the policy header was not applied: " + seen[:120])
        await pg.wait_for_timeout(2500)
        if not await pg.locator('button[aria-label="Manage"]').count():
            problems.append("the app did not start under the policy (no rail)")
        else:
            await pg.click('button[aria-label="Manage"]'); await pg.wait_for_timeout(300)
            await pg.keyboard.press("Escape")
            await pg.keyboard.press("Control+Shift+P"); await pg.wait_for_timeout(300)
            await pg.keyboard.press("Escape")
            await pg.click('button[title="New host"]'); await pg.wait_for_timeout(500)
            for tab in ["Route", "Organise", "Automation"]:
                await pg.click(f'[role=tab]:has-text("{tab}")'); await pg.wait_for_timeout(200)
            await pg.keyboard.press("Escape"); await pg.wait_for_timeout(300)
            if await pg.locator('button:has-text("Discard")').count():
                await pg.click('button:has-text("Discard")'); await pg.wait_for_timeout(200)
            for label in ["Snippets", "Tunnels", "Databases", "Containers", "Ops", "Runbooks", "Settings"]:
                await pg.click(f'button[aria-label="{label}"]'); await pg.wait_for_timeout(500)
                if label == "Runbooks":
                    for tab in ["History", "Schedules"]:
                        await pg.click(f'[role=tab]:has-text("{tab}")'); await pg.wait_for_timeout(300)
                    await pg.click('[data-testid=schedule-new]'); await pg.wait_for_timeout(300)
            await pg.click('button[aria-label="Hosts"]')
            await pg.keyboard.press("Control+Shift+Backquote"); await pg.wait_for_timeout(1500)
            await pg.keyboard.type("echo hello"); await pg.keyboard.press("Enter"); await pg.wait_for_timeout(500)
            if not await pg.locator(".xterm").count():
                problems.append("the terminal did not render under the policy")
        problems += ["policy violation: " + v for v in await pg.evaluate("window.__csp || []")]
        await b.close()
    server.shutdown()
    print("policy:", header[:300])
    if notes:
        print("not about the policy (also present with CSP_OFF=1):\n  " + "\n  ".join(sorted(set(notes))))
    if problems:
        print("\n".join(sorted(set(problems))))
        sys.exit(1)
    print("no policy violations: the app runs under the policy")


asyncio.run(main())
