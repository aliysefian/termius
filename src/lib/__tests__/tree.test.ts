import { describe, expect, it } from "vitest";
import { buildTree, flattenTree, groupPaths } from "../tree";
import type { Host, VaultRecord } from "../types";

function host(id: string, label: string, group: string): VaultRecord<Host> {
  return {
    id,
    rev: 1,
    updated_at: 0,
    deleted: false,
    device_id: "test",
    data: {
      label,
      group,
      hostname: label,
      port: 22,
      protocol: "ssh",
      tags: [],
      custom: {},
      favorite: false,
    } as unknown as Host,
  };
}

describe("flattenTree", () => {
  const hosts = [host("1", "root-host", ""), host("2", "db1", "Prod/DB"), host("3", "db2", "Prod/DB"), host("4", "web1", "Prod")];
  const tree = buildTree(hosts);

  it("orders child groups (with their subtree) before this level's own hosts", () => {
    const rows = flattenTree(tree, new Set());
    expect(rows.map((r) => r.key)).toEqual(["g:Prod", "g:Prod/DB", "h:2", "h:3", "h:4", "h:1"]);
  });

  it("skips a collapsed group's children", () => {
    const rows = flattenTree(tree, new Set(["Prod/DB"]));
    expect(rows.map((r) => r.key)).toEqual(["g:Prod", "g:Prod/DB", "h:4", "h:1"]);
  });

  it("skips an entire collapsed subtree, nested group included", () => {
    const rows = flattenTree(tree, new Set(["Prod"]));
    expect(rows.map((r) => r.key)).toEqual(["g:Prod", "h:1"]);
  });

  it("records depth and the parent group's key", () => {
    const rows = flattenTree(tree, new Set());
    const db = rows.find((r) => r.key === "g:Prod/DB")!;
    expect(db.depth).toBe(1);
    expect(db.parentKey).toBe("g:Prod");
    const h2 = rows.find((r) => r.key === "h:2")!;
    expect(h2.depth).toBe(2);
    expect(h2.parentKey).toBe("g:Prod/DB");
  });

  it("matches groupPaths for the groups it lists", () => {
    const rows = flattenTree(tree, new Set());
    const groupKeys = rows.filter((r) => r.kind === "group").map((r) => r.path);
    expect(groupKeys.sort()).toEqual(groupPaths(tree).sort());
  });
});
