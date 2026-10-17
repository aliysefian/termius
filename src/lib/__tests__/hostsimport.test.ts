import { describe, expect, it } from "vitest";
import { describeResult, initialChoices, nameProblems, setAll, toResolutions, type Choice } from "../hostsimport";
import type { HostsImportConflict } from "../types";

const conflicts: HostsImportConflict[] = [
  { id: "a", label: "web-01", group: "Prod", suggested: "web-01 (2)" },
  { id: "b", label: "db-01", group: "Prod/DB", suggested: "db-01 (2)" },
];

describe("host import answers", () => {
  it("starts by keeping everything under the suggested names", () => {
    expect(toResolutions(conflicts, initialChoices(conflicts))).toEqual([
      { id: "a", action: "rename", label: "web-01 (2)" },
      { id: "b", action: "rename", label: "db-01 (2)" },
    ]);
    expect(nameProblems(conflicts, initialChoices(conflicts))).toEqual({});
  });

  it("can skip them all, or go back to renaming them all", () => {
    expect(toResolutions(conflicts, setAll(conflicts, "skip")).map((r) => r.action)).toEqual(["skip", "skip"]);
    expect(toResolutions(conflicts, setAll(conflicts, "rename")).map((r) => r.action)).toEqual(["rename", "rename"]);
  });

  it("trims what was typed and asks again when a name is empty, unchanged or chosen twice", () => {
    const choices: Record<string, Choice> = { a: { action: "rename", label: "  web-01 new " }, b: { action: "rename", label: "DB-01" } };
    expect(toResolutions(conflicts, choices)[0]).toEqual({ id: "a", action: "rename", label: "web-01 new" });
    expect(nameProblems(conflicts, choices)).toEqual({ b: "That is the name already in use. Pick a different one." });
    expect(nameProblems(conflicts, { ...choices, a: { action: "rename", label: " " } }).a).toBe("Type a name.");
    // Two hosts in one group can't get the same name; the same name in different groups is fine.
    const twins: HostsImportConflict[] = [...conflicts, { id: "c", label: "web-02", group: "Prod", suggested: "x" }];
    const same: Record<string, Choice> = { a: { action: "rename", label: "Copy" }, b: { action: "skip" }, c: { action: "rename", label: "copy" } };
    expect(Object.keys(nameProblems(twins, same))).toEqual(["c"]);
    const apart: HostsImportConflict[] = [conflicts[0], { id: "d", label: "web-01", group: "Staging", suggested: "x" }];
    expect(nameProblems(apart, { a: { action: "rename", label: "Copy" }, d: { action: "rename", label: "Copy" } })).toEqual({});
    // A skipped host needs no name.
    expect(nameProblems(conflicts, { a: { action: "skip" }, b: { action: "skip" } })).toEqual({});
  });

  it("says what happened", () => {
    expect(describeResult({ hosts_added: 3, hosts_renamed: 1, hosts_skipped: 2, groups_added: 1, identities_added: 2, keys_added: 0, proxies_added: 1, warnings: [] })).toEqual([
      "Added 3 hosts, 1 under a new name.",
      "Skipped 2 hosts.",
      "Also added 1 group, 2 logins, 1 proxy.",
    ]);
    expect(describeResult({ hosts_added: 1, hosts_renamed: 0, hosts_skipped: 0, groups_added: 0, identities_added: 0, keys_added: 0, proxies_added: 0, warnings: [] })).toEqual(["Added 1 host."]);
  });
});
