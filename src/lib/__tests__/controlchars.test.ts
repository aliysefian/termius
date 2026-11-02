import { describe, expect, it } from "vitest";
import { hasControlChars, showControlChars } from "$lib/controlchars";

describe("control characters in names", () => {
  it("finds C0, DEL and C1 characters", () => {
    for (const bad of ["a\u0000b", "a\u0015b", "a\u001bb", "a\nb", "a\tb", "a\u007fb", "a\u0080b", "a\u009fb"]) expect(hasControlChars(bad)).toBe(true);
  });
  it("leaves ordinary names alone, whatever the language", () => {
    for (const ok of ["plain.txt", "with space", "é日本語", "😀", "a\u00a0b", "-rf", "$(x)", "it's"]) expect(hasControlChars(ok)).toBe(false);
  });
  it("shows them escaped, so a name cannot hide what it is", () => {
    expect(showControlChars("x\u0015touch y #")).toBe("x\\x15touch y #");
    expect(showControlChars("a\u001b[2Jb")).toBe("a\\x1b[2Jb");
    expect(showControlChars("normal")).toBe("normal");
  });
});
