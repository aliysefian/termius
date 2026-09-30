import { describe, expect, it } from "vitest";
import { fuzzyScore } from "$lib/fuzzy";
import { estimate } from "$lib/strength";
import { parseAdhoc } from "$lib/ssh";
import { parentPath } from "$lib/sftp";
import { buildTree } from "$lib/tree";
import type { Host, VaultRecord } from "$lib/types";

describe("parseAdhoc", () => {
  it("parses user@host with optional port", () => {
    expect(parseAdhoc("root@10.0.0.5")).toEqual({ username: "root", hostname: "10.0.0.5", port: 22 });
    expect(parseAdhoc(" deploy@web.example.com:2222 ")).toEqual({ username: "deploy", hostname: "web.example.com", port: 2222 });
    expect(parseAdhoc("me@[::1]:2200")).toEqual({ username: "me", hostname: "::1", port: 2200 });
  });
  it("rejects malformed input", () => {
    for (const bad of ["host", "@host", "user@", "user@host:0", "user@host:70000", "a b@host", "user@host:port"]) {
      expect(parseAdhoc(bad), bad).toBeNull();
    }
  });

  // Saved hosts: "bastion" (any user) and "gw" logged in as "ops".
  const find = (name: string, port: number | undefined, user: string | undefined) =>
    name === "bastion" && port === undefined ? "b-id" : name === "gw" && user === "ops" ? "g-id" : undefined;

  it("parses ssh command lines with jump hosts", () => {
    expect(parseAdhoc("ssh -J bastion root@10.0.0.5", find)).toEqual({
      username: "root", hostname: "10.0.0.5", port: 22, jumps: [{ host_id: "b-id" }],
    });
    expect(parseAdhoc("ssh -p 2200 -J bastion,me@hop.example.com:2222 -l root db", find)).toEqual({
      username: "root", hostname: "db", port: 2200,
      jumps: [{ host_id: "b-id" }, { hostname: "hop.example.com", port: 2222, username: "me" }],
    });
    expect(parseAdhoc("-Jops@gw -o Port=2222 root@db", find)?.jumps).toEqual([{ host_id: "g-id" }]);
    expect(parseAdhoc("ssh -o ProxyJump=me@[fd00::1]:22 root@db", find)?.jumps).toEqual([{ hostname: "fd00::1", port: 22, username: "me" }]);
    expect(parseAdhoc("ssh -J none root@db", find)).toEqual({ username: "root", hostname: "db", port: 22 });
  });

  it("refuses what it can't honour", () => {
    for (const bad of [
      "ssh -J unknown root@db", // no user and not saved
      "ssh -i key root@db", // unsupported flag
      "ssh -J bastion", // no destination
      "ssh -J bastion root@a root@b",
      "ssh -o ForwardAgent=yes root@db",
      "ssh -p 99999 root@db",
      "ssh -J root@db",
    ]) {
      expect(parseAdhoc(bad, find), bad).toBeNull();
    }
  });
});

describe("fuzzyScore", () => {
  it("matches in-order subsequences and ranks direct hits first", () => {
    expect(fuzzyScore("pdb", "prod-db-01")).not.toBeNull();
    expect(fuzzyScore("zzz", "prod-db-01")).toBeNull();
    const direct = fuzzyScore("db", "prod-db-01")!;
    const scattered = fuzzyScore("db", "dashboard")!;
    expect(direct).toBeGreaterThan(scattered);
    expect(fuzzyScore("", "anything")).toBe(0);
  });
});

describe("estimate", () => {
  it("scores obvious passwords low and long passphrases high", () => {
    expect(estimate("").score).toBe(0);
    expect(estimate("password123").score).toBeLessThanOrEqual(1);
    expect(estimate("aaaaaaaaaaaaaaaa").score).toBeLessThanOrEqual(1);
    expect(estimate("12345678").score).toBeLessThanOrEqual(1);
    expect(estimate("correct horse battery staple").score).toBeGreaterThanOrEqual(3);
    expect(estimate("Tr0ub4dor&3xQ!zP").score).toBeGreaterThanOrEqual(3);
  });
});

describe("parentPath", () => {
  it("handles POSIX paths", () => {
    expect(parentPath("/home/me/docs", "/")).toBe("/home/me");
    expect(parentPath("/home/", "/")).toBe("/");
    expect(parentPath("/", "/")).toBe("/");
  });
  it("handles Windows paths", () => {
    expect(parentPath("C:\\Users\\me", "\\")).toBe("C:\\Users");
    expect(parentPath("C:\\Users", "\\")).toBe("C:\\");
  });
});

describe("buildTree", () => {
  const h = (id: string, label: string, group: string): VaultRecord<Host> => ({
    id,
    rev: 1,
    updated_at: 0,
    deleted: false,
    device_id: "d",
    data: { label, hostname: "x", port: 22, group, tags: [], notes: "" },
  });
  it("nests groups and sorts children", () => {
    const root = buildTree([h("1", "b", "Prod/DB"), h("2", "a", "Prod/DB"), h("3", "top", ""), h("4", "w", "Prod")]);
    expect(root.hosts.map((x) => x.data!.label)).toEqual(["top"]);
    const prod = root.children[0];
    expect(prod.name).toBe("Prod");
    expect(prod.hosts.map((x) => x.data!.label)).toEqual(["w"]);
    expect(prod.children[0].path).toBe("Prod/DB");
    expect(prod.children[0].hosts.map((x) => x.data!.label)).toEqual(["a", "b"]);
  });
});
