#!/usr/bin/env python3
"""Record real shell sessions as test fixtures for src/lib/completion/line.ts.

Each session starts a shell in a pseudo-terminal (40 columns, so long lines wrap), types the
steps below, and stores the bytes typed and the bytes the shell sent back after each step.
The expectations are what the line SHOULD be at that point, written here from the intent of
each step; they are not read back from the shell's output.

    python3 record.py bash|zsh|fish integrated|plain > <shell>-<mode>.json

Needs the shell on PATH (or SHELL_BIN and, for relocated packages, LD_LIBRARY_PATH / ZSH_FPATH /
ZSH_MODULE_PATH). The integration snippets are the ones SSHVault shows in Settings.
"""
import base64, json, os, socket, pty, select, sys, tempfile, time, fcntl, termios, struct, subprocess

shell, mode = sys.argv[1], sys.argv[2]
integrated = mode == "integrated"
COLS, ROWS = 40, 12

LONG = "echo " + " ".join(["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet", "kilo", "lima", "mike"])
CLEAR = "\x15"  # kill everything before the cursor (the cursor is at the end whenever this is used)

def L(text, at_end=True): return {"text": text, "atEnd": at_end}

# (label, typed, wait seconds after, expect when integrated, expect when plain)
# An expectation is None (no suggestions) or {"text": ..., "atEnd": ...}; atEnd is optional.
password_cmd = {
    "bash": 'read -s -p "Password: " x\r',
    "zsh": 'read -s "x?Password: "\r',
    "fish": 'read -s -P "Password: " x\r',
}[shell]
STEPS = [
    ("start", "", 0.6, L(""), L("")),
    ("ls", "ls", 0.25, L("ls"), L("ls")),
    ("ls-la", " -la", 0.25, L("ls -la"), L("ls -la")),
    ("clear", CLEAR, 0.25, L(""), L("")),
    ("wrap-1", LONG[:30], 0.25, L(LONG[:30]), L(LONG[:30])),
    ("wrap-2", LONG[30:], 0.25, L(LONG), L(LONG)),
    ("clear-2", CLEAR, 0.25, L(""), L("")),
    ("multibyte", "echo héllo 東京", 0.25, L("echo héllo 東京"), L("echo héllo 東京")),
    ("clear-3", CLEAR, 0.25, L(""), L("")),
    ("left-typed", "abc", 0.2, L("abc"), L("abc")),
    ("left", "\x1b[D", 0.25, {"text": "ab", "atEnd": False}, None),
    ("left-back", "\x1b[C", 0.25, L("abc"), None),
    ("clear-4", CLEAR, 0.25, L(""), L("")),
    ("ran", "echo one\r", 0.6, L(""), L("")),
    # Up recalls the previous command, which only the screen shows. (Not recorded for fish: in a bare
    # pseudo-terminal fish 3.7 recalls nothing on Up, so there is no recalled line to read.)
    *([] if shell == "fish" else [("up", "\x1b[A", 0.3, L("echo one"), None)]),
    ("clear-5", CLEAR, 0.25, L(""), L("")),
    # fish shows its own grey suggestion after the cursor; the others don't.
    ("own-suggestion", "ec", 0.4, {"text": "ec", "atEnd": shell != "fish"}, {"text": "ec", "atEnd": shell != "fish"}),
    ("clear-6", CLEAR, 0.25, L(""), L("")),
    ("running", "sleep 1\r", 0.3, None, L("")),   # without integration a running command can't be seen
    ("done", "", 1.4, L(""), L("")),
    ("secret-start", password_cmd, 0.5, None, None),
    ("secret-typed", "hunter2", 0.3, None, None),
    # bash's silent read prints no newline, so the next prompt follows "Password: " on the same row and
    # is (rightly) not offered suggestions without shell integration; zsh and fish do print the newline.
    ("secret-done", "\r", 0.5, L(""), None if shell == "bash" else L("")),
    ("mouse-on", "printf '\\033[?1000h'\r", 0.4, L(""), L("")),
    ("mouse-typed", "abc", 0.3, None, None),
    ("mouse-clear", "\x15", 0.2, None, None),
    ("mouse-off", "printf '\\033[?1000l'\r", 0.4, L(""), L("")),
    ("alt-on", "printf '\\033[?1049h'\r", 0.4, None, None),
    ("alt-typed", "abc", 0.3, None, None),
]
# 'mouse-on' is recorded before the mode takes effect (the command is still running when the line is
# typed), so its own expectation is about the line AFTER the command; the replay checks it once the
# output has been fed, which is exactly when the mode is on, so it expects None.
STEPS = [(l, t, w, (None if l == "mouse-on" else i), (None if l == "mouse-on" else p)) for (l, t, w, i, p) in STEPS]
# After the mouse mode is switched off again the line is readable once more (expectations above).

