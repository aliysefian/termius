import { describe, expect, it } from "vitest";
import { sshCommand, timeAgo } from "../sshcmd";
import type { Host, Identity, VaultRecord } from "../types";

const rec = <T>(id: string, data: T): VaultRecord<T> => ({ id, rev: 1, updated_at: 0, deleted: false, device_id: "d", data });
const host = (label: string, hostname: string, extra: Partial<Host> = {}): Host => ({
  label, hostname, port: 22, group: "", tags: [], notes: "", ...extra,
});

describe("sshCommand", () => {
  const ident = rec("i1", { label: "ops", username: "ops", auth: { type: "agent" }, notes: "" } as Identity);
  const bastion = rec("b", host("bastion", "bastion.example.com", { port: 2222, identity_id: "i1" }));
  const gw = rec("g", host("gw", "10.0.0.1", { jump_host_id: "b" }));
  const web = host("web", "10.0.1.5", { port: 22, identity_id: "i1", jump_host_id: "g", forward_agent: true });
  const input = {
    hostById: new Map([[bastion.id, bastion], [gw.id, gw]]),
    identityById: new Map([[ident.id, ident]]),
    identityFor: (h: Host) => h.identity_id,
    jumpFor: (h: Host) => h.jump_host_id,
  };

  it("builds a chain, outermost jump first, without secrets", () => {
    expect(sshCommand({ ...input, host: web })).toBe("ssh -J ops@bastion.example.com:2222,10.0.0.1 -A ops@10.0.1.5");
  });

  it("keeps it minimal for a plain host", () => {
    expect(sshCommand({ ...input, host: host("x", "example.org", { port: 2200 }) })).toBe("ssh -p 2200 example.org");
  });
});

describe("timeAgo", () => {
  it("rounds to the nearest unit", () => {
    const now = 1_000_000_000;
    expect(timeAgo(now - 5_000, now)).toBe("just now");
    expect(timeAgo(now - 90_000, now)).toBe("2 minutes ago");
    expect(timeAgo(now - 3_600_000 * 5, now)).toBe("5 hours ago");
    expect(timeAgo(now - 86_400_000, now)).toBe("1 day ago");
  });
});
