import { describe, expect, it } from "vitest";
import { sessionFacts } from "$lib/sessioninfo";

const up = { state: "up", latency_ms: 12, banner: null } as const;

describe("sessionFacts", () => {
  it("an SSH host: protocol, TERM, encoding and the probed latency", () => {
    expect(sessionFacts({ kind: "host" }, {}, up)).toEqual({ protocol: "SSH", term: "xterm-256color", encoding: "UTF-8", latencyMs: 12 });
  });

  it("Mosh and Telnet name themselves and claim no TERM", () => {
    expect(sessionFacts({ kind: "host" }, { mosh: true }, undefined)).toMatchObject({ protocol: "Mosh", term: null, latencyMs: null });
    expect(sessionFacts({ kind: "host" }, { protocol: "telnet" }, undefined)).toMatchObject({ protocol: "Telnet", term: null, encoding: "UTF-8" });
    expect(sessionFacts({ kind: "telnet" }, undefined, undefined).protocol).toBe("Telnet");
  });

  it("a local shell has a TERM but no latency", () => {
    expect(sessionFacts({ kind: "local" }, undefined, up)).toEqual({ protocol: "Local", term: "xterm-256color", encoding: "UTF-8", latencyMs: null });
  });

  it("a graphical session has no text encoding", () => {
    expect(sessionFacts({ kind: "host" }, { protocol: "rdp" }, undefined)).toMatchObject({ protocol: "RDP", encoding: null, term: null });
    expect(sessionFacts({ kind: "host" }, { protocol: "vnc" }, undefined).encoding).toBeNull();
  });

  it("latency is only reported while the host is reachable", () => {
    expect(sessionFacts({ kind: "host" }, {}, { state: "down", reason: "x" }).latencyMs).toBeNull();
    expect(sessionFacts({ kind: "host" }, {}, { state: "via_jump" }).latencyMs).toBeNull();
  });
});
