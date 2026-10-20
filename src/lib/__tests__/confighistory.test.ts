import { describe, expect, it } from "vitest";
import { detectEdits, globToRegExp, matchesWatched, normalizePath, parseLine, parsePatterns, resolvePath } from "$lib/confighistory/terminalEditInterceptor";
import { parseRead, readScript, reloadCommand, writeScript } from "$lib/confighistory/remoteFile";
import { wantsCapture, maxBytes } from "$lib/confighistory/useConfigTimeMachine.svelte";
import { formatBytes } from "$lib/confighistory/snapshotVaultStore.svelte";
import { DEFAULT_PREFS } from "$lib/stores/settings.svelte";

const paths = (line: string, cwd: string | null = "/etc/nginx/conf.d") => detectEdits(line, cwd).flatMap((e) => e.paths);

describe("paths", () => {
  it("resolves against the shell's folder and tidies dots", () => {
    expect(resolvePath("subdomains.conf", "/etc/nginx/conf.d")).toBe("/etc/nginx/conf.d/subdomains.conf");
    expect(resolvePath("../nginx.conf", "/etc/nginx/conf.d")).toBe("/etc/nginx/nginx.conf");
    expect(resolvePath("./a/./b", "/x/")).toBe("/x/a/b");
    expect(resolvePath("/etc//ssh/../hosts", null)).toBe("/etc/hosts");
    expect(resolvePath("~/app/.env", null)).toBe("~/app/.env");
  });
  it("won't guess", () => {
    expect(resolvePath("a.conf", null)).toBeNull();
    expect(resolvePath("a.conf", "relative/dir")).toBeNull();
    expect(resolvePath("~other/x", "/")).toBeNull();
    expect(resolvePath("-", "/")).toBeNull();
    expect(normalizePath("/../../x")).toBe("/x");
  });
});

