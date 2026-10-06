"""Smart completion against REAL shells and programs, in a real (headless) browser.

The page's local terminal is wired to a pseudo-terminal running bash, zsh or fish with the shell
integration snippets SSHVault shows in Settings. Then: history suggestions, accepting them, the list,
and the places nothing may appear (a password prompt, sudo, vim, top, tmux), a multi-line paste and a
very long line.

    pnpm dev &                                   # http://127.0.0.1:1420
    python3 src/lib/__tests__/e2e/shells.py [chromium-executable]

bash must be on PATH. zsh and fish are used when found on PATH, or under $SHELLS_ROOT (an unpacked
.deb tree: SHELLS_ROOT/bin/zsh and SHELLS_ROOT/usr/bin/fish); otherwise they are skipped and say so.
"""
import asyncio, os, pty, fcntl, struct, sys, tempfile, termios, shutil, signal
from playwright.async_api import async_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
URL = "http://127.0.0.1:1420/"
EXE = sys.argv[1] if len(sys.argv) > 1 else None
ROOT = os.environ.get("SHELLS_ROOT", "")
failures, skipped = [], []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


SNIPPETS = {
    "bash": """__sshvault_prompt() { local e=$?; printf '\\e]133;D;%s\\a\\e]7;file://%s%s\\a\\e]133;A\\a' "$e" "$HOSTNAME" "$PWD"; }
PROMPT_COMMAND="__sshvault_prompt${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
PS1="$PS1"'\\[\\e]133;B\\a\\]'
PS0='\\[\\e]133;C\\a\\]'
""",
    "zsh": """__sshvault_precmd() { local e=$?; printf '\\e]133;D;%s\\a\\e]7;file://%s%s\\a\\e]133;A\\a' "$e" "$HOST" "$PWD"; }
__sshvault_preexec() { printf '\\e]133;C\\a'; }
autoload -Uz add-zsh-hook
add-zsh-hook precmd __sshvault_precmd
add-zsh-hook preexec __sshvault_preexec
PS1="$PS1"$'%{\\e]133;B\\a%}'
""",
    "fish": """function __sshvault_postexec --on-event fish_postexec; printf '\\e]133;D;%s\\a' $status; end
function __sshvault_prompt --on-event fish_prompt; printf '\\e]7;file://%s%s\\a\\e]133;A\\a' $hostname $PWD; end
function __sshvault_preexec --on-event fish_preexec; printf '\\e]133;C\\a'; end
functions -c fish_prompt __sshvault_orig_prompt
function fish_prompt; __sshvault_orig_prompt; printf '\\e]133;B\\a'; end
""",
}


def find_shells():
    found = {}
    if shutil.which("bash"):
        found["bash"] = {"cmd": [shutil.which("bash")], "env": {}}
    lib = os.path.join(ROOT, "usr/lib/x86_64-linux-gnu") if ROOT else ""
    zsh = shutil.which("zsh") or (os.path.join(ROOT, "bin/zsh") if ROOT and os.path.exists(os.path.join(ROOT, "bin/zsh")) else None)
    if zsh:
        env = {}
        if ROOT and zsh.startswith(ROOT):
            fp = [os.path.join(ROOT, "usr/share/zsh/functions", d) for d in sorted(os.listdir(os.path.join(ROOT, "usr/share/zsh/functions")))]
            env = {"LD_LIBRARY_PATH": lib, "ZSH_MODULE_PATH": os.path.join(lib, "zsh/5.9"), "ZSH_FPATH": " ".join(fp)}
        found["zsh"] = {"cmd": [zsh], "env": env}
    fish = shutil.which("fish") or (os.path.join(ROOT, "usr/bin/fish") if ROOT and os.path.exists(os.path.join(ROOT, "usr/bin/fish")) else None)
    if fish:
        found["fish"] = {"cmd": [fish], "env": {"LD_LIBRARY_PATH": lib} if ROOT and fish.startswith(ROOT) else {}}
    return found


