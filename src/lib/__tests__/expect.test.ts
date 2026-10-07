import { describe, expect, it } from "vitest";
import { Expect, plain, problem } from "../expect";

describe("waiting for text and sending text", () => {
  it("sends each answer when its prompt appears, in order", () => {
    const e = new Expect([{ wait: "Select a host:", send: "2" }, { wait: "$", send: "uptime" }], 0);
    expect(e.feed("Welcome\r\n")).toEqual([]);
    expect(e.feed("Select a host: ")).toEqual(["2\r"]);
    expect(e.waiting?.wait).toBe("$");
    expect(e.feed("db-02 ready\r\n$ ")).toEqual(["uptime\r"]);
    expect(e.done).toBe(true);
    expect(e.feed("$ more")).toEqual([]);
  });

  it("finds a prompt split across chunks, and ignores colour codes", () => {
    const e = new Expect([{ wait: "Continue? [y/n]", send: "y" }], 0);
    expect(e.feed("\x1b[1mContinue? [y/")).toEqual([]);
    expect(e.feed("n]\x1b[0m ")).toEqual(["y\r"]);
    expect(plain("\x1b[31mred\x1b[0m\x1b]0;title\x07 text\r\n")).toBe("red text\n");
  });

  it("a prompt that was already used up does not trigger the next step", () => {
    const e = new Expect([{ wait: "ok", send: "a" }, { wait: "ok", send: "b" }], 0);
    expect(e.feed("ok")).toEqual(["a\r"]);
    expect(e.feed("ok")).toEqual(["b\r"]);
    // Both prompts in one chunk answer both, in order.
    const f = new Expect([{ wait: "one", send: "1" }, { wait: "two", send: "2" }], 0);
    expect(f.feed("one then two")).toEqual(["1\r", "2\r"]);
    const g = new Expect([{ wait: "ok", send: "a" }, { wait: "ok", send: "b" }], 0);
    expect(g.feed("ok ok")).toEqual(["a\r", "b\r"]);
  });

  it("gives up on a step that waits too long, and drops the rest", () => {
    const e = new Expect([{ wait: "never", send: "x", timeout_secs: 5 }, { wait: "after", send: "y" }], 1000);
    expect(e.expired(5999)).toBeNull();
    expect(e.expired(6000)?.wait).toBe("never");
    expect(e.done).toBe(true);
    expect(e.feed("after")).toEqual([]);
    expect(e.expired(1e9)).toBeNull();
    // The clock for the next step starts when the one before is answered.
    const f = new Expect([{ wait: "a", send: "1" }, { wait: "b", send: "2" }], 0);
    f.feed("a", 20_000);
    expect(f.expired(40_000)).toBeNull();
    expect(f.expired(50_001)?.wait).toBe("b");
  });

  it("only searches recent output", () => {
    const e = new Expect([{ wait: "needle", send: "x" }], 0);
    e.feed("needle".padStart(10_000, "z").slice(0, 100));
    expect(e.done).toBe(false);
    expect(e.feed("here is the needle")).toEqual(["x\r"]);
  });
});

describe("what a host may do", () => {
  it("accepts ordinary steps", () => {
    expect(problem([])).toBeNull();
    expect(problem([{ wait: "Select:", send: "2" }, { wait: "$ ", send: "cd /srv", timeout_secs: 10 }])).toBeNull();
  });

  it("refuses a step that waits for a secret or sends one", () => {
    for (const wait of ["Password:", "Enter passphrase for key", "PIN:", "Verification code:", "API token", "One-time code", "OTP"]) expect(problem([{ wait, send: "x" }]), wait).toMatch(/password, code or similar/);
    expect(problem([{ wait: "login:", send: "mysql -u root -pHunter2" }])).toMatch(/looks like a secret/);
    expect(problem([{ wait: "login:", send: "export API_TOKEN=abcdef123456" }])).toMatch(/looks like a secret/);
  });

  it("refuses empty, long, multi-line and out-of-range steps", () => {
    expect(problem([{ wait: " ", send: "x" }])).toMatch(/Step 1/);
    expect(problem([{ wait: "a", send: "x".repeat(201) }])).toMatch(/under 200/);
    expect(problem([{ wait: "a\nb", send: "x" }])).toMatch(/line breaks/);
    expect(problem([{ wait: "a", send: "x\ry" }])).toMatch(/line breaks/);
    expect(problem([{ wait: "a", send: "x", timeout_secs: 0 }])).toMatch(/between 1 and 300/);
    expect(problem([{ wait: "a", send: "x", timeout_secs: 301 }])).toMatch(/between 1 and 300/);
    expect(problem(Array.from({ length: 11 }, () => ({ wait: "a", send: "b" })))).toMatch(/At most 10/);
    expect(problem([{ wait: "ok", send: "1" }, { wait: "Password", send: "" }])).toMatch(/Step 2/);
  });
});
