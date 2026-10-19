import { describe, expect, it } from "vitest";
import { attachCommand, parseProbe, parseScreenList, parseTmuxVersion, prefixSequence, supportsControlMode, supportsHexKeys, validSessionName } from "$lib/tmux/discovery";

describe("tmux version", () => {
  it("reads releases, suffixes and builds from source", () => {
    expect(parseTmuxVersion("tmux 3.3a")).toMatchObject({ major: 3, minor: 3, dev: false });
    expect(parseTmuxVersion("tmux next-3.5")).toMatchObject({ major: 3, minor: 5, dev: true });
    expect(parseTmuxVersion("tmux 1.8")).toMatchObject({ major: 1, minor: 8 });
    expect(parseTmuxVersion("tmux master")!.major).toBe(Infinity);
    expect(parseTmuxVersion("bash: tmux: command not found")).toBeNull();
    expect(parseTmuxVersion("")).toBeNull();
  });
  it("gates control mode at 1.8 and hex keys at 3.0", () => {
    const v = (s: string) => parseTmuxVersion(s);
    expect([supportsControlMode(v("tmux 1.7")), supportsControlMode(v("tmux 1.8")), supportsControlMode(v("tmux 2.0")), supportsControlMode(null)]).toEqual([false, true, true, false]);
    expect([supportsHexKeys(v("tmux 2.9a")), supportsHexKeys(v("tmux 3.0")), supportsHexKeys(v("tmux master"))]).toEqual([false, true, true]);
  });
});

const PROBE = `@@tmux-version
tmux 3.4
@@tmux-sessions
$0|0|1|0|1790918085
$1|deploy|3|1|1792436614
@@tmux-windows
0|0|zsh|zsh|1790918100
deploy|0|editor|vim|1792436700
deploy|1|web|nginx|1792436700
deploy|2|top|htop|1792436700
@@screen
There are screens on:
\t4242.build\t(Detached)
\t4100.old\t(05/01/26 10:00:00 AM)\t(Attached)
2 Sockets in /run/screen/S-ali.
`;

describe("probe output", () => {
  it("lists sessions with their windows and last attach", () => {
    const p = parseProbe(PROBE);
    expect(p.tmux.version?.major).toBe(3);
    expect(p.tmux.sessions.map((s) => [s.name, s.windows, s.attached])).toEqual([
      ["0", 1, 0],
      ["deploy", 3, 1],
    ]);
    const d = p.tmux.sessions[1];
    expect(d.lastAttached).toBe(1792436700);
    expect(d.windowList.map((w) => w.command)).toEqual(["vim", "nginx", "htop"]);
    expect(p.tmux.sessions[0].windowList).toEqual([{ index: 0, name: "zsh", command: "zsh" }]);
  });
  it("lists screen sessions and skips the chatter", () => {
    expect(parseProbe(PROBE).screen).toEqual([
      { id: "4242.build", name: "build", state: "detached" },
      { id: "4100.old", name: "old", state: "attached" },
    ]);
    expect(parseScreenList(["No Sockets found in /run/screen/S-ali."])).toEqual([]);
    expect(parseScreenList(["\t99.gone\t(Dead ???)"])).toEqual([]);
  });
  it("copes with nothing installed, or a server that isn't running", () => {
    const empty = parseProbe("@@tmux-version\n@@tmux-sessions\n@@tmux-windows\n@@screen\n");
    expect(empty).toEqual({ tmux: { version: null, sessions: [] }, screen: [] });
    expect(parseProbe("")).toEqual(empty);
  });
  it("keeps a session name with a bar in it", () => {
    const p = parseProbe("@@tmux-sessions\n$2|a|b|2|0|1700000000\n");
    expect(p.tmux.sessions[0]).toMatchObject({ name: "a|b", windows: 2 });
  });
});

describe("commands typed into the shell", () => {
  it("quotes names that came from the host", () => {
    expect(attachCommand.tmuxNative("deploy")).toBe("tmux -CC -u attach -t 'deploy'");
    expect(attachCommand.tmuxStandard("x; rm -rf ~")).toBe("tmux attach -t 'x; rm -rf ~'");
    expect(attachCommand.newNative("")).toBe("tmux -CC -u new");
    expect(attachCommand.newNative("it's")).toBe("tmux -CC -u new -s 'it'\\''s'");
    expect(attachCommand.screen("4242.build")).toBe("screen -r '4242.build'");
  });
  it("accepts the names tmux accepts", () => {
    expect(["deploy", "my build", "a-b_c", "日本"].every(validSessionName)).toBe(true);
    expect(["a.b", "a:b", "a\nb", "x".repeat(101)].some(validSessionName)).toBe(false);
  });
  it("has one-key versions of the common actions for the fallback toolbar", () => {
    expect(prefixSequence("tmux", "newWindow")).toBe("\x02c");
    expect(prefixSequence("tmux", "splitRight")).toBe("\x02%");
    expect(prefixSequence("screen", "detach")).toBe("\x01d");
    expect(prefixSequence("screen", "splitRight")).toBeNull();
  });
});
