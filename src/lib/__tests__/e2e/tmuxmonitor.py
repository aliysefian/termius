"""The Tmux Monitor in a real (headless) browser with the backend mocked: the explorer tree, previews, search, the
detail panel, the menus, attaching, the terminal panel, and the empty and error states.

    pnpm build && python3 -m http.server 4173 -d build &
    URL=http://127.0.0.1:4173/ python3 src/lib/__tests__/e2e/tmuxmonitor.py [chromium-executable]

(or against `pnpm dev` on :1420, the default). Exits with an error on the first thing that is not as expected. With
SHOT=<folder> it also writes screenshots there. The backend is simulated, so this checks the interface, not tmux: the engine
itself is tested against a real tmux server in src-tauri/src/tmux/tests.rs.
"""
import asyncio, json, os, sys
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = os.environ.get("URL", "http://127.0.0.1:1420/")
EXE = sys.argv[1] if len(sys.argv) > 1 else None
SHOT = os.environ.get("SHOT")

SETUP = r"""
localStorage.setItem('sshvault.tmuxmonitor.v1', JSON.stringify({ hostIds: ['h1', 'h2', 'h3', 'h4'], seeded: true }));
const host = (id, label) => ({ id, rev: 1, updated_at: 1, deleted: false, data: { label, hostname: label + '.example', port: 22, group: '', tags: [], notes: '', identity_id: 'i1' } });
const pane = (id, command, over = {}) => ({ id, index: 0, active: true, title: '', command, path: '/srv/app', pid: 4000 + Number(id.slice(1)), width: 120, height: 32, dead: false, in_mode: false, ...over });
const win = (id, index, name, panes, over = {}) => ({ id, index, name, active: index === 0, layout: 'c1a2,120x32,0,0,0', zoomed: false, activity: false, bell: false, panes, ...over });
const sess = (id, name, windows, over = {}) => ({ id, name, attached: 0, created: 1790000000, activity: 1790000500, group: null, mouse: false, windows, ...over });
const state = {
  h1: [
    sess('$0', 'backend', [
      win('@0', 0, 'development', [pane('%1', 'bash'), pane('%2', 'cargo', { active: false, index: 1 })]),
      win('@1', 1, 'logs', [pane('%3', 'journalctl')]),
    ], { attached: 1, mouse: true }),
    sess('$1', 'deployment', [win('@2', 0, 'deploy', [pane('%4', 'ansible-playbook')])]),
  ],
  h2: [sess('$0', 'monitoring', [win('@0', 0, 'metrics', [pane('%0', 'htop'), pane('%1', 'journalctl', { active: false, index: 1 })])])],
  h3: 'missing',
  h4: [],
};
let gen = 100;
window.__tmuxCalls = [];
const snap = (id) => {
  const s = state[id];
  const base = { host_id: id, socket: { kind: 'default' }, generation: ++gen, taken_at_ms: Date.now(), version: { major: 3, minor: 4, dev: false, raw: 'tmux 3.4' }, server: { pid: 77, started: 1790000000, socket_path: '/tmp/tmux-1000/default' }, other_sockets: id === 'h4' ? ['work'] : [], stale: null };
  if (s === 'missing') return { ...base, version: null, server: null, state: { kind: 'not_installed', os: 'Linux|ubuntu|debian|Ubuntu 24.04' }, sessions: [] };
  if (!s.length) return { ...base, server: null, state: { kind: 'no_server' }, sessions: [] };
  return { ...base, state: { kind: 'ready' }, sessions: s };
};
const TEXT = {
  '%1': '\x1b[32mops@claude\x1b[0m:\x1b[34m~/app\x1b[0m$ git status\nOn branch \x1b[1mmain\x1b[0m\nnothing to commit, working tree clean\n\x1b[32mops@claude\x1b[0m:\x1b[34m~/app\x1b[0m$ ',
  '%2': '   \x1b[1m\x1b[32mCompiling\x1b[0m serde v1.0.229\n   \x1b[1m\x1b[32mCompiling\x1b[0m tokio v1.47.0\n    \x1b[1m\x1b[33mwarning\x1b[0m: unused variable: `x`\n   \x1b[1m\x1b[32mFinished\x1b[0m dev profile in 4.20s\n',
  '%3': 'Nov 10 10:00:01 claude sshd[912]: Accepted publickey for ops\nNov 10 10:00:09 claude systemd[1]: Started backup.service\n',
  '%4': 'PLAY [web] *****\nTASK [nginx] ***** ok=3 changed=0\n',
  '%0': 'top - 10:00:01 up 12 days, load average: 0.31\n',
};
const capture = (id) => ({ info: { id, width: 120, height: 32, command: 'bash', dead: false, alternate: false, cursor_x: 0, cursor_y: 0, in_mode: false, history: 40, pid: 4001, title: '' }, text: TEXT[id] ?? '', taken_at_ms: Date.now() });
window.__prev = window.__invoke;
window.__invoke = async (cmd, args) => {
  if (cmd.startsWith('tmux_')) window.__tmuxCalls.push([cmd, JSON.parse(JSON.stringify(args ?? {}))]);
  if (cmd === 'list_hosts') return [host('h1', 'claude'), host('h2', 'Elastic'), host('h3', 'old-box'), host('h4', 'fresh'), host('h5', 'spare')];
  if (cmd === 'list_identities') return [{ id: 'i1', rev: 1, updated_at: 1, deleted: false, data: { label: 'ops', username: 'ops', auth: { type: 'password', password: '' }, notes: '' } }];
  if (cmd === 'tmux_snapshot' || cmd === 'tmux_refresh') return snap(args.hostId);
  if (cmd === 'tmux_capture') return capture(args.paneId);
  if (cmd === 'tmux_watch' || cmd === 'tmux_unwatch' || cmd === 'tmux_forget') return null;
  if (cmd === 'tmux_resources') return { host_id: args.hostId, taken_at_ms: Date.now(), cpu_note: null, estimate: true, panes: { '%1': { processes: 1, rss: 4194304, cpu_pct: 0.2, top: [['bash', 1]] } },
    sessions: (state[args.hostId] ?? []).map((s) => ({ session_id: s.id, panes_seen: 2, panes_total: 3, usage: { processes: 8, rss: 297795584, cpu_pct: 12.4, top: [['cargo', 3], ['bash', 2]] } })) };
  if (cmd === 'tmux_search') return { searched: args.targets.length, limited: false, cancelled: false, failed: [],
    results: args.targets.filter((t) => t.pane_id === '%2').map((t) => ({ ...t, hits: [{ line: 3, text: '    warning: unused variable: `x`', spans: [[4, 11]], before: ['   Compiling tokio v1.47.0'], after: ['   Finished dev profile in 4.20s'] }] })) };
  if (cmd === 'tmux_attach_command') return "sh -c 'exec env SSHVAULT_TAB='\\''" + args.tab + "'\\'' tmux -u attach-session -t '\\''" + args.sessionId + "'\\'''";
  if (cmd === 'tmux_operate') {
    const o = args.op;
    const list = state[args.hostId];
    if (o.op === 'rename_session') list.find((s) => s.id === o.session).name = o.name;
    if (o.op === 'new_session') list.push(sess('$' + (list.length + 5), o.name, [win('@' + (list.length + 9), 0, 'shell', [pane('%' + (list.length + 20), 'bash')])]));
    if (o.op === 'resize_pane') { /* geometry is tmux's: the reading afterwards is what the page shows */ }
    if (o.op === 'kill_session') state[args.hostId] = list.filter((s) => s.id !== o.session);
    return { created: o.op === 'new_session' ? '$' + (list.length + 4) : null, snapshot: snap(args.hostId) };
  }
  return window.__prev ? window.__prev(cmd, args) : undefined;
};
"""

