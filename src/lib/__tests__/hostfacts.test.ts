// The app's type setup has no Node types; vitest runs on Node, so these built-ins are there at run time.
// @ts-expect-error Node built-in
import { execFileSync } from "node:child_process";
// @ts-expect-error Node built-in
import { platform } from "node:os";
import { describe, expect, it } from "vitest";
import { FACTS_SCRIPT, factsText, parseFacts, sizeText } from "../hostfacts";

const SAMPLE = [
  "HOSTNAME=web-01",
  "KERNEL=Linux 6.8.0-1",
  "ARCH=x86_64",
  "OS=Ubuntu 24.04.1 LTS",
  "CPU=AMD EPYC 7B13",
  "CORES=8",
  "MEM_KB=16384000",
  "SWAP_KB=0",
  "UPTIME=86400",
  "DISK=/|103080888|41230355|42%",
  "DISK=/data|515928320|10|1%",
  "INIT=systemd",
  "VIRT=kvm",
  "TOOL=docker|Docker version 27.1.1, build abc",
  "TOOL=git|git version 2.43.0",
].join("\n");

describe("host facts", () => {
  it("reads what the script prints", () => {
    const f = parseFacts(SAMPLE);
    expect(f).toMatchObject({ hostname: "web-01", os: "Ubuntu 24.04.1 LTS", kernel: "Linux 6.8.0-1", arch: "x86_64", cpu: "AMD EPYC 7B13", cores: 8, memKb: 16384000, swapKb: 0, uptimeSecs: 86400, init: "systemd", virtualization: "kvm" });
    expect(f.disks).toEqual([
      { mount: "/", sizeKb: 103080888, usedKb: 41230355, usedPct: 42 },
      { mount: "/data", sizeKb: 515928320, usedKb: 10, usedPct: 1 },
    ]);
    expect(f.tools).toEqual([{ name: "docker", version: "Docker version 27.1.1, build abc" }, { name: "git", version: "git version 2.43.0" }]);
  });

  it("leaves out what it was not told, rather than guessing", () => {
    const f = parseFacts("HOSTNAME=\nOS=Linux\nCORES=x\nMEM_KB=-5\nDISK=|1|2|3%\nDISK=/x|nope|1|1%\nTOOL=Bad Name|1\nTOOL=ok|v1\ngarbage\n=novalue\r\n");
    expect(f).toMatchObject({ hostname: null, os: "Linux", cores: null, memKb: null, cpu: null, init: null });
    expect(f.disks).toEqual([]);
    expect(f.tools).toEqual([{ name: "ok", version: "v1" }]);
    expect(parseFacts("")).toMatchObject({ hostname: null, disks: [], tools: [] });
  });

  it("cuts long values and keeps a tool version that contains a bar", () => {
    const f = parseFacts(`OS=${"x".repeat(500)}\nTOOL=git|a|b`);
    expect(f.os?.length).toBe(200);
    expect(f.tools[0].version).toBe("a|b");
  });

  it("writes sizes and a plain-text summary", () => {
    expect([sizeText(null), sizeText(512), sizeText(2048), sizeText(16384000), sizeText(1500)]).toEqual(["—", "512 KiB", "2.0 MiB", "15.6 GiB", "1.5 MiB"]);
    const text = factsText(parseFacts(SAMPLE), Date.UTC(2026, 9, 9, 12, 0), "web-01");
    expect(text).toContain("web-01: facts observed");
    expect(text).toContain("Memory: 15.6 GiB");
    expect(text).toContain("Disk /: 98.3 GiB, 42% used");
    expect(text).toContain("Tool docker: Docker version 27.1.1, build abc");
  });

  it("each tool is asked for at most a few seconds, and kubectl and helm are asked the way they understand", () => {
    expect(FACTS_SCRIPT).toMatch(/timeout 3 "\$@"/);
    expect(FACTS_SCRIPT).toContain("kubectl version --client");
    expect(FACTS_SCRIPT).toContain("helm version --short");
    expect(FACTS_SCRIPT).not.toMatch(/kubectl --version|helm --version/);
  });

  it("the script only reads: nothing in it installs, starts, writes a file or reaches a network", () => {
    for (const bad of [/\brm\b/, /\bsudo\b/, /\bcurl\b|\bwget\b|\bnc\b|\bssh\b/, /\bapt\b|\byum\b|\bdnf\b|\bpip\b|\bnpm\b/, /\bsystemctl\b/, /(?<![=<])>>?\s*(?!\/dev\/null)["']?[\/~$.]/, /\bchmod\b|\bchown\b|\bmkdir\b|\btouch\b/]) {
      expect(FACTS_SCRIPT, String(bad)).not.toMatch(bad);
    }
  });

  // Runs the real script with the system shell, on whatever machine this is (Linux only).
  it.runIf(platform() === "linux")("the real script runs here and prints facts that parse", () => {
    const out = execFileSync("sh", ["-c", FACTS_SCRIPT], { encoding: "utf8", timeout: 30_000 });
    const f = parseFacts(out);
    expect(f.hostname).not.toBeNull();
    expect(f.kernel).toMatch(/^Linux/);
    expect(f.cores).toBeGreaterThan(0);
    expect(f.memKb).toBeGreaterThan(0);
    expect(f.disks.length).toBeGreaterThan(0);
    expect(f.disks.every((d) => d.sizeKb > 0 && d.usedPct !== null)).toBe(true);
  }, 30_000);
});
