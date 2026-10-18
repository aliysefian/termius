import { describe, expect, it } from "vitest";
import { backupName, canDiff, compareSides, DIFF_MAX_BYTES, isTextName, planFromDecisions, sizeDeltaText, type Decision, type SideInfo } from "$lib/conflict";

const side = (o: Partial<SideInfo> = {}): SideInfo => ({ name: "nginx.conf", path: "/etc/nginx/nginx.conf", size: 100, modified: 1000, isDir: false, ...o });

describe("compareSides", () => {
  it("matching checksums are the same file", () => {
    expect(compareSides(side({ sha256: "aa" }), side({ sha256: "aa", modified: 5 })).identical).toBe(true);
  });
  it("different checksums differ, and different sizes differ without waiting for one", () => {
    expect(compareSides(side({ sha256: "aa" }), side({ sha256: "bb" })).identical).toBe(false);
    expect(compareSides(side({ size: 1 }), side({ size: 2 })).identical).toBe(false);
  });
  it("the same size with no checksum yet can't be told", () => {
    expect(compareSides(side(), side()).identical).toBeNull();
  });
  it("says which is newer and by how much the size differs", () => {
    expect(compareSides(side({ modified: 2000 }), side()).newer).toBe("source");
    expect(compareSides(side({ modified: 5 }), side()).newer).toBe("destination");
    expect(compareSides(side(), side()).newer).toBe("same");
    expect(compareSides(side({ modified: null }), side()).newer).toBeNull();
    expect(compareSides(side({ size: 150 }), side()).sizeDelta).toBe(50);
  });
  it("reads the size difference aloud", () => {
    const f = (n: number) => `${n} B`;
    expect(sizeDeltaText(50, f)).toBe("50 B larger");
    expect(sizeDeltaText(-7, f)).toBe("7 B smaller");
    expect(sizeDeltaText(0, f)).toBe("same size");
  });
});

describe("canDiff", () => {
  it("is offered for small text files", () => {
    expect(canDiff(side(), side())).toBe(true);
    expect(canDiff(side({ name: ".env" }), side({ name: ".env" }))).toBe(true);
    expect(canDiff(side({ name: "hosts" }), side({ name: "hosts" }))).toBe(true);
  });
  it("is not offered for big files, folders, or things that are not text", () => {
    expect(canDiff(side({ size: DIFF_MAX_BYTES + 1 }), side())).toBe(false);
    expect(canDiff(side(), side({ size: DIFF_MAX_BYTES + 1 }))).toBe(false);
    expect(canDiff(side({ isDir: true }), side())).toBe(false);
    expect(canDiff(side({ name: "backup.tar.gz" }), side())).toBe(false);
    expect(canDiff(side({ name: "photo.png" }), side())).toBe(false);
    expect(canDiff(side({ name: "id_ed25519" }), side())).toBe(false);
  });
  it("knows text names", () => {
    expect(isTextName("main.rs")).toBe(true);
    expect(isTextName("README.md")).toBe(true);
    expect(isTextName("app.service")).toBe(true);
    expect(isTextName("data.bin")).toBe(false);
  });
});

describe("backupName", () => {
  it("is .bak, then numbered when that is taken", () => {
    expect(backupName(new Set(), "a.conf")).toBe("a.conf.bak");
    expect(backupName(new Set(["a.conf.bak"]), "a.conf")).toBe("a.conf.bak.1");
    expect(backupName(new Set(["a.conf.bak", "a.conf.bak.1", "a.conf.bak.2"]), "a.conf")).toBe("a.conf.bak.3");
  });
});

describe("planFromDecisions", () => {
  const base = (p: string) => p.split("/").pop()!;
  it("copies what has no clash as it is and sorts the rest by the answer", () => {
    const sources = ["/s/new.txt", "/s/a.conf", "/s/b.conf", "/s/c.conf", "/s/d.conf"];
    const decisions = new Map<string, Decision>([
      ["a.conf", "overwrite"],
      ["b.conf", "backup"],
      ["c.conf", "rename"],
      ["d.conf", "skip"],
    ]);
    const plan = planFromDecisions(sources, new Set(["a.conf", "b.conf", "c.conf", "d.conf"]), decisions, base);
    expect(plan).toEqual({ overwrite: ["/s/new.txt", "/s/a.conf"], backup: ["/s/b.conf"], rename: ["/s/c.conf"], skipped: ["/s/d.conf"] });
  });
  it("a clash with no answer is left alone", () => {
    const plan = planFromDecisions(["/s/a"], new Set(["a"]), new Map(), base);
    expect(plan.skipped).toEqual(["/s/a"]);
    expect(plan.overwrite).toEqual([]);
  });
});
