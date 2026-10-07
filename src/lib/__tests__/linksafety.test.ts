import { describe, expect, it } from "vitest";
import { judgeLink } from "../linksafety";
import { findPathMatches, withoutLocation } from "../termlinks";

describe("links programs put in the terminal", () => {
  it("opens web and mail links, and asks when the text hides the address", () => {
    expect(judgeLink("https://example.com/docs", "https://example.com/docs")).toEqual({ ok: true, confirm: false, address: "https://example.com/docs" });
    expect(judgeLink("https://example.com", "https://example.com/")).toMatchObject({ ok: true, confirm: false });
    expect(judgeLink("https://evil.example/login", "https://bank.example")).toMatchObject({ ok: true, confirm: true, address: "https://evil.example/login" });
    expect(judgeLink("https://example.com/x", "click here")).toMatchObject({ confirm: true });
    expect(judgeLink("mailto:ops@example.com", "ops@example.com")).toMatchObject({ ok: true, confirm: true });
  });

  it("refuses everything else", () => {
    for (const bad of ["file:///etc/passwd", "javascript:alert(1)", "ssh://host", "data:text/html,x", "tauri://localhost", "nonsense", "", "http://a b", "https://x/\u0007bell", "https://x/" + "a".repeat(2100)]) {
      expect(judgeLink(bad, bad).ok, bad).toBe(false);
    }
  });
});

describe("file:line links", () => {
  const found = (s: string) => findPathMatches(s).map((m) => m.text);

  it("take the line and column into the link", () => {
    expect(found("error in /srv/app/main.rs:42:7: expected")).toEqual(["/srv/app/main.rs:42:7"]);
    expect(found("at ./src/lib.rs:10")).toEqual(["./src/lib.rs:10"]);
    expect(found("see ~/notes/todo.md:3: here")).toEqual(["~/notes/todo.md:3"]);
    expect(found("/var/log/syslog")).toEqual(["/var/log/syslog"]);
  });

  it("the folder to browse to is found from the path alone", () => {
    expect(withoutLocation("/srv/app/main.rs:42:7")).toBe("/srv/app/main.rs");
    expect(withoutLocation("./a.rs:9")).toBe("./a.rs");
    expect(withoutLocation("/srv/app")).toBe("/srv/app");
    expect(withoutLocation("/srv/v1.2:ab")).toBe("/srv/v1.2:ab");
  });

  it("still leaves URLs alone", () => {
    expect(found("https://example.com/a/b:80")).toEqual([]);
  });
});
