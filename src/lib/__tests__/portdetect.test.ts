import { describe, expect, it } from "vitest";
import { findPorts, PortDetector, portsInLine, stripAnsi } from "$lib/portdetect";

const ports = (text: string) => findPorts(text).map((h) => h.port);

describe("ports a server announces", () => {
  it.each([
    ["Vite", "  VITE v5.4.0  ready in 312 ms\n\n  ➜  Local:   http://localhost:5173/\n  ➜  Network: use --host to expose\n", [5173]],
    ["Next.js", "   ▲ Next.js 14.2.3\n   - Local:        http://localhost:3000\n   - Network:      http://192.168.1.4:3000\n", [3000]],
    ["Uvicorn / FastAPI", "INFO:     Uvicorn running on http://127.0.0.1:8000 (Press CTRL+C to quit)\n", [8000]],
    ["Gunicorn", "[2026-10-18 10:00:00 +0000] [1] [INFO] Listening at: http://0.0.0.0:8000 (1)\n", [8000]],
    ["Django", "Starting development server at http://127.0.0.1:8000/\nQuit the server with CONTROL-C.\n", [8000]],
    ["Flask", " * Running on http://127.0.0.1:5000\n * Running on http://10.0.0.5:5000\n", [5000]],
    ["Express", "Server listening on port 3000\n", [3000]],
    ["Express with a host", "Example app listening at http://localhost:4000\n", [4000]],
    ["Go net/http", "2026/10/18 10:00:00 listening on :8080\n", [8080]],
    ["python http.server", "Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...\n", [8000]],
    ["Spring Boot", "o.s.b.w.embedded.tomcat.TomcatWebServer  : Tomcat started on port(s): 8080 (http) with context path ''\n", [8080]],
    ["Rails Puma", "* Listening on http://127.0.0.1:3000\n* Listening on tcp://0.0.0.0:3001\n", [3000, 3001]],
    ["webpack-dev-server", "<i> [webpack-dev-server] Project is running at:\n<i> [webpack-dev-server] Loopback: http://localhost:8080/\n", [8080]],
    ["Jupyter", "    http://localhost:8888/lab?token=abc123\n", [8888]],
    ["docker ps", "a1b2c3  nginx  \"nginx\"  2 minutes ago  Up 2 minutes  0.0.0.0:8081->80/tcp, :::8081->80/tcp  web\n", [8081]],
    ["IPv6 loopback", "Listening on http://[::1]:9000\n", [9000]],
  ])("%s", (_name, output, want) => {
    expect(ports(output)).toEqual(want);
  });

  it("reads the same lines through the terminal's colour codes", () => {
    const coloured = "\x1b[32m➜\x1b[39m  \x1b[1mLocal\x1b[22m:   \x1b[36mhttp://localhost:\x1b[1m5173\x1b[22m/\x1b[39m\n";
    expect(stripAnsi(coloured)).toBe("➜  Local:   http://localhost:5173/\n");
    expect(ports(coloured)).toEqual([5173]);
  });

  it("names the pattern that matched", () => {
    expect(portsInLine("listening on :8080")).toEqual([{ port: 8080, source: "start-colon-port" }]);
    expect(portsInLine("Uvicorn running on http://127.0.0.1:8000")[0].source).toBe("start-host-port");
  });
});

describe("things that are not a server starting", () => {
  it.each([
    ["a clock", "build started at 10:45:30\n"],
    ["a typed command", "$ curl http://localhost:3000/health\n"],
    ["a command with flags", "curl -s -X POST http://localhost:8000/api\n"],
    ["a logged request", '127.0.0.1 - - [18/Oct/2026] "GET http://localhost:8000/x HTTP/1.1" 200 -\n'],
    ["a request line", "GET http://localhost:9000/status 200 4ms\n"],
    ["a proxy rule", "proxying http://localhost:3000 -> http://localhost:4000\n"],
    ["somebody else's server", "see https://example.com:8443/docs for details\n"],
    ["SSH itself", "Listening on port 22\n"],
    ["a port that is too big", "Listening on http://localhost:99999\n"],
    ["a privileged port in a bare URL", "  http://localhost:80/\n"],
    ["nothing", "total 24\ndrwxr-xr-x  3 root root 4096 Oct 18 10:00 .\n"],
  ])("%s", (_name, output) => {
    expect(ports(output)).toEqual([]);
  });

  it("but an explicit 'listening' line on a low port still counts", () => {
    expect(ports("Listening on http://0.0.0.0:80\n")).toEqual([80]);
  });
});

