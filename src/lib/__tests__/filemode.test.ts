import { describe, expect, it } from "vitest";
import { modeString } from "$lib/filemode";

describe("modeString", () => {
  it.each([
    [0o644, "rw-r--r--"],
    [0o755, "rwxr-xr-x"],
    [0o600, "rw-------"],
    [0o000, "---------"],
    [0o777, "rwxrwxrwx"],
    [0o100644, "rw-r--r--"],
  ])("%o is %s", (m, text) => expect(modeString(m)).toBe(text));
  it("is empty where the mode is unknown", () => {
    expect(modeString(null)).toBe("");
    expect(modeString(undefined)).toBe("");
  });
});