describe("editor commands", () => {
  it("finds the file an editor opens, with or without sudo", () => {
    expect(paths("vim subdomains.conf")).toEqual(["/etc/nginx/conf.d/subdomains.conf"]);
    expect(paths("sudo nano /etc/nginx/nginx.conf")).toEqual(["/etc/nginx/nginx.conf"]);
    expect(paths("vi /etc/ssh/sshd_config")).toEqual(["/etc/ssh/sshd_config"]);
    expect(paths("sudoedit /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("sudo -e /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(detectEdits("sudo -e /etc/hosts", null)[0].program).toBe("sudoedit");
  });
  it("sees through wrappers, assignments and options", () => {
    expect(paths("EDITOR=x sudo -u root -E nvim +42 -c 'set ft=sh' /etc/fstab")).toEqual(["/etc/fstab"]);
    expect(paths("sudo -H -u deploy env FOO=1 nano -l /etc/a.conf /etc/b.conf")).toEqual(["/etc/a.conf", "/etc/b.conf"]);
    expect(paths("/usr/bin/vim /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("vim -- -weird-name")).toEqual(["/etc/nginx/conf.d/-weird-name"]);
  });
  it("reads quotes", () => {
    expect(paths(`vim "/etc/my dir/a.conf"`)).toEqual(["/etc/my dir/a.conf"]);
    expect(paths(`vim '/etc/it'"'"'s.conf'`)).toEqual(["/etc/it's.conf"]);
    expect(paths("vim /etc/a\\ b.conf")).toEqual(["/etc/a b.conf"]);
  });
  it("finds the files of each command in a line", () => {
    expect(paths("cd /etc && vim hosts; nano /etc/fstab")).toEqual(["/etc/nginx/conf.d/hosts", "/etc/fstab"]);
  });
  it("ignores editors with nothing to open, and other programs", () => {
    expect(detectEdits("vim", "/etc")).toEqual([]);
    expect(detectEdits("ls /etc", "/etc")).toEqual([]);
    expect(detectEdits("cat /etc/hosts", "/etc")).toEqual([]);
    expect(detectEdits("", null)).toEqual([]);
  });
  it("drops what it can't know rather than guess", () => {
    expect(paths("vim $HOME/.bashrc")).toEqual([]);
    expect(paths("vim /etc/*.conf")).toEqual([]);
    expect(paths("vim $(which x)")).toEqual([]);
    expect(paths('vim "/etc/$X"')).toEqual([]);
    expect(detectEdits("vim a.conf", null)[0]).toMatchObject({ paths: [], unresolved: ["a.conf"] });
  });
});

describe("sed, tee and redirects", () => {
  it("catches sed -i in its forms, and not plain sed", () => {
    expect(paths("sed -i 's/a/b/' /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("sudo sed -i.bak -e 's/a/b/' /etc/a /etc/b")).toEqual(["/etc/a", "/etc/b"]);
    expect(paths("sed --in-place 's/a/b/' /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("sed -ni 'p' /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("sed 's/a/b/' /etc/hosts")).toEqual([]);
    expect(paths("sed -n 's/a/b/p' /etc/hosts")).toEqual([]);
  });
  it("catches tee, also at the end of a pipe", () => {
    expect(paths("echo 'x' | sudo tee /etc/sysctl.d/99.conf")).toEqual(["/etc/sysctl.d/99.conf"]);
    expect(paths("echo x | tee -a /etc/hosts /etc/other")).toEqual(["/etc/hosts", "/etc/other"]);
    expect(detectEdits("echo x | tee /etc/hosts", null)[0].program).toBe("tee");
  });
  it("catches redirects, but not descriptors or /dev/null", () => {
    expect(paths("echo x > /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("echo x >> /etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("echo x >/etc/hosts")).toEqual(["/etc/hosts"]);
    expect(paths("make 2>&1 | tee /tmp/x")).toEqual(["/tmp/x"]);
    expect(paths("cmd > /dev/null 2>&1")).toEqual([]);
    expect(paths("cmd 2> /etc/err")).toEqual(["/etc/err"]);
    expect(paths("sort < /etc/hosts")).toEqual([]);
    expect(parseLine("a && b || c")).toHaveLength(3);
  });
});

describe("watched paths", () => {
  const P = DEFAULT_PREFS.cfgPatterns;
  it("matches the defaults", () => {
    for (const p of ["/etc/nginx/nginx.conf", "/etc/hosts", "/usr/local/etc/php.ini", "/var/www/site/index.php", "/srv/app/docker-compose.yml", "/srv/app/docker-compose.prod.yml", "/home/ali/app/.env", "~/app/.env.local", "/etc/systemd/system/app.service", "docker-compose.yml"]) {
      expect(matchesWatched(p, P), p).toBe(true);
    }
  });
  it("doesn't match what isn't watched", () => {
    for (const p of ["/home/ali/notes.txt", "/var/log/syslog", "/tmp/x.conf", "/srv/app/compose.yml", "/etcetera/x"]) {
      expect(matchesWatched(p, P), p).toBe(false);
    }
  });
  it("lets a star stay in its folder and two stars cross them", () => {
    expect(globToRegExp("/etc/*.conf").test("/etc/a.conf")).toBe(true);
    expect(globToRegExp("/etc/*.conf").test("/etc/x/a.conf")).toBe(false);
    expect(globToRegExp("/etc/**").test("/etc/x/y/z")).toBe(true);
    expect(globToRegExp("/etc/**").test("/etc")).toBe(false);
    expect(globToRegExp("/a/?.txt").test("/a/b.txt")).toBe(true);
    expect(globToRegExp("/a.b").test("/aXb")).toBe(false);
  });
  it("reads a list from text", () => {
    expect(parsePatterns("/etc/**\n /var/www/** , /etc/**\n\n")).toEqual(["/etc/**", "/var/www/**"]);
  });
});

describe("the policy", () => {
  const on = { ...DEFAULT_PREFS, cfgEnabled: true };
  it("is off until asked for", () => {
    expect(DEFAULT_PREFS.cfgEnabled).toBe(false);
    expect(wantsCapture(DEFAULT_PREFS, "production", "/etc/hosts")).toBe(false);
  });
  it("follows the environments and the patterns", () => {
    expect(wantsCapture(on, "production", "/etc/hosts")).toBe(true);
    expect(wantsCapture(on, "staging", "/etc/hosts")).toBe(true);
    expect(wantsCapture(on, "", "/etc/hosts")).toBe(true);
    expect(wantsCapture(on, "development", "/etc/hosts")).toBe(false);
    expect(wantsCapture(on, "production", "/home/ali/notes.txt")).toBe(false);
    expect(wantsCapture({ ...on, cfgEnvs: ["development"] }, "development", "/etc/hosts")).toBe(true);
  });
  it("turns kilobytes into the limit", () => {
    expect(maxBytes({ cfgMaxKb: 2048 })).toBe(2 * 1024 * 1024);
    expect(maxBytes({ cfgMaxKb: 0 })).toBe(1024);
    expect(formatBytes(14_900_000)).toBe("14 MB");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });
});

describe("rollback scripts", () => {
  it("read a file's mode and owner before its text", () => {
    expect(parseRead("@@OK 12 644 root:root\nhello\nworld\n")).toEqual({ exists: true, text: "hello\nworld\n", permissions: "0644", owner: "root:root" });
    expect(parseRead("@@MISSING\n").exists).toBe(false);
    expect(parseRead("")).toMatchObject({ exists: false });
    expect(parseRead("@@OK 0 600 -\n")).toMatchObject({ exists: true, text: "", owner: "" });
  });
  it("pass the path as an argument and refuse anything else", () => {
    const s = readScript("/etc/it's.conf");
    expect(s.endsWith("sh '/etc/it'\\''s.conf'")).toBe(true);
    expect(() => readScript("etc/passwd")).toThrow();
    expect(() => readScript("/etc/x\n; reboot")).toThrow();
    expect(() => writeScript("relative", "x", "0644", "root:root")).toThrow();
  });
  it("write by copying over the file, then set the mode and owner back", () => {
    const s = writeScript("/etc/nginx/nginx.conf", "worker_processes 1;\n", "0644", "root:root");
    expect(s).toContain("cat \"$tmp\" > \"$f\"");
    expect(s).toContain('chmod 0644 "$f"');
    expect(s).toContain('chown root:root "$f"');
    expect(s).toContain(btoa("worker_processes 1;\n"));
    expect(s).not.toContain("worker_processes");
    // A mode or owner that isn't plain is left out, never put in a command.
    const odd = writeScript("/etc/a", "x", "0644; reboot", "a b:c");
    expect(odd).not.toContain("chmod");
    expect(odd).not.toContain("chown");
  });
  it("name the command that makes a service read the file again", () => {
    expect(reloadCommand("/etc/nginx/conf.d/subdomains.conf", true)).toBe("nginx -t && systemctl reload nginx");
    expect(reloadCommand("/etc/nginx/nginx.conf", false)).toBe("sudo nginx -t && sudo systemctl reload nginx");
    expect(reloadCommand("/etc/ssh/sshd_config", true)).toContain("sshd -t");
    expect(reloadCommand("/etc/systemd/system/app.service", true)).toBe("systemctl daemon-reload");
    expect(reloadCommand("/etc/hosts", true)).toBeNull();
  });
});
