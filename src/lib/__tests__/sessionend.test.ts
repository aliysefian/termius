import { describe, expect, it } from "vitest";
import { attemptsAfterDrop, MAX_RECONNECTS, STABLE_MS, wasDropped } from "../sessionend";

describe("wasDropped", () => {
  it("is a drop only when the link died: no status, and the other end did not finish", () => {
    expect(wasDropped({ code: null })).toBe(true);
    expect(wasDropped({ code: null, ended: false })).toBe(true);
  });
  it("is not a drop when the shell exited with a status, including a failing one", () => {
    expect(wasDropped({ code: 0, ended: true })).toBe(false);
    expect(wasDropped({ code: 130, ended: true })).toBe(false);
    expect(wasDropped({ code: 1 })).toBe(false);
  });
  it("is not a drop when the channel closed with no status (EOF, a signal, nested su or sudo)", () => {
    expect(wasDropped({ code: null, ended: true })).toBe(false);
  });
});

describe("attemptsAfterDrop", () => {
  it("keeps counting when the connection was cut off straight after it came up", () => {
    let used = 0;
    // A server that accepts the login and drops it again: every run is short.
    for (let i = 0; i < 10 && used < MAX_RECONNECTS; i++) {
      used = attemptsAfterDrop(used, 500);
      used += 1; // what scheduleReconnect does
    }
    expect(used).toBe(MAX_RECONNECTS);
  });
  it("starts over once the connection held for a while", () => {
    expect(attemptsAfterDrop(MAX_RECONNECTS, STABLE_MS)).toBe(0);
    expect(attemptsAfterDrop(2, STABLE_MS + 1)).toBe(0);
  });
  it("does not forget just under the limit", () => {
    expect(attemptsAfterDrop(2, STABLE_MS - 1)).toBe(2);
  });
});
