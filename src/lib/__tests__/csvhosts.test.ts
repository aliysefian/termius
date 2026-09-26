import { describe, expect, it } from "vitest";
import { guessMapping, rowsToHosts, secretColumns } from "../csvhosts";

describe("CSV host import", () => {
  const headers = ["Groups", "Label", "Tags", "Hostname/IP", "Protocol", "Port", "Username", "Password"];

  it("guesses Termius-style columns and flags secrets", () => {
    const m = guessMapping(headers);
    expect(m).toMatchObject({ hostname: 3, label: 1, port: 5, user: 6, group: 0, tags: 2 });
    expect(secretColumns(headers)).toEqual(["Password"]);
  });

  it("turns rows into unique hosts and skips rows without an address", () => {
    const m = guessMapping(headers);
    const { hosts, skipped } = rowsToHosts(
      [
        ["Prod, DB", "db-01", "sql, eu", "10.0.0.5", "ssh", "2222", "ops", "hunter2"],
        ["", "db-01", "", "10.0.0.6", "ssh", "", "", ""],
        ["", "", "", "", "", "", "", ""],
      ],
      m,
    );
    expect(skipped).toBe(1);
    expect(hosts[0]).toMatchObject({ alias: "db-01", hostname: "10.0.0.5", port: 2222, user: "ops", group: "Prod/DB", tags: ["sql", "eu"] });
    expect(hosts[1].alias).toBe("db-01 (2)");
    expect(JSON.stringify(hosts)).not.toContain("hunter2");
  });
});
