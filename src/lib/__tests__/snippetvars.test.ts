import { describe, expect, it } from "vitest";
import { promptedVariables, render, type HostContext } from "$lib/snippetvars";

const ctx: HostContext = { host: "web-1", hostname: "10.0.0.5", port: 2222, user: "deploy" };
const now = new Date(2026, 8, 7, 4, 5, 6);

describe("snippet variables", () => {
  it("lists only user-supplied names, once, in order", () => {
    expect(promptedVariables("tail -n {{lines}} {{ file }} {{host}} {{lines}}")).toEqual(["lines", "file"]);
    expect(promptedVariables("echo {{{{literal}}")).toEqual([]);
    expect(promptedVariables("no vars")).toEqual([]);
  });

  it("fills built-ins from the host and prompted values", () => {
    expect(render("ssh {{user}}@{{hostname}} -p {{port}} # {{host}}", ctx, {}, now)).toBe(
      "ssh deploy@10.0.0.5 -p 2222 # web-1",
    );
    expect(render("backup-{{date}}T{{time}}.tgz", ctx, {}, now)).toBe("backup-2026-09-07T04:05:06.tgz");
    expect(render("tail -n {{ lines }} {{file}}", ctx, { lines: "50", file: "/var/log/syslog" }, now)).toBe(
      "tail -n 50 /var/log/syslog",
    );
  });

  it("leaves unknown names and escaped braces alone", () => {
    expect(render("echo {{missing}}", ctx, {}, now)).toBe("echo {{missing}}");
    expect(render("echo {{{{host}}", ctx, {}, now)).toBe("echo {{host}}");
    expect(render("value is empty: [{{v}}]", ctx, { v: "" }, now)).toBe("value is empty: []");
  });
});
