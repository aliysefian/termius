import { describe, expect, it } from "vitest";
import { syncBadge } from "../syncstatus";

describe("the sync badge", () => {
  it("says nothing when there is nothing wrong", () => {
    expect(syncBadge({ conflicts: 0, rollbacks: 0 })).toBeNull();
    expect(syncBadge({ conflicts: -3, rollbacks: Number.NaN })).toBeNull();
  });

  it("points at conflicts", () => {
    expect(syncBadge({ conflicts: 1, rollbacks: 0 })).toMatchObject({ text: "1 sync conflict", tone: "warning" });
    expect(syncBadge({ conflicts: 12, rollbacks: 0 })?.text).toBe("12 sync conflicts");
  });

  it("puts a folder that went backwards first, and still mentions the conflicts", () => {
    const b = syncBadge({ conflicts: 2, rollbacks: 1 })!;
    expect(b).toMatchObject({ text: "Folder went backwards: 1 record", tone: "danger" });
    expect(b.title).toContain("2 sync conflicts need a decision");
    expect(syncBadge({ conflicts: 0, rollbacks: 4 })?.title).toContain("4 records are older than");
  });

  it("never claims the devices are in sync", () => {
    for (const p of [{ conflicts: 0, rollbacks: 0 }, { conflicts: 1, rollbacks: 0 }, { conflicts: 0, rollbacks: 1 }]) {
      expect(JSON.stringify(syncBadge(p) ?? {})).not.toMatch(/in sync|synced|up to date/i);
    }
  });
});
