"""Network ergonomics in a real (headless) browser with the backend mocked: the hop chain bar and its inspector,
port detection in terminal output (the pill, the tunnel badge, stopping it), the agent-forwarding switch, the
signature notice, and the diagnostics drawer.

    pnpm dev &
    python3 src/lib/__tests__/e2e/netergo.py [chromium-executable]
"""
import asyncio, os, re, sys, time
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
AXE = open(os.path.join(ROOT, "node_modules", "axe-core", "axe.min.js")).read()
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
failures = []

MOCK = """
window.__calls = [];
window.__connects = 0;
window.__rtts = [{ total_ms: 24, segment_ms: 24 }, { total_ms: 36, segment_ms: 12 }];
window.__verdict = 'healthy';
window.__runs = 0;
const rec = (id, data) => ({ id, rev: 1, updated_at: 1, deleted: false, data });
const base = { port: 22, group: '', tags: [], notes: '' };
const hosts = [
  rec('bastion', { ...base, label: 'Elastic', hostname: '91.107.165.47', identity_id: 'idn' }),
  rec('app', { ...base, label: 'Spark', hostname: '10.0.0.7', port: 2223, identity_id: 'idn', jump_host_id: 'bastion', forward_agent: true }),
  rec('plain', { ...base, label: 'plain-box', hostname: 'plain.example', identity_id: 'idn' }),
];
const hops = [
  { label: '91.107.165.47', host: '91.107.165.47', port: 22, fingerprint: 'SHA256:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG', algorithm: 'ssh-ed25519', connect_ms: 61, connected_at: Math.floor(Date.now() / 1000) - 125 },
  { label: '10.0.0.7:2223', host: '10.0.0.7', port: 2223, fingerprint: 'SHA256:ZZZZefghijklmnopqrstuvwxyz0123456789ABCDEFG', algorithm: 'ssh-ed25519', connect_ms: 88, connected_at: Math.floor(Date.now() / 1000) - 124 },
];
let agent = { state: 'on' };
window.__invoke = async (cmd, args) => {
  window.__calls.push(cmd);
  if (cmd === 'list_hosts') return hosts;
  if (cmd === 'list_identities') return [rec('idn', { label: 'ops', username: 'alice', auth: { method: 'agent' } })];
  if (cmd === 'list_forwards') return [rec('rule1', { label: 'web UI', host_id: 'app', auto_start: false, kind: 'local', bind_addr: '127.0.0.1', bind_port: 8080, dest_host: 'localhost', dest_port: 80 })];
  if (cmd === 'ssh_connect') { window.__connects++; return undefined; }
  if (cmd === 'ssh_hops') return args.paneId && hops;
  if (cmd === 'ssh_hop_rtts') return window.__rtts;
  if (cmd === 'ssh_agent_forwarding') return args.paneId && window.__agentHost !== 'plain' ? agent : null;
  if (cmd === 'ssh_agent_forwarding_set') {
    agent = args.mode === 'off' ? { state: 'off' } : args.mode === 'on' ? { state: 'on' } : { state: 'timed', expires_in_secs: args.secs ?? 300 };
    window.__agentSet = args;
    return agent;
  }
  if (cmd === 'forward_start_adhoc') {
    window.__adhoc = args;
    const id = 'adhoc-' + args.kind.dest_port;
    setTimeout(() => window.__emit('forward:status', { rule_id: id, status: { state: 'active', port: args.kind.bind_port || 45123 } }), 50);
    return id;
  }
  if (cmd === 'forward_stop') { window.__stopped = args.id; return null; }
  if (cmd === 'plugin:opener|open_url') { window.__opened = args.url; return null; }
  if (cmd === 'diagnose_host') {
    window.__runs++;
    const ch = args.onEvent; let i = 0;
    const emit = (m) => window['_' + ch.id]({ index: i++, message: m });
    const bad = window.__verdict !== 'healthy';
    const steps = [
      [40, { stage: 'dns', report: { name: '10.0.0.7', literal: false, addresses: [{ ip: '203.0.113.5', record: 'A', ptr: 'static.5.113.0.203.example.net' }, { ip: '2001:db8::5', record: 'AAAA', ptr: null }], resolve_ms: 4, error: null } }],
      [80, { stage: 'tcp', report: { addr: '91.107.165.47:22', connect_ms: bad ? null : 24, failure: bad ? 'timeout' : null, error: bad ? 'no answer within 6 s' : null } }],
      [120, { stage: 'route_started' }],
    ];
    if (!bad) steps.push([160, { stage: 'ssh', report: { banner: 'SSH-2.0-OpenSSH_9.6p1', banner_ms: 11, kex_ms: 96, algorithm: 'ssh-ed25519', fingerprint: 'SHA256:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG', error: null } }]);
    steps.push([300, { stage: 'route', report: { tool: 'mtr', reached: !bad, error: null, hops: [
      { hop: 1, ip: '192.168.1.1', loss_pct: 0, avg_ms: 0.6, sent: 3 },
      { hop: 2, ip: null, loss_pct: 100, avg_ms: null, sent: 3 },
      { hop: 3, ip: '203.0.113.5', loss_pct: 33.3, avg_ms: 128.4, sent: 3 },
    ] } }]);
    if (!bad) steps.push([340, { stage: 'auth', report: { ok: true, total_ms: 210, failure: null, error: null } }]);
    const verdict = bad
      ? { kind: window.__verdict, title: 'Bastion unreachable', detail: 'The route answers up to hop 3 (198.51.100.7) and nothing after it.' }
      : { kind: 'healthy', title: 'Healthy', detail: 'The login works (210 ms from start to finish).' };
    steps.push([380, { stage: 'verdict', verdict }]);
    for (const [at, ev] of steps) setTimeout(() => emit(ev), at);
    await new Promise((r) => setTimeout(r, 420));
    return verdict;
  }
  return undefined;
};
"""


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