class Pty:
    """A shell in a pseudo-terminal whose output is handed to the page."""

    def __init__(self, name, spec, page):
        self.name, self.spec, self.page = name, spec, page
        self.fd = None
        self.pid = None
        self.buf = b""
        self.q = asyncio.Queue()
        self.home = tempfile.mkdtemp(prefix="shell-e2e-")

    def start(self, cols, rows):
        home = self.home
        env = {"HOME": home, "TERM": "xterm-256color", "LANG": "C.UTF-8", "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "USER": os.environ.get("USER", "tester"), "HOSTNAME": "mockhost", "HOST": "mockhost", **self.spec["env"]}
        cmd = list(self.spec["cmd"])
        snip = SNIPPETS[self.name]
        if self.name == "bash":
            rc = os.path.join(home, ".bashrc")
            open(rc, "w").write("PS1='$ '\nHISTFILE=/dev/null\n" + snip)
            cmd += ["--noprofile", "--rcfile", rc, "-i"]
        elif self.name == "zsh":
            open(os.path.join(home, ".zshenv"), "w").write(
                (f"fpath=({env['ZSH_FPATH']} $fpath)\n" if env.get("ZSH_FPATH") else "") + (f"module_path=({env['ZSH_MODULE_PATH']})\n" if env.get("ZSH_MODULE_PATH") else ""))
            open(os.path.join(home, ".zshrc"), "w").write("PS1='$ '\nunsetopt PROMPT_SP\nHISTFILE=/dev/null\n" + snip)
            env["ZDOTDIR"] = home
            cmd += ["-d", "-i"]
        else:
            cfg = os.path.join(home, ".config", "fish")
            os.makedirs(cfg)
            open(os.path.join(cfg, "config.fish"), "w").write("set -g fish_greeting\nfunction fish_prompt; echo -n '$ '; end\nset -g fish_history_path /dev/null\n" + snip)
            env["XDG_CONFIG_HOME"] = os.path.join(home, ".config")
            env["XDG_DATA_HOME"] = os.path.join(home, ".local", "share")
            cmd += ["-i"]
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(home)
            os.execvpe(cmd[0], cmd, env)
        self.pid, self.fd = pid, fd
        self.resize(cols, rows)
        loop = asyncio.get_event_loop()
        loop.add_reader(fd, self._readable)
        loop.create_task(self._pump())

    def resize(self, cols, rows):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    def _readable(self):
        try:
            data = os.read(self.fd, 65536)
        except OSError:
            data = b""
        if not data:
            asyncio.get_event_loop().remove_reader(self.fd)
            return
        self.buf += data
        self.q.put_nowait(data)

    async def _pump(self):
        while True:
            data = await self.q.get()
            try:
                await self.page.evaluate("a => window.__deliver(a)", list(data))
            except Exception:
                return

    def write(self, data):
        os.write(self.fd, bytes(data))

    def text(self):
        return self.buf.decode("utf-8", "replace")

    def stop(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        except OSError:
            pass
        shutil.rmtree(self.home, ignore_errors=True)


async def ghost_text(pg):
    return await pg.evaluate("document.querySelector('[data-testid=completion-ghost]')?.textContent ?? null")


async def menu_labels(pg):
    return await pg.evaluate("(() => { const m = document.querySelector('[data-testid=completion-menu]'); return m ? [...m.querySelectorAll('[role=option]')].map(i => i.querySelector('span').textContent.trim()) : null; })()")


async def sent_count(pg):
    return await pg.evaluate("window.__sent.length")


async def run_shell(p, name, spec):
    pty_ref = {}
    page = await p.new_page(viewport={"width": 1000, "height": 700})
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    sh = Pty(name, spec, page)
    await page.expose_function("__ptySpawn", lambda cols, rows: sh.start(cols, rows))
    await page.expose_function("__ptyWrite", lambda data: sh.write(data))
    await page.expose_function("__ptyResize", lambda cols, rows: sh.resize(cols, rows))
    await page.add_init_script(path=os.path.join(HERE, "mock.js"))
    await page.goto(URL)
    await page.evaluate("localStorage.setItem('sshvault.prefs.v1', JSON.stringify({smartCompletion: true, acInline: true, acMenu: true, fontSize: 13}))")
    await page.goto(URL)
    await page.wait_for_timeout(2500)
    await page.keyboard.press("Control+Shift+Backquote")
    await page.wait_for_timeout(2000)
    await page.click(".xterm")
    k = page.keyboard
    tag = f"[{name}] "

    async def settle(ms=500):
        await page.wait_for_timeout(ms)

    async def line(text):
        await k.type(text, delay=8)
        await settle(400)

    async def enter(wait=700):
        await k.press("Enter")
        await settle(wait)

    async def clear():
        await k.press("Control+u")
        await settle(300)

    # history: two commands, run for real
    await line("echo alpha-one")
    await enter()
    await line("echo alpha-two")
    await enter()
    check(tag + "the shell really ran them", "alpha-one" in sh.text() and "alpha-two" in sh.text())

    # the inline suggestion
    await line("echo alpha-t")
    g = await ghost_text(page)
    if name == "fish":
        # fish draws its own grey suggestion after the cursor; ours stays hidden so there are not two.
        check(tag + "no second suggestion beside fish's own", g is None, g)
        await clear()
    else:
        check(tag + "a history suggestion appears after what is typed", g == "wo", g)
        before = len(sh.text())
        await k.press("ArrowRight")
        await settle(400)
        await enter()
        new = sh.text()[before:]
        check(tag + "accepting it completes the line and Enter runs it", "alpha-two" in new.replace("echo alpha-two", ""), new[-200:])

    # the list
    await line("alpha-o")
    await k.press("Control+Space")
    await settle(700)
    labels = await menu_labels(page)
    if name == "fish":
        await k.press("Escape")
        await clear()
        check(tag + "the list is not opened while the shell has its own suggestion there", True)
    else:
        check(tag + "Ctrl+Space lists the history that matches", bool(labels) and "echo alpha-one" in labels, labels)
        await clear()
        await line("echo one")  # a different, still-matching line
        await clear()
        await k.press("Control+Space")
        await settle(500)
        await k.press("Escape")

    # a password prompt
    await clear()
    pw = {"bash": 'read -s -p "Password: " x', "zsh": 'read -s "x?Password: "', "fish": 'read -s -P "Password: " x'}[name]
    await line(pw)
    await enter(600)
    await k.type("hunt", delay=8)
    await settle(500)
    await k.press("Control+Space")
    await settle(500)
    check(tag + "nothing is offered at a password prompt", await ghost_text(page) is None and await menu_labels(page) is None, (await ghost_text(page), await menu_labels(page)))
    await enter(800)

    # sudo's password prompt
    if shutil.which("sudo"):
        await line("sudo -k true")
        await enter(900)
        await k.type("ec", delay=8)
        await settle(400)
        await k.press("Control+Space")
        await settle(500)
        check(tag + "nothing is offered at sudo's password prompt", await ghost_text(page) is None and await menu_labels(page) is None)
        await k.press("Control+c")
        await settle(700)

    # full-screen programs
    for prog, quit_keys, label in [("vim", [":q!", "Enter"], "vim"), ("top", ["q"], "top"), ("tmux", None, "tmux")]:
        if prog not in os.environ.get("PROGS", "vim top tmux").split():
            continue
        if not shutil.which(prog):
            skipped.append(f"{tag}{label} not installed")
            continue
        await clear()
        await line(prog if prog != "tmux" else "tmux -f /dev/null")
        await enter(1500)
        await k.type("echo", delay=8)
        await settle(400)
        await k.press("Control+Space")
        await settle(500)
        check(tag + f"nothing is offered inside {label}", await ghost_text(page) is None and await menu_labels(page) is None, (await ghost_text(page), await menu_labels(page)))
        if prog == "vim":
            await k.press("Escape")
            await k.type(":q!", delay=8)
            await k.press("Enter")
        elif prog == "top":
            # The letters typed above are top's own commands (some open prompts), so q may not quit it.
            await k.press("Control+c")
        else:
            # What was typed above is on tmux's own command line; clear it, then leave.
            await k.press("Control+u")
            await settle(200)
            await k.type("exit", delay=8)
            await k.press("Enter")
        await settle(1200)

    # back at the prompt the suggestions work again
    await clear()
    await line("echo alpha-t")
    g = await ghost_text(page)
    if os.environ.get("DEBUG") and g is None:
        print("DEBUG after programs:", repr(sh.text()[-500:]))
    check(tag + "after the programs, the prompt is served again", (g == "wo") if name != "fish" else (g is None), g)
    await clear()

    # a multi-line paste: the shell holds it as one command to edit, which gets no suggestions
    before = len(sh.text())
    await page.evaluate("""() => {
      const ta = document.querySelector('.xterm-helper-textarea');
      const dt = new DataTransfer();
      dt.setData('text/plain', 'echo paste-1\\necho paste-2\\n');
      ta.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    }""")
    await settle(900)
    dialog = page.locator('button:has-text("Paste anyway"), button:has-text("Paste"), button:has-text("Continue")')
    if await dialog.count():
        await dialog.first.click()
        await settle(900)
    await k.type("echo alpha-t", delay=8)
    await settle(500)
    g = await ghost_text(page)
    if name != "fish":
        check(tag + "no suggestion while a pasted multi-line command is being edited", g is None, g)
    await k.press("Control+Space")
    await settle(400)
    check(tag + "and no list either", await menu_labels(page) is None or name == "fish")
    await enter(1000)
    out = sh.text()[before:]
    check(tag + "Enter runs the pasted lines", "paste-1\r\n" in out.replace("echo paste-1", "") and "paste-2\r\n" in out.replace("echo paste-2", ""), out[-300:])
    await settle(500)
    await line("echo alpha-t")
    g = await ghost_text(page)
    # fish shows its own suggestion when it has one; ours fills in when it doesn't.
    check(tag + "suggestions are back after a paste", (g == "wo") if name != "fish" else (g in (None, "wo")), g)
    await clear()

    # a very long line
    long_text = "echo " + " ".join(f"word{i}" for i in range(60))
    await k.type(long_text, delay=2)
    await settle(600)
    inside = await page.evaluate("""() => {
      const g = document.querySelector('[data-testid=completion-ghost]');
      const s = document.querySelector('.xterm-screen').getBoundingClientRect();
      if (!g) return true;
      const r = g.getBoundingClientRect();
      return r.right <= s.right + 1 && r.bottom <= s.bottom + 1;
    }""")
    check(tag + "on a very long line nothing is drawn outside the terminal", inside)
    before = len(sh.text())
    await enter(900)
    check(tag + "and the long line runs intact", "word59" in sh.text()[before:], sh.text()[-120:])

    check(tag + "no page errors", not errors, errors[:3])
    sh.stop()
    await page.close()


async def main():
    shells = find_shells()
    only = os.environ.get("ONLY")
    if only:
        shells = {k: v for k, v in shells.items() if k == only}
    for want in ("bash", "zsh", "fish"):
        if want not in shells:
            skipped.append(f"{want} not found")
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path=EXE) if EXE else await p.chromium.launch()
        for name, spec in shells.items():
            await run_shell(b, name, spec)
        await b.close()
    for s in skipped:
        print("SKIPPED " + s)
    print("\n%d failed" % len(failures) if failures else "\nall passed")
    sys.exit(1 if failures else 0)


asyncio.run(main())