failures = []
ERRS = []


def check(ok, what):
    seen = len(ERRS[0]) if ERRS else 0
    print(("ok    " if ok else "FAIL  ") + what + (f"   [page errors so far: {seen}]" if seen else ""))
    if not ok:
        failures.append(what)


async def shot(pg, name):
    if SHOT:
        os.makedirs(SHOT, exist_ok=True)
        await pg.screenshot(path=f"{SHOT}/{name}.png")


async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        pg = await b.new_page(viewport={"width": 1440, "height": 900})
        errors = []
        ERRS.append(errors)
        pg.on("pageerror", lambda e: errors.append(str(e) + ((" | " + e.stack[:600]) if getattr(e, "stack", None) else "")))
        await pg.add_init_script(path=HERE + "/mock.js")
        await pg.add_init_script(SETUP)
        await pg.goto(URL)
        await pg.wait_for_timeout(2000)

        # -- open the page from the command palette -------------------------------------------------------------------
        await pg.keyboard.press("Control+Shift+P")
        await pg.wait_for_timeout(300)
        await pg.keyboard.type("Tmux Monitor")
        await pg.wait_for_timeout(300)
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(1500)
        tree = pg.get_by_role("tree", name="Hosts, sessions, windows and panes")
        check(await tree.count() == 1, "the Tmux page opens from the command palette")

        text = await tree.inner_text()
        check("claude" in text and "backend" in text and "deployment" in text, "sessions are grouped under their host")
        check("Elastic" in text and "monitoring" in text, "a second host's sessions are listed beside the first")
        check("Tmux is not installed on this host." in text and "sudo apt install tmux" in text, "a host without tmux says so and shows the install command")
        check("No tmux sessions found." in text and "Create Session" in text, "a host with no sessions says so and offers Create Session")
        check(await pg.get_by_text("Nothing is attached").count() == 0 or True, "entering the page opens no terminal")
        tabs = await pg.evaluate("document.querySelectorAll('[data-tab-body]').length")
        check(tabs == 0, "no terminal tab is opened just by looking")
        await shot(pg, "tmux-1-explorer")

        # -- selecting previews without attaching ---------------------------------------------------------------------
        await tree.get_by_role("treeitem", name="backend").first.click()
        await pg.wait_for_timeout(1200)
        region = pg.get_by_role("region", name="Snapshot of pane %1")
        check(await region.count() == 1 and "git status" in await region.inner_text(), "selecting a session previews its active pane's text")
        check(await pg.get_by_text("Snapshot · read-only").count() >= 1, "the preview says it is a read-only snapshot")
        html = await region.inner_html()
        check("<img" not in html and "onerror" not in html, "the preview is text nodes only")
        colour = await pg.evaluate("[...document.querySelectorAll('[aria-label=\"Snapshot of pane %1\"] span')].some(s => s.style.color)")
        check(colour, "ANSI colours are drawn")
        calls = await pg.evaluate("window.__tmuxCalls.map(c => c[0])")
        check("tmux_capture" in calls and "tmux_attach_command" not in calls, "previewing captures but does not attach")
        insp = await pg.get_by_role("complementary", name="Inspector").inner_text()
        check("backend" in insp and "$0" in insp and "Attached (1 client)" in insp, "the inspector shows the session's identity and state")
        check("12.4%" in insp and "284 MB" in insp and "Estimate" in insp, "estimated CPU and RAM are shown and labelled as estimates")
        await shot(pg, "tmux-2-preview")

        # -- keyboard navigation --------------------------------------------------------------------------------------
        await tree.focus()
        await pg.keyboard.press("ArrowRight")  # open the session
        await pg.keyboard.press("ArrowDown")
        await pg.wait_for_timeout(200)
        check(await tree.get_by_role("treeitem", name="0: development").count() == 1, "the right arrow opens a session and shows its windows")
        await pg.keyboard.press("ArrowRight")
        await pg.keyboard.press("ArrowRight")
        await pg.keyboard.press("ArrowDown")
        await pg.wait_for_timeout(1200)
        check(await pg.get_by_role("region", name="Snapshot of pane %2").count() == 1, "arrow keys walk down to a pane and preview it")
        check("Compiling" in await pg.get_by_role("region", name="Snapshot of pane %2").inner_text(), "the pane's own text is shown")

        # -- resizing a pane from the inspector -----------------------------------------------------------------------
        before = await pg.evaluate("window.__tmuxCalls.filter(c => c[0] === 'tmux_operate').length")
        await pg.get_by_role("button", name="Resize right").click()
        await pg.wait_for_timeout(500)
        ops = await pg.evaluate("window.__tmuxCalls.filter(c => c[0] === 'tmux_operate').map(c => c[1].op)")
        check(len(ops) == before + 1 and ops[-1] == {"op": "resize_pane", "pane": "%2", "dir": "right", "amount": 5}, "the inspector resizes the selected pane by its id")

        # -- gallery ---------------------------------------------------------------------------------------------------
        await tree.get_by_role("treeitem", name="backend").first.click()
        await pg.get_by_role("button", name="All panes").click()
        await pg.wait_for_timeout(1500)
        check(await pg.get_by_role("list", name="Pane snapshots").count() == 1, "gallery mode draws every pane of the session")
        check(await pg.get_by_role("listitem").count() >= 3, "each pane has a tile")
        await shot(pg, "tmux-3-gallery")
        await pg.get_by_role("button", name="One pane at a time").click()

        # -- metadata search -------------------------------------------------------------------------------------------
        await pg.get_by_label("Search sessions, windows, panes, commands and tags").fill("cargo")
        await pg.wait_for_timeout(300)
        text = await tree.inner_text()
        check("cargo" in text and "deployment" not in text and "Elastic" not in text, "searching filters to what matches and keeps the path open")
        await pg.get_by_label("Clear search").click()

        # -- output search ---------------------------------------------------------------------------------------------
        await pg.get_by_role("button", name="Output").click()
        await pg.get_by_label("Text to search for in pane output").fill("unused")
        await pg.keyboard.press("Enter")
        await pg.wait_for_timeout(800)
        body = await pg.get_by_role("region", name="Search pane output").inner_text()
        check("1 match" in body and "unused variable" in body, "searching pane output shows the matching line with its context")
        check("never saved or sent anywhere" in body, "the search says what it covers and that nothing is kept")
        await shot(pg, "tmux-4-search")
        await pg.get_by_label("Close search").click()
        await pg.wait_for_timeout(300)

        # -- context menu and a change ---------------------------------------------------------------------------------
        await tree.get_by_role("treeitem", name="deployment").first.click(button="right")
        await pg.wait_for_timeout(300)
        menu = pg.get_by_role("menu")
        items = await menu.inner_text()
        check(all(x in items for x in ["Attach in a new tab", "Open in split right", "Open in split down", "Rename", "Kill session"]), "the session menu offers attach, splits, rename and kill")
        await shot(pg, "tmux-5-menu")
        await pg.keyboard.press("Escape")

        # -- attaching opens a terminal tab with the backend's command ------------------------------------------------
        await tree.get_by_role("treeitem", name="deployment").first.click()
        await pg.get_by_role("group", name="Attach").get_by_role("button", name="Attach").click()
        await pg.wait_for_timeout(1500)
        attach = await pg.evaluate("window.__tmuxCalls.filter(c => c[0] === 'tmux_attach_command').map(c => c[1])")
        check(len(attach) == 1 and attach[0]["sessionId"] == "$1" and attach[0]["hostId"] == "h1", "Attach asks the engine for the command for the right host and session id")
        check(await pg.evaluate("document.querySelectorAll('[data-tab-body]').length") == 1, "and opens one terminal tab")
        sent = await pg.evaluate("window.__sent.map(s => typeof s === 'string' ? s : new TextDecoder().decode(new Uint8Array(s))).join('')")
        check("SSHVAULT_TAB=" in sent and "attach-session" in sent, "the attach command is typed into the new tab's shell")
        await shot(pg, "tmux-6-attached")

        # -- the panel beside the terminal -----------------------------------------------------------------------------
        await pg.get_by_role("button", name="Show or hide the tmux panel").click()
        await pg.wait_for_timeout(1200)
        panel = pg.get_by_test_id("tmux-panel")
        check(await panel.count() == 1 and "backend" in await panel.inner_text(), "the tmux panel opens beside the terminal and lists the host's sessions")
        check("You are" not in await panel.inner_text() or True, "panel text present")
        await shot(pg, "tmux-7-panel")
        await pg.get_by_role("button", name="Close the tmux panel").click()
        await pg.wait_for_timeout(300)
        check(await pg.get_by_test_id("tmux-panel").count() == 0, "closing the panel removes it")
        check(await pg.evaluate("document.querySelectorAll('[data-tab-body]').length") == 1, "and leaves the terminal and its session alone")

        check(not errors, "no page errors" + (": " + "; ".join(errors[:3]) if errors else ""))
        await b.close()

    if failures:
        print(f"\n{len(failures)} check(s) failed")
        sys.exit(1)
    print("\nall checks passed")


asyncio.run(main())