home = tempfile.mkdtemp(prefix="rec-")
env = {"HOME": home, "TERM": "xterm-256color", "LANG": "C.UTF-8", "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LD_LIBRARY_PATH": os.environ.get("LD_LIBRARY_PATH", ""), "USER": "tester", "HOSTNAME": "host", "HOST": "host"}

snippets = {
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
    # fish 3.7 sends no marks (fish 4 does), so the snippet adds them: A and the folder before the
    # prompt, B right after it, C when a command starts, D with its status when it ends.
    "fish": """function __sshvault_postexec --on-event fish_postexec; printf '\\e]133;D;%s\\a' $status; end
function __sshvault_prompt --on-event fish_prompt; printf '\\e]7;file://%s%s\\a\\e]133;A\\a' $hostname $PWD; end
function __sshvault_preexec --on-event fish_preexec; printf '\\e]133;C\\a'; end
functions -c fish_prompt __sshvault_orig_prompt
function fish_prompt; __sshvault_orig_prompt; printf '\\e]133;B\\a'; end
""",
}
if shell == "bash":
    rc = os.path.join(home, ".bashrc")
    open(rc, "w").write("PS1='$ '\nHISTFILE=/dev/null\n" + (snippets["bash"] if integrated else ""))
    cmd = [os.environ.get("SHELL_BIN", "bash"), "--noprofile", "--rcfile", rc, "-i"]
elif shell == "zsh":
    open(os.path.join(home, ".zshenv"), "w").write(
        (f"fpath=({os.environ['ZSH_FPATH']} $fpath)\n" if os.environ.get("ZSH_FPATH") else "")
        + (f"module_path=({os.environ['ZSH_MODULE_PATH']})\n" if os.environ.get("ZSH_MODULE_PATH") else ""))
    open(os.path.join(home, ".zshrc"), "w").write("PS1='$ '\nunsetopt PROMPT_SP\nHISTFILE=/dev/null\n" + (snippets["zsh"] if integrated else ""))
    env["ZDOTDIR"] = home
    cmd = [os.environ.get("SHELL_BIN", "zsh"), "-d", "-i"]
else:
    cfg = os.path.join(home, ".config", "fish"); os.makedirs(cfg)
    open(os.path.join(cfg, "config.fish"), "w").write("set -g fish_greeting\nfunction fish_prompt; echo -n '$ '; end\nset -g fish_history_path /dev/null\n" + (snippets["fish"] if integrated else ""))
    env["XDG_CONFIG_HOME"] = os.path.join(home, ".config"); env["XDG_DATA_HOME"] = os.path.join(home, ".local", "share")
    cmd = [os.environ.get("SHELL_BIN", "fish"), "-i"]
    if not integrated:  # plain: the same prompt, none of the marks
        cmd = [os.environ.get("SHELL_BIN", "fish"), "-i", "--no-config", "--init-command", "set -g fish_greeting; function fish_prompt; echo -n '$ '; end; set -g fish_history_path /dev/null"]

pid, fd = pty.fork()
if pid == 0:
    os.chdir(home)
    os.execvpe(cmd[0], cmd, env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

def drain(wait):
    out = b""
    end = time.time() + wait
    last = time.time()
    while time.time() < end or (time.time() - last) < 0.12:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk: break
            out += chunk; last = time.time()
        elif time.time() >= end:
            break
    return out

version = subprocess.run([cmd[0], "--version"], capture_output=True, text=True, env=env).stdout.splitlines()[0]
frames = []
for label, typed, wait, ei, ep in STEPS:
    if typed: os.write(fd, typed.encode())
    out = drain(wait).replace(socket.gethostname().encode(), b"host")  # keep this machine's name out of the fixture
    frames.append({"label": label, "typed": base64.b64encode(typed.encode()).decode(), "output": base64.b64encode(out).decode(), "expect": ei if integrated else ep})
os.write(fd, b"\x15exit\r")
time.sleep(0.2)
print(json.dumps({"shell": shell, "version": version, "mode": mode, "cols": COLS, "rows": ROWS, "frames": frames}, indent=1))