/** A timer the test controls. */
function fakeClock() {
  let now = 0;
  let next: { at: number; fn: () => void } | null = null;
  return {
    now: () => now,
    setTimer: (fn: () => void, ms: number) => {
      next = { at: now + ms, fn };
      return 1;
    },
    clearTimer: () => {
      next = null;
    },
    advance(ms: number) {
      now += ms;
      if (next && next.at <= now) {
        const f = next.fn;
        next = null;
        f();
      }
    },
  };
}

const bytes = (s: string) => new TextEncoder().encode(s);

describe("PortDetector", () => {
  it("reports a port once the line is complete and the delay has passed", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c });
    d.feed(bytes("Listening on :80"));
    c.advance(300);
    expect(got).toEqual([]); // the line is not finished: ":80" might be the start of ":8000"
    d.feed(bytes("00\n"));
    c.advance(300);
    expect(got).toEqual([8000]);
  });

  it("does not report the same port again within the repeat window, and does after it", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c, repeatMs: 30_000 });
    for (let i = 0; i < 3; i++) {
      d.feed(bytes("Listening on :3000\n"));
      c.advance(300);
    }
    expect(got).toEqual([3000]);
    c.advance(31_000);
    d.feed(bytes("Listening on :3000\n"));
    c.advance(300);
    expect(got).toEqual([3000, 3000]);
  });

  it("can be told to forget a port so it is offered again at once", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c });
    d.feed(bytes("Listening on :3000\n"));
    c.advance(300);
    d.forget(3000);
    d.feed(bytes("Listening on :3000\n"));
    c.advance(300);
    expect(got).toEqual([3000, 3000]);
  });

  it("scans a flood once per interval, not once per chunk, and finds the port in it", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c });
    const line = bytes("2026-10-18 10:00:00 INFO request handled in 3ms path=/api/items\n");
    for (let i = 0; i < 20_000; i++) d.feed(line);
    d.feed(bytes("Server listening on port 4321\n"));
    expect(d.scans).toBe(0);
    c.advance(300);
    expect(d.scans).toBe(1);
    expect(got).toEqual([4321]);
  });

  it("keeps the work bounded for a huge chunk: only its tail is looked at", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c });
    // 40 MB of log with a start-up line near the beginning (long scrolled away) and another at the very end.
    const filler = "x".repeat(1000) + "\n";
    const big = new Uint8Array(40 * 1024 * 1024);
    const text = bytes("Listening on :1111\n" + filler.repeat(10));
    big.set(text, 0);
    big.fill(120, text.length);
    const endLine = bytes("\nListening on :2222\n");
    big.set(endLine, big.length - endLine.length);
    const started = performance.now();
    d.feed(big);
    const feedMs = performance.now() - started;
    c.advance(300);
    expect(got).toEqual([2222]);
    expect(feedMs).toBeLessThan(200);
  });

  it("is quiet after dispose", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c });
    d.feed(bytes("Listening on :3000\n"));
    d.dispose();
    c.advance(1000);
    expect(got).toEqual([]);
  });

  it("copes with a character split across chunks", () => {
    const c = fakeClock();
    const got: number[] = [];
    const d = new PortDetector((h) => got.push(h.port), { ...c });
    const all = bytes("➜  Local:   http://localhost:5173/\n");
    d.feed(all.subarray(0, 2)); // cuts the 3-byte arrow
    d.feed(all.subarray(2));
    c.advance(300);
    expect(got).toEqual([5173]);
  });
});
