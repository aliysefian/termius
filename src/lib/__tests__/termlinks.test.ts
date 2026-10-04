import { describe, expect, it } from "vitest";
import { findHostPortMatches, findPathMatches, resolveBrowsePath } from "../termlinks";

describe("findPathMatches", () => {
  it("finds absolute, home and relative paths", () => {
    expect(findPathMatches("edit /etc/nginx/nginx.conf now")).toEqual([{ start: 5, end: 26, text: "/etc/nginx/nginx.conf" }]);
    expect(findPathMatches("see ~/.ssh/config for keys")).toEqual([{ start: 4, end: 17, text: "~/.ssh/config" }]);
    expect(findPathMatches("run ./build.sh please")).toEqual([{ start: 4, end: 14, text: "./build.sh" }]);
    expect(findPathMatches("back to ../lib/app.js")).toEqual([{ start: 8, end: 21, text: "../lib/app.js" }]);
  });

  it("strips trailing punctuation", () => {
    expect(findPathMatches("look at /var/log/app.log.")).toEqual([{ start: 8, end: 24, text: "/var/log/app.log" }]);
    expect(findPathMatches("(/tmp/out, done)")).toEqual([{ start: 1, end: 9, text: "/tmp/out" }]);
  });

  it("ignores a URL's own scheme separator", () => {
    expect(findPathMatches("fetch https://example.com/a/b for it")).toEqual([]);
  });

  it("requires at least two segments for a bare absolute path", () => {
    expect(findPathMatches("just /etc here")).toEqual([]);
  });
});

describe("resolveBrowsePath", () => {
  it("takes the parent of an absolute path", () => {
    expect(resolveBrowsePath("/etc/nginx/nginx.conf", undefined)).toBe("/etc/nginx");
    expect(resolveBrowsePath("/etc", undefined)).toBe("/");
  });

  it("resolves a relative path against the known cwd", () => {
    expect(resolveBrowsePath("./build.sh", "/srv/app")).toBe("/srv/app");
    expect(resolveBrowsePath("logs/today.log", "/srv/app")).toBe("/srv/app/logs");
    expect(resolveBrowsePath("../other/file.txt", "/srv/app/sub")).toBe("/srv/app/other");
  });

  it("passes a relative path through as typed with no known cwd", () => {
    expect(resolveBrowsePath("logs/today.log", undefined)).toBe("logs");
  });

  it("passes a ~ path through untouched (home isn't knowable from OSC 7)", () => {
    expect(resolveBrowsePath("~/.ssh/config", "/srv/app")).toBe("~/.ssh");
  });
});

describe("findHostPortMatches", () => {
  it("finds a hostname:port and an ip:port", () => {
    expect(findHostPortMatches("connect to db.internal:5432 now")).toEqual([{ start: 11, end: 27, text: "db.internal:5432" }]);
    expect(findHostPortMatches("ping 10.0.0.5:22 works")).toEqual([{ start: 5, end: 16, text: "10.0.0.5:22" }]);
  });

  it("rejects a port out of range", () => {
    expect(findHostPortMatches("weird host:99999 here")).toEqual([]);
  });

  it("ignores the authority part of a URL", () => {
    expect(findHostPortMatches("open https://example.com:8080/path")).toEqual([]);
  });
});
