import { describe, expect, it } from "vitest";
import { ageText, estimateText, formatBytes } from "../dbinspect";

describe("dbinspect", () => {
  it("writes sizes in the unit that fits", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.00 MB");
    expect(formatBytes(123 * 1024 ** 3)).toBe("123 GB");
    expect(formatBytes(-1)).toBe("");
  });
  it("always marks a row count as an estimate", () => {
    expect(estimateText(2500)).toBe("≈ 2,500 rows (estimate)");
    expect(estimateText(null)).toBe("unknown");
  });
  it("writes a connection's age", () => {
    expect(ageText(5000)).toBe("5 s");
    expect(ageText(3 * 60_000)).toBe("3 min");
    expect(ageText(125 * 60_000)).toBe("2 h 5 min");
  });
});

import { connectionFacts, tableFacts } from "../dbinspect";

describe("connectionFacts", () => {
  const base = { engineLabel: "PostgreSQL", host: "db.example", port: 5432, user: "app", database: "shop", tls: "verify_full" as const, sshHost: null, environment: "production", readOnly: false, version: "16.2", connectedAt: 1000, now: 1000 + 3 * 60_000 };

  it("describes a connected connection, with no password", () => {
    const f = connectionFacts(base);
    expect(f.find((x) => x.label === "Status")?.value).toBe("Connected for 3 min");
    expect(f.find((x) => x.label === "Server version")?.value).toBe("16.2");
    expect(f.find((x) => x.label === "Address")?.value).toBe("db.example:5432");
    expect(f.find((x) => x.label === "Encryption")?.value).toBe("TLS, certificate checked");
    expect(f.some((x) => /password/i.test(x.label))).toBe(false);
  });

  it("says nothing about a server it has not talked to", () => {
    const f = connectionFacts({ ...base, version: null, connectedAt: null });
    expect(f[0].value).toBe("Not connected");
    expect(f.some((x) => x.label === "Server version")).toBe(false);
  });

  it("warns about no encryption, unless an SSH tunnel carries it", () => {
    expect(connectionFacts({ ...base, tls: "disable" }).find((x) => x.label === "Encryption")).toMatchObject({ warn: true });
    const tunnel = connectionFacts({ ...base, tls: "disable", sshHost: "bastion" });
    expect(tunnel.find((x) => x.label === "Encryption")?.value).toBe("None, inside the SSH tunnel");
    expect(tunnel.find((x) => x.label === "SSH tunnel")?.value).toBe("through bastion");
  });

  it("says whether sessions are read-only", () => {
    expect(connectionFacts({ ...base, readOnly: true }).find((x) => x.label === "Sessions")?.value).toMatch(/Read-only/);
  });
});

describe("tableFacts", () => {
  it("lists only what was reported", () => {
    const meta = { supported: true, kind: "table", comment: null, approx_rows: 2500, size_bytes: 1048576, indexes: [{}, {}], constraints: [], referenced_by: [], column_notes: [], ddl: null } as never;
    const info = { columns: [{}, {}, {}], primary_key: ["id"] } as never;
    const f = tableFacts("public", "items", meta, info);
    expect(f.find((x) => x.label === "Rows")?.value).toBe("≈ 2,500 rows (estimate)");
    expect(f.find((x) => x.label === "Size")?.value).toBe("1.00 MB");
    expect(f.find((x) => x.label === "Indexes")?.value).toBe("2");
    expect(f.find((x) => x.label === "Primary key")?.value).toBe("id");
    expect(tableFacts("public", "items", null, null).map((x) => x.label)).toEqual(["Name", "Schema"]);
  });
});
