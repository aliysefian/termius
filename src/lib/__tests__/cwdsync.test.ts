// The two tests that run a real shell are skipped where there is no POSIX one. The app's type setup has no Node
// types, hence the suppressed imports.
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { execFileSync } from "node:child_process";
// @ts-expect-error Node built-in, available to vitest but not typed for the app
import { platform } from "node:os";
import { describe, expect, it } from "vitest";
import { cdCommand, followedPath, OSC7_BASH_ZSH, OSC7_FISH, shellQuote } from "$lib/cwdsync";

const posix = platform() !== "win32";

describe("cdCommand", () => {
  it("quotes a path so it stays one word", () => {
    expect(cdCommand("/etc/nginx/conf.d")).toBe("cd '/etc/nginx/conf.d'");
    expect(cdCommand("/srv/my app")).toBe("cd '/srv/my app'");
    expect(cdCommand("/srv/it's; rm -rf ~ #")).toBe(`cd '/srv/it'\\''s; rm -rf ~ #'`);
    expect(cdCommand("/a/$(id)/`x`")).toBe("cd '/a/$(id)/`x`'");
  });
  it("uses double quotes for a Windows shell", () => {
    expect(cdCommand("C:\\Users\\me\\My Docs", true)).toBe('cd "C:\\Users\\me\\My Docs"');
  });
  it.skipIf(!posix)("a quoted word survives a real shell", () => {
    for (const path of ["/tmp", "/a b/c", "/it's/odd", "/x$(echo pwned)/y", "/q\"uote", "/back`tick`"]) {
      const out = execFileSync("sh", ["-c", `printf '%s' ${shellQuote(path)}`]).toString();
      expect(out).toBe(path);
    }
  });
});

describe("followedPath", () => {
  const base = { follow: true, paneHostId: "h1", terminalHostId: "h1", cwd: "/etc/nginx/conf.d", current: "/home/u" };
  it("goes where the terminal went", () => {
    expect(followedPath(base)).toBe("/etc/nginx/conf.d");
  });
  it("stays when it is already there, ignoring a trailing slash", () => {
    expect(followedPath({ ...base, current: "/etc/nginx/conf.d" })).toBeNull();
    expect(followedPath({ ...base, current: "/etc/nginx/conf.d/" })).toBeNull();
    expect(followedPath({ ...base, cwd: "/", current: "/" })).toBeNull();
  });
  it("stays when it is off, or the terminal has not said where it is", () => {
    expect(followedPath({ ...base, follow: false })).toBeNull();
    expect(followedPath({ ...base, cwd: undefined })).toBeNull();
  });
  it("a pane showing this computer follows a shell on this computer", () => {
    expect(followedPath({ ...base, paneHostId: "local", terminalHostId: "local", cwd: "/home/me/src" })).toBe("/home/me/src");
    expect(followedPath({ ...base, paneHostId: "local", terminalHostId: "h1" })).toBeNull();
    expect(followedPath({ ...base, paneHostId: "h1", terminalHostId: "local" })).toBeNull();
  });
  it("follows only the terminal of the same host", () => {
    expect(followedPath({ ...base, terminalHostId: "h2" })).toBeNull();
    expect(followedPath({ ...base, terminalHostId: null })).toBeNull();
    expect(followedPath({ ...base, paneHostId: null })).toBeNull();
  });
});

describe("the setup lines", () => {
  it("are one line each", () => {
    expect(OSC7_BASH_ZSH).not.toContain("\n");
    expect(OSC7_FISH).not.toContain("\n");
  });

  it.skipIf(!posix)("make bash print an OSC 7 for the new folder at the next prompt", () => {
    // Run it in bash, change folder, and the next prompt hook prints the folder.
    const out = execFileSync("bash", ["-c", `PROMPT_COMMAND=''; ${OSC7_BASH_ZSH}; cd /tmp; eval "$PROMPT_COMMAND"`]).toString();
    expect(out).toMatch(/\u001b\]7;file:\/\/[^/]*\/tmp\u001b\\/);
  });
});
