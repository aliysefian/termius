import { describe, expect, it } from "vitest";
import { ALWAYS_SHOWN, RAIL_GROUPS, arrange, move } from "../railitems";

const names = (g: { view: string }[][]) => g.map((x) => x.map((i) => i.view));

describe("the rail", () => {
  it("has each page once, and Hosts can't be put away", () => {
    const all = RAIL_GROUPS.flat().map((i) => i.view);
    expect(new Set(all).size).toBe(all.length);
    expect(ALWAYS_SHOWN).toContain("hosts");
  });

  it("is drawn as given when nothing is chosen", () => {
    const r = arrange(RAIL_GROUPS, [], []);
    expect(names(r.groups)).toEqual(names(RAIL_GROUPS));
    expect(r.away).toEqual([]);
  });

  it("applies an order inside a group only, and leaves unnamed entries where they were, after the named ones", () => {
    const r = arrange(RAIL_GROUPS, ["forwarding", "snippets"], []);
    expect(names(r.groups)[1]).toEqual(["forwarding", "snippets", "sftp"]);
    expect(names(r.groups)[0]).toEqual(["hosts", "favorites"]);
    // An entry named for another group does not pull across.
    expect(names(arrange(RAIL_GROUPS, ["ops", "snippets"], []).groups)[1]).toEqual(["snippets", "sftp", "forwarding"]);
  });

  it("takes hidden entries out and hands them back, keeps a group's line only while it has entries", () => {
    const r = arrange(RAIL_GROUPS, [], ["databases", "containers", "ops", "favorites"]);
    expect(r.away.map((i) => i.view)).toEqual(["favorites", "databases", "containers", "ops"]);
    expect(names(r.groups)).toEqual([["hosts"], ["snippets", "sftp", "forwarding"]]);
  });

  it("never hides Hosts, and ignores names it doesn't know", () => {
    const r = arrange(RAIL_GROUPS, ["nonsense", "hosts"], ["hosts", "nothing"]);
    expect(names(r.groups)[0][0]).toBe("hosts");
    expect(r.away).toEqual([]);
  });

  it("moves an entry up or down within its group, and stops at the ends", () => {
    let order: string[] = [];
    order = move(RAIL_GROUPS, order, "forwarding", -1);
    expect(names(arrange(RAIL_GROUPS, order, []).groups)[1]).toEqual(["snippets", "forwarding", "sftp"]);
    order = move(RAIL_GROUPS, order, "forwarding", -1);
    expect(names(arrange(RAIL_GROUPS, order, []).groups)[1]).toEqual(["forwarding", "snippets", "sftp"]);
    expect(move(RAIL_GROUPS, order, "forwarding", -1)).toBe(order);
    order = move(RAIL_GROUPS, order, "sftp", 1);
    expect(move(RAIL_GROUPS, order, "sftp", 1)).toBe(order);
    // Another group's order is kept.
    order = move(RAIL_GROUPS, order, "ops", -1);
    expect(names(arrange(RAIL_GROUPS, order, []).groups)).toEqual([["hosts", "favorites"], ["forwarding", "snippets", "sftp"], ["databases", "ops", "containers"]]);
    expect(move(RAIL_GROUPS, ["x"], "unknown" as never, 1)).toEqual(["x"]);
  });
});
