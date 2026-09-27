import { describe, expect, it } from "vitest";
import { matchesForward } from "../forwardsearch";
import type { ForwardRule, Host } from "../types";

const host: Host = { label: "db-01", hostname: "10.0.0.5", port: 22, group: "Production/DB", tags: ["postgres"], notes: "" };
const pg: ForwardRule = { label: "Postgres", host_id: "h", auto_start: false, kind: "local", bind_addr: "127.0.0.1", bind_port: 5433, dest_host: "localhost", dest_port: 5432 };
const socks: ForwardRule = { label: "Office proxy", host_id: "h", auto_start: false, kind: "dynamic", bind_addr: "127.0.0.1", bind_port: 1080 };

describe("forward search", () => {
  it("matches label, host, group, tags, ports and type", () => {
    for (const q of ["postgres", "db-01", "10.0.0.5", "production", "5433", "5432", "localhost:5432", "local", "-L", "db 5433"]) {
      expect(matchesForward(pg, host, undefined, q), q).toBe(true);
    }
    expect(matchesForward(socks, host, undefined, "socks")).toBe(true);
    expect(matchesForward(pg, host, undefined, "socks")).toBe(false);
    expect(matchesForward(pg, host, undefined, "db-01 redis")).toBe(false);
    expect(matchesForward(pg, undefined, undefined, "missing")).toBe(true);
  });

  it("filters by status, and the status is searchable", () => {
    const running = { state: "active" as const, port: 5433 };
    expect(matchesForward(pg, host, running, "", "running")).toBe(true);
    expect(matchesForward(pg, host, undefined, "", "running")).toBe(false);
    expect(matchesForward(pg, host, { state: "error", message: "x" }, "error")).toBe(true);
    expect(matchesForward(pg, host, undefined, "stopped")).toBe(true);
  });
});
