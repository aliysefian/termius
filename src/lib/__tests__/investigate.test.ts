import { describe, expect, it } from "vitest";
import { confidenceText, emptyState, endText, isCurrent, isStaleResult, listenText, parseTargetInput, stateText, targetText, udpNote, unitKind, type Investigation, type ServiceAttribution, type SocketInfo } from "../investigate";

const result = (over: Partial<Investigation> = {}): Investigation => ({ host_id: "h", target: { type: "pid", pid: 1 }, status: "found", taken_at_ms: 0, os: null, sockets: [], subjects: [], warnings: [], ...over });
const sock = (over: Partial<SocketInfo> = {}): SocketInfo => ({ protocol: "tcp", state: "LISTEN", address: "0.0.0.0", port: 8080, ipv6: false, local_only: false, owners: [], uid: null, ...over });
const unit = (over: Partial<ServiceAttribution> = {}): ServiceAttribution => ({ unit_name: "backend.service", unit_type: "service", state: null, main_pid: null, evidence: [], confidence: "confirmed", ...over });

describe("what the person typed", () => {
  it("takes a pid or a port that is a whole number in range", () => {
    expect(parseTargetInput("pid", " 4231 ", "tcp")).toEqual({ target: { type: "pid", pid: 4231 } });
    expect(parseTargetInput("port", "8080", "udp")).toEqual({ target: { type: "port", port: 8080, protocol: "udp" } });
  });
  it("refuses anything else, with a reason", () => {
    for (const [k, t] of [["pid", "0"], ["pid", "-1"], ["pid", "4x"], ["pid", ""], ["pid", "4294967296"], ["port", "0"], ["port", "65536"], ["port", "80;ls"], ["port", "1.5"]] as const) {
      expect(parseTargetInput(k, t, "tcp"), `${k} ${t}`).toHaveProperty("error");
    }
  });
  it("names the target the way a person would", () => {
    expect(targetText({ type: "pid", pid: 4231 })).toBe("PID 4231");
    expect(targetText({ type: "port", port: 8080, protocol: "tcp" })).toBe("TCP :8080");
  });
});

describe("nothing to show", () => {
  it("says which of the empty cases it is", () => {
    expect(emptyState(result())).toBeNull();
    expect(emptyState(result({ status: "process_not_found" }))?.title).toBe("Process not found");
    expect(emptyState(result({ status: "process_not_found", warnings: [{ code: "process_denied", message: "" }] }))?.title).toBe("Insufficient permissions");
    expect(emptyState(result({ status: "port_not_found" }))?.title).toBe("Port not found");
    expect(emptyState(result({ status: "port_not_found", warnings: [{ code: "ss_missing", message: "" }] }))?.title).toBe("Cannot list sockets");
    expect(emptyState(result({ status: "unsupported_os", os: "Darwin" }))?.text).toContain("Darwin");
  });
});

describe("a parent chain", () => {
  it("says nothing when it reached the top, and why it stopped when it did not", () => {
    expect(endText({ kind: "root" })).toBeNull();
    for (const e of [{ kind: "parent_gone", pid: 5 }, { kind: "parent_denied", pid: 5 }, { kind: "parent_reused", pid: 5 }, { kind: "cycle", pid: 5 }, { kind: "depth_limit" }, { kind: "unread", pid: 5 }] as const) {
      expect(endText(e), e.kind).toBeTruthy();
    }
    expect(endText({ kind: "parent_reused", pid: 5 })).toContain("reused");
  });
});

describe("what manages it", () => {
  it("does not call a scope a service", () => {
    expect(unitKind(unit())).toBe("Service");
    expect(unitKind(unit({ unit_name: "session-3.scope", unit_type: "scope" }))).toBe("Login session");
    expect(unitKind(unit({ unit_name: "docker-abc.scope", unit_type: "scope" }))).toBe("Container");
    expect(unitKind(unit({ unit_name: "x.scope", unit_type: "scope" }))).toBe("Scope");
  });
  it("says whether systemd confirmed it", () => {
    expect(confidenceText(unit())).toBe("Confirmed by systemd");
    expect(confidenceText(unit({ confidence: "inferred" }))).toBe("From the cgroup only");
  });
});

describe("sockets", () => {
  it("keeps the address family and what the bind means", () => {
    expect(listenText(sock())).toBe("TCP 0.0.0.0:8080 · all addresses");
    expect(listenText(sock({ address: "::1", ipv6: true, local_only: true }))).toBe("TCP (IPv6) [::1]:8080 · this host only");
    expect(listenText(sock({ address: "10.0.0.5" }))).toBe("TCP 10.0.0.5:8080");
  });
  it("explains UDP only for UDP", () => {
    expect(udpNote(sock())).toBeNull();
    expect(udpNote(sock({ protocol: "udp" }))).toContain("no listening state");
  });
});

describe("states and staleness", () => {
  it("spells out the kernel's state letters and leaves an unknown one as it came", () => {
    expect(stateText("S")).toBe("sleeping (S)");
    expect(stateText("Z")).toBe("zombie (Z)");
    expect(stateText("?")).toBe("?");
    expect(stateText(null)).toBeNull();
  });
  it("calls an old answer stale", () => {
    expect(isStaleResult(result({ taken_at_ms: 0 }), 30_000)).toBe(false);
    expect(isStaleResult(result({ taken_at_ms: 0 }), 61_000)).toBe(true);
  });
  it("lets only the newest request's answer through", () => {
    expect(isCurrent(3, 3)).toBe(true);
    expect(isCurrent(2, 3)).toBe(false);
  });
});
