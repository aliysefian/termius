import { describe, expect, it } from "vitest";
import { normalizeMac, validBroadcast } from "../wol";

describe("Wake-on-LAN addresses", () => {
  it("accepts the usual spellings and writes them one way", () => {
    for (const s of ["00:1a:2b:3c:4d:5e", "00-1A-2B-3C-4D-5E", "001A.2B3C.4D5E", "001a2b3c4d5e", " 00:1A:2B:3C:4D:5E "]) expect(normalizeMac(s), s).toBe("00:1A:2B:3C:4D:5E");
  });
  it("refuses what is not a MAC address", () => {
    for (const s of ["", "00:1A:2B:3C:4D", "00:1A:2B:3C:4D:5G", "0:0:0:0:0:0:0:0:0:0:0:0", "001A:2B3C:4D5E", "00 1A 2B 3C 4D 5E"]) expect(normalizeMac(s), s).toBeNull();
  });
  it("checks the broadcast address", () => {
    expect(validBroadcast("")).toBe(true);
    expect(validBroadcast("192.168.1.255")).toBe(true);
    expect(validBroadcast("192.168.1.255:7")).toBe(true);
    expect(validBroadcast("192.168.1.256")).toBe(false);
    expect(validBroadcast("example.com")).toBe(false);
  });
});
