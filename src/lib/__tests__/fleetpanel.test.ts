import { describe, expect, it } from "vitest";
import { UNGROUPED, fleetSections } from "../fleetpanel";

const host = (id: string, label: string, group: string, hostname = `${label}.lan`, tags: string[] = []) =>
  ({ id, updated_at: 0, data: { label, hostname, group, tags } }) as never;

const hosts = [host("1", "web-2", "Prod/web"), host("2", "web-1", "Prod/web"), host("3", "db", "Prod/db", "10.0.0.5", ["pg"]), host("4", "box", "")];
const health = { "1": { state: "up" }, "2": { state: "down" }, "3": { state: "up" } };

describe("the fleet panel", () => {
  it("makes a section per group, by name, with hosts that have none last, and counts reachability", () => {
    const s = fleetSections(hosts, health, "", "all");
    expect(s.map((x) => x.name)).toEqual(["Prod/db", "Prod/web", UNGROUPED]);
    expect(s[1].hosts.map((h) => h.data!.label)).toEqual(["web-1", "web-2"]);
    expect(s[1]).toMatchObject({ up: 1, down: 1 });
  });

  it("searches label, address, tag and group, and a group name brings the whole group", () => {
    const names = (q: string) => fleetSections(hosts, health, q, "all").flatMap((s) => s.hosts.map((h) => h.data!.label));
    expect(names("10.0.0")).toEqual(["db"]);
    expect(names("PG")).toEqual(["db"]);
    expect(names("prod/web")).toEqual(["web-1", "web-2"]);
    expect(names("nothing")).toEqual([]);
  });

  it("filters by status, and drops sections left empty", () => {
    expect(fleetSections(hosts, health, "", "down").map((s) => s.name)).toEqual(["Prod/web"]);
    expect(fleetSections(hosts, health, "", "up").flatMap((s) => s.hosts.length)).toEqual([1, 1]);
  });
});
