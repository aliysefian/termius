import { describe, expect, it } from "vitest";
import { lineTone } from "../containerlog";

describe("log line tone", () => {
  it("reads errors, warnings and the rest from the line itself", () => {
    expect(lineTone("2026-10-01 ERROR connection refused")).toBe("error");
    expect(lineTone('time=1 level=error msg="x"')).toBe("error");
    expect(lineTone("[crit] 12#12: open() failed")).toBe("error");
    expect(lineTone("Traceback (most recent call last):")).toBe("error");
    expect(lineTone("WARN: slow query")).toBe("warn");
    expect(lineTone("deprecated option used")).toBe("warn");
    expect(lineTone("listening on :8080")).toBe("info");
    expect(lineTone("terrorist")).toBe("info");
  });
});