async def axe(pg, where, only=None):
    """Serious or critical accessibility findings on the page (or inside `only`), in the dark theme."""
    await pg.evaluate(AXE)
    res = await pg.evaluate(
        """async (only) => {
          const r = await axe.run(only ? document.querySelector(only) : document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] } });
          return r.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical').map((v) => v.id + ' x' + v.nodes.length + ' ' + v.nodes[0].target.join(' '));
        }""",
        only,
    )
    check(f"no serious accessibility findings: {where}", not res, res)


async def deliver(pg, text):
    await pg.evaluate("(t) => window.__deliver(Array.from(new TextEncoder().encode(t)))", text)


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1500, "height": 850})
        errors = []
        pg.on("pageerror", lambda e: errors.append(str(e)))
        await pg.add_init_script(path=os.path.join(HERE, "mock.js"))
        await pg.add_init_script(MOCK)
        await pg.goto(URL)
        await pg.evaluate("() => localStorage.clear()")
        await pg.goto(URL)
        await pg.wait_for_timeout(2500)

        # -- the hop chain bar -------------------------------------------------------------------------------------
        await pg.locator('[role="treeitem"]', has_text="Spark").dblclick(position={"x": 40, "y": 10})
        await pg.wait_for_selector('[data-testid="hop-chain"]', timeout=10000)
        await pg.wait_for_selector('[data-testid="hop-latency"]:has-text("24 ms")', timeout=8000)
        chain = await pg.locator('[data-testid="hop-chain"]').inner_text()
        check("the chain shows you, the bastion and the host", "You" in chain and "Elastic" in chain and "Spark" in chain, chain)
        lat = await pg.locator('[data-testid="hop-latency"]').all_inner_texts()
        check("each link carries its own latency", lat == ["24 ms", "12 ms"], lat)
        mono = await pg.locator('[data-testid="hop-chain"]').evaluate("(e) => [getComputedStyle(e).fontFamily, getComputedStyle(e.querySelector('[data-testid=hop-latency]')).fontVariantNumeric]")
        check("latencies are monospace with tabular figures", "mono" in mono[0].lower() or "JetBrains" in mono[0], mono)
        check("tabular-nums on the latencies", "tabular-nums" in mono[1], mono)
        dots = await pg.locator('[data-testid="hop-chain"] [data-health]').evaluate_all("(els) => els.map((e) => e.getAttribute('data-health'))")
        check("both hops are green while they answer", dots == ["up", "up"], dots)

        await pg.locator('[data-testid="hop-node"]').first.click()
        await pg.wait_for_selector('[data-testid="hop-inspector"]')
        insp = await pg.locator('[data-testid="hop-inspector"]').inner_text()
        check("the inspector shows address, uptime, host key and fingerprint", all(x in insp for x in ["91.107.165.47:22", "ssh-ed25519", "SHA256:abcdefgh…BCDEFG"]) and ("2m" in insp), insp)
        w = await pg.locator('[data-testid="hop-inspector"]').evaluate("(e) => e.getBoundingClientRect().width")
        check("it is a compact w-72 popover", abs(w - 288) < 2, w)
        await axe(pg, "the hop inspector", '[data-testid="hop-inspector"]')
        check("with both actions", await pg.get_by_role("button", name="Reconnect hop").count() == 1 and await pg.get_by_role("button", name="Open direct shell").count() == 1)
        before = await pg.evaluate("window.__connects")
        await pg.get_by_role("button", name="Reconnect hop").click()
        await pg.wait_for_timeout(1500)
        check("Reconnect hop reconnects the session", await pg.evaluate("window.__connects") > before)
        await pg.wait_for_selector('[data-testid="hop-chain"]')
        await pg.keyboard.press("Escape")

        # A hop that stops answering turns amber, then red.
        await pg.evaluate("window.__rtts = [{ total_ms: 24, segment_ms: 24 }, { total_ms: null, segment_ms: null }]")
        await pg.wait_for_selector('[data-testid="hop-chain"] [data-health="slow"]', timeout=9000)
        check("one missed probe is amber", True)
        await pg.wait_for_selector('[data-testid="hop-chain"] [data-health="down"]', timeout=9000)
        check("repeated misses are red", True)
        dash = await pg.locator('[data-testid="hop-latency"]').all_inner_texts()
        check("a link with no answer shows a dash", dash[1] == "–", dash)
        await pg.evaluate("window.__rtts = [{ total_ms: 24, segment_ms: 24 }, { total_ms: 36, segment_ms: 12 }]")

        # -- port detection ----------------------------------------------------------------------------------------
        # The data channel belongs to the most recently connected pane: reconnect the first tab's session was the last.
        await deliver(pg, "Starting development server at http://0.0.0.0:8000/\r\n")
        await pg.wait_for_selector('[data-testid="port-offer"]', timeout=3000)
        offer = " ".join((await pg.locator('[data-testid="port-offer"]').inner_text()).split())
        check("a server announcing a port is offered for forwarding", "Port 8000 detected on remote" in offer and "Forward to :8000" in offer, offer)
        box = await pg.locator('[data-testid="port-offer"]').bounding_box()
        pane = await pg.locator('.xterm').first.bounding_box()
        check("the pill sits in the terminal's bottom-right corner", box["x"] + box["width"] > pane["x"] + pane["width"] - 40 and box["y"] + box["height"] > pane["y"] + pane["height"] - 40, (box, pane))
        await pg.screenshot(path=os.environ.get("NET_SHOT", "/tmp/net-offer.png"))
        await axe(pg, "the port offer", '[data-testid="port-offer"]')
        await pg.get_by_role("button", name="Forward to :8000").click()
        await pg.wait_for_selector('[data-testid="pane-forward"][data-state="active"]', timeout=4000)
        badge = await pg.locator('[data-testid="pane-forward"]').inner_text()
        check("the tunnel becomes a badge in the header", "localhost:8000 ➔ remote:8000" in badge, badge)
        check("and the pill is gone", await pg.locator('[data-testid="port-offer"]').count() == 0)
        adhoc = await pg.evaluate("window.__adhoc")
        check("the forward goes through the host, local to the same port", adhoc["hostId"] == "app" and adhoc["kind"]["dest_port"] == 8000 and adhoc["kind"]["bind_port"] == 8000 and adhoc["kind"]["kind"] == "local", adhoc)
        await pg.get_by_role("button", name="Open in browser").click()
        await pg.wait_for_timeout(200)
        check("Open in browser opens the local address", await pg.evaluate("window.__opened") == "http://localhost:8000", await pg.evaluate("window.__opened"))
        await deliver(pg, "Starting development server at http://0.0.0.0:8000/\r\n")
        await pg.wait_for_timeout(600)
        check("a port that is already forwarded is not offered again", await pg.locator('[data-testid="port-offer"]').count() == 0)
        await pg.get_by_role("button", name="Stop tunnel").click()
        await pg.wait_for_timeout(300)
        check("Stop tunnel stops it and removes the badge", await pg.evaluate("window.__stopped") == "adhoc-8000" and await pg.locator('[data-testid="pane-forward"]').count() == 0)

        # Dismissing is for good in this pane; a flood of output costs nothing and still finds the port at its end.
        await deliver(pg, "Listening on :9000\r\n")
        await pg.wait_for_selector('[data-testid="port-offer"]', timeout=3000)
        await pg.get_by_role("button", name="Dismiss").click()
        await deliver(pg, "Listening on :9000\r\n")
        await pg.wait_for_timeout(700)
        check("a dismissed port is not offered again", await pg.locator('[data-testid="port-offer"]').count() == 0)
        t = await pg.evaluate("""async () => {
          const line = new TextEncoder().encode('2026-10-18 10:00:00 INFO request handled in 3ms path=/api/items?id=42&page=7\\r\\n');
          const chunk = new Uint8Array(line.length * 200);
          for (let i = 0; i < 200; i++) chunk.set(line, i * line.length);
          const arr = Array.from(chunk);
          const t0 = performance.now();
          for (let i = 0; i < 60; i++) window.__deliver(arr);
          window.__deliver(Array.from(new TextEncoder().encode('Server listening on port 4321\\r\\n')));
          return performance.now() - t0;
        }""")
        await pg.wait_for_selector('[data-testid="port-offer"]:has-text("4321")', timeout=5000)
        check("a flood of log output is taken in without the detector slowing it", t < 3000, f"{t:.0f} ms for ~{60*200} lines")
        await pg.get_by_role("button", name="Dismiss").click()

        # -- agent forwarding --------------------------------------------------------------------------------------
        toggle = pg.locator('[data-testid="agent-toggle"]')
        check("a host that forwards the agent has the switch, showing it is on", (await toggle.inner_text()).strip() == "Agent: ON", await toggle.inner_text())
        await toggle.click()
        await axe(pg, "the agent menu", '[data-testid="agent-menu"]')
        await pg.get_by_role("menuitemradio", name=re.compile(r"^Off")).click()
        await pg.wait_for_timeout(300)
        check("switching it off says so", (await toggle.inner_text()).strip() == "Agent: OFF" and await toggle.get_attribute("data-state") == "off")
        await toggle.click()
        await pg.get_by_role("menuitemradio", name="On for 5 minutes").click()
        await pg.wait_for_timeout(1500)
        txt = (await toggle.inner_text()).strip()
        check("a timed grant counts down", txt.startswith("Agent: ON 4:5") or txt == "Agent: ON 5:00", txt)
        check("and asked for five minutes", (await pg.evaluate("window.__agentSet")) == {"paneId": (await pg.evaluate("window.__agentSet"))["paneId"], "mode": "timed", "secs": 300})

        # -- signature notice --------------------------------------------------------------------------------------
        sig = {"pane_id": "x", "host": "Elastic", "origin": "91.107.165.47", "fingerprint": "SHA256:abcdefghijklmnopqrstuvwxyz", "key_name": "id_ed25519", "at": int(time.time()), "refused": False}
        await pg.evaluate("(s) => window.__emit('agent:signature', s)", sig)
        await pg.wait_for_selector('text=requested an SSH key signature', timeout=3000)
        toast = pg.locator('[role="status"]', has_text="requested an SSH key signature")
        ttxt = await toast.inner_text()
        check("a signature request is announced with host, key and time", "Elastic" in ttxt and "id_ed25519" in ttxt and ":" in ttxt.split("·")[-1], ttxt)
        cls = await toast.get_attribute("class")
        check("in the amber warning style", "border-warning" in cls and "text-warning" in cls, cls)
        await pg.evaluate("(s) => window.__emit('agent:signature', s)", sig)
        await pg.wait_for_selector('text=×2', timeout=3000)
        check("repeats within a moment are one notice with a count", await pg.locator('[role="status"]', has_text="requested an SSH key signature").count() == 1)

        # -- Open direct shell (last: it connects another pane, and the mock's data channel follows the newest one) --
        await pg.locator('[data-testid="hop-node"]').first.click()
        await pg.get_by_role("button", name="Open direct shell").click()
        await pg.wait_for_timeout(800)
        tabs = await pg.locator('[role="tab"]').count()
        check("Open direct shell opens a tab for the bastion", tabs == 2, tabs)
        await pg.locator('[role="tab"]').first.click()
        await pg.wait_for_timeout(500)

        # -- the other host has no switch --------------------------------------------------------------------------
        await pg.evaluate("window.__agentHost = 'plain'")

        # -- diagnostics -------------------------------------------------------------------------------------------
        row = pg.locator('[role="treeitem"]', has_text="Spark").first
        await row.hover()
        await row.get_by_role("button", name="Diagnose connection").click()
        await pg.wait_for_selector('[data-testid="diagnose-drawer"]')
        w = await pg.locator('[data-testid="diagnose-drawer"]').evaluate("(e) => e.getBoundingClientRect().width")
        check("the drawer is 24rem wide", abs(w - 384) < 2, w)
        check("it says it is running", "Running checks" in await pg.locator('[data-testid="diagnose-drawer"]').inner_text())
        await pg.wait_for_selector('[data-testid="diagnose-verdict"]', timeout=5000)
        v = await pg.locator('[data-testid="diagnose-verdict"]').inner_text()
        check("a healthy host gets a healthy verdict", "Healthy" in v and await pg.locator('[data-testid="diagnose-verdict"]').get_attribute("data-kind") == "healthy", v)
        hs = await pg.locator('[data-testid="diagnose-handshake"]').inner_text()
        check("the handshake card compares TCP and key exchange", "24 ms" in hs and "96 ms" in hs and "OpenSSH_9.6p1" in hs, hs)
        route = await pg.locator('[data-testid="diagnose-route"] tbody tr').all_inner_texts()
        check("the route table has a row per hop", len(route) == 3 and "192.168.1.1" in route[0] and "* * *" in route[1] and "33%" in route[2] and "128" in route[2], route)
        rs = await pg.locator('[data-testid="diagnose-route"] table').evaluate("(t) => [getComputedStyle(t).fontFamily, getComputedStyle(t).fontVariantNumeric]")
        check("monospace and tabular figures in the route", ("mono" in rs[0].lower() or "JetBrains" in rs[0]) and "tabular-nums" in rs[1], rs)
        dns = await pg.locator('[data-testid="diagnose-dns"]').inner_text()
        check("DNS shows A, AAAA and the reverse name", "A" in dns and "AAAA" in dns and "203.0.113.5" in dns and "static.5.113" in dns, dns)
        check("the login card reports the sign-in", "210 ms" in await pg.locator('[data-testid="diagnose-login"]').inner_text())
        await pg.screenshot(path=os.environ.get("NET_SHOT2", "/tmp/net-diagnose.png"))
        await axe(pg, "the diagnostics drawer", '[data-testid="diagnose-drawer"]')

        await pg.evaluate("window.__verdict = 'bastion_unreachable'")
        await pg.get_by_role("button", name="Run again").click()
        await pg.wait_for_selector('[data-testid="diagnose-verdict"][data-kind="bastion_unreachable"]', timeout=5000)
        v = await pg.locator('[data-testid="diagnose-verdict"]').inner_text()
        check("a failure is classified and named", "Bastion Unreachable" in v and "hop 3" in v, v)
        check("the failure is not shown as healthy", "Healthy" not in v)
        check("running it again started a second run", await pg.evaluate("window.__runs") == 2)
        await pg.keyboard.press("Escape")
        await pg.wait_for_timeout(300)
        check("Escape closes the drawer", await pg.locator('[data-testid="diagnose-drawer"]').count() == 0)

        # -- the Tunnels view --------------------------------------------------------------------------------------
        await pg.click('button[aria-label="Tunnels"]')
        await pg.wait_for_selector('text=web UI', timeout=5000)
        tun = await pg.locator('[data-testid="hop-chain"]').last.inner_text()
        check("a rule whose host is behind a bastion shows its route", "Elastic" in tun and "Spark" in tun, tun)
        check("no page errors", not errors, errors)
        await b.close()


asyncio.run(main())
print("\n%d failure(s)" % len(failures) if failures else "\nall passed")
sys.exit(1 if failures else 0)
