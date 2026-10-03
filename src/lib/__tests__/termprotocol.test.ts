import { describe, expect, it } from "vitest";
import { decodeOsc52, redundantMouseEnable } from "../termprotocol";

const b64 = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s)));

describe("decodeOsc52", () => {
  it("decodes a clipboard write", () => {
    expect(decodeOsc52(`c;${b64("hello world")}`)).toBe("hello world");
    expect(decodeOsc52(`;${b64("x")}`)).toBe("x");
  });
  it("keeps non-ASCII text intact", () => {
    expect(decodeOsc52(`c;${b64("héllo → ✓")}`)).toBe("héllo → ✓");
  });
  it("tolerates missing padding and wrapped lines", () => {
    const padded = b64("ab"); // "YWI="
    expect(decodeOsc52(`c;${padded.replace(/=+$/, "")}`)).toBe("ab");
    expect(decodeOsc52(`c;YW\nI=`)).toBe("ab");
  });
  it("ignores queries, clears and junk", () => {
    expect(decodeOsc52("c;?")).toBeNull();
    expect(decodeOsc52("c;")).toBeNull();
    expect(decodeOsc52("c")).toBeNull();
    expect(decodeOsc52("c;not base64!")).toBeNull();
  });
});

describe("redundantMouseEnable", () => {
  it("spots a re-enable of the active mode", () => {
    expect(redundantMouseEnable([1000], "vt200")).toBe(true);
    expect(redundantMouseEnable([1002], "drag")).toBe(true);
    expect(redundantMouseEnable([1003], "any")).toBe(true);
    expect(redundantMouseEnable([9], "x10")).toBe(true);
  });
  it("lets real changes and other modes through", () => {
    expect(redundantMouseEnable([1000], "none")).toBe(false);
    expect(redundantMouseEnable([1002], "vt200")).toBe(false);
    expect(redundantMouseEnable([1006], "vt200")).toBe(false);
    expect(redundantMouseEnable([1000, 1006], "vt200")).toBe(false);
    expect(redundantMouseEnable([1049], "vt200")).toBe(false);
    expect(redundantMouseEnable([], "vt200")).toBe(false);
    expect(redundantMouseEnable([[1000]], "vt200")).toBe(false);
  });
});
