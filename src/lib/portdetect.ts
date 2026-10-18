// Spots "this server is now listening on port N" in what a remote terminal prints, so the port can be offered for
// forwarding. It reads output the terminal is already drawing and must never slow that down: `PortDetector.feed` does
// a bounded amount of work per chunk however much arrives (`cat huge.log` is millions of lines), and the scanning
// happens at most once per interval on a capped buffer.

export interface PortHit {
  port: number;
  /** Which pattern matched, for tests and for explaining a detection. */
  source: string;
}

// A name that means "this computer" from the server's own point of view.
const LOCAL = String.raw`(?:localhost|127(?:\.\d{1,3}){3}|0\.0\.0\.0|\[::1?\]|\*)`;
const START = String.raw`\b(?:listening|running|started|serving|available|ready|local|server|bound|accepting)\b`;

interface Pattern {
  name: string;
  re: RegExp;
  /** The pattern needs the port to be one an ordinary person could be hosting a dev server on. */
  unprivileged?: boolean;
}

const PATTERNS: Pattern[] = [
  // "Listening on http://0.0.0.0:8000", "Uvicorn running on http://127.0.0.1:8000", "➜  Local:   http://localhost:5173/",
  // "Starting development server at http://127.0.0.1:8000/", "Server listening at localhost:3000"
  { name: "start-host-port", re: new RegExp(`${START}[^\\n]{0,80}?(?:https?://|tcp://)?${LOCAL}:(\\d{2,5})\\b`, "i") },
  // "Server listening on port 3000", "Tomcat started on port(s): 8080 (http)", "Serving HTTP on 0.0.0.0 port 8000"
  { name: "start-port", re: /\b(?:listening|running|started|serving)\b[^\n]{0,60}?\bport(?:\(s\))?[: ]+(\d{2,5})\b/i },
  // Go-style "listening on :8080"
  { name: "start-colon-port", re: /\b(?:listening|serving|running)\b[^\n]{0,20}?\s:(\d{2,5})\b/i },
  // Docker "0.0.0.0:8080->80/tcp"
  { name: "docker", re: new RegExp(`\\b(?:0\\.0\\.0\\.0|\\[::\\]|127\\.0\\.0\\.1):(\\d{2,5})->\\d+/tcp`, "i") },
  // A bare local URL, as Jupyter and many dev servers print on a line of their own. Only for ordinary ports, and
  // lines that are someone's request or command are left out below.
  { name: "local-url", re: new RegExp(`\\bhttps?://${LOCAL}:(\\d{2,5})\\b`, "i"), unprivileged: true },
];

/** Lines that mention a local URL without a server having started: a command typed, or a request logged. */
const NOISE = /(?:^\s*[$#>]\s)|\b(?:curl|wget|http|https|httpie|fetch)\s+(?:-\S+\s+)*https?:|\b(?:GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s|\bHTTP\/[12]\b|\bproxy(?:ing)?\b|->\s*https?:/i;

/** Ports never offered: SSH itself is how the person is connected. */
const NEVER = new Set([22]);

// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]/g;

export const stripAnsi = (s: string) => s.replace(ANSI, "");

/** The ports one line announces, most explicit pattern first, each at most once. */
export function portsInLine(line: string): PortHit[] {
  const out: PortHit[] = [];
  const noisy = NOISE.test(line);
  for (const p of PATTERNS) {
    const m = p.re.exec(line);
    if (!m) continue;
    // A command typed or a request logged shows an address without announcing a server (an explicit "listening"
    // phrase in the same line still counts).
    if (p.name === "local-url" && noisy) continue;
    const port = Number(m[1]);
    if (!Number.isInteger(port) || port < 1 || port > 65535 || NEVER.has(port)) continue;
    if (p.unprivileged && port < 1024) continue;
    if (!out.some((h) => h.port === port)) out.push({ port, source: p.name });
  }
  return out;
}

/** Every port announced in `text` (complete lines), in order of appearance. */
export function findPorts(text: string): PortHit[] {
  const out: PortHit[] = [];
  for (const raw of stripAnsi(text).split(/\r\n|\r|\n/)) {
    for (const hit of portsInLine(raw)) if (!out.some((h) => h.port === hit.port)) out.push(hit);
  }
  return out;
}

export interface PortDetectorOptions {
  /** Wait this long after output arrives before scanning it, so a flood is scanned once, not per chunk. */
  delayMs?: number;
  /** Most text kept between scans; older text is dropped (it has scrolled away). */
  maxBuffer?: number;
  /** Most bytes of one chunk that are even decoded: a huge chunk is mostly scrollback nobody needs scanned. */
  maxChunk?: number;
  /** The same port is not reported again within this long. */
  repeatMs?: number;
  now?: () => number;
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (id: unknown) => void;
}

export class PortDetector {
  #decoder = new TextDecoder("utf-8", { fatal: false });
  #pending = "";
  #timer: unknown = null;
  #lastSeen = new Map<number, number>();
  #scans = 0;
  readonly #delay: number;
  readonly #maxBuffer: number;
  readonly #maxChunk: number;
  readonly #repeat: number;
  readonly #now: () => number;
  readonly #setTimer: (fn: () => void, ms: number) => unknown;
  readonly #clearTimer: (id: unknown) => void;

  constructor(
    private readonly onPort: (hit: PortHit) => void,
    opts: PortDetectorOptions = {},
  ) {
    this.#delay = opts.delayMs ?? 250;
    this.#maxBuffer = opts.maxBuffer ?? 16 * 1024;
    this.#maxChunk = opts.maxChunk ?? 8 * 1024;
    this.#repeat = opts.repeatMs ?? 30_000;
    this.#now = opts.now ?? (() => Date.now());
    this.#setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.#clearTimer = opts.clearTimer ?? ((id) => clearTimeout(id as ReturnType<typeof setTimeout>));
  }

  /** How many scans have run (for tests and to show the work stays bounded). */
  get scans() {
    return this.#scans;
  }

  /** Take in a chunk of terminal output. Cheap: copies at most `maxChunk` bytes and arms one timer. */
  feed(bytes: Uint8Array) {
    // A chunk bigger than the limit is mostly output that has already scrolled by; only its tail is of interest.
    const tail = bytes.length > this.#maxChunk ? bytes.subarray(bytes.length - this.#maxChunk) : bytes;
    this.#pending += this.#decoder.decode(tail, { stream: true });
    if (this.#pending.length > this.#maxBuffer) this.#pending = this.#pending.slice(-this.#maxBuffer);
    if (this.#timer === null) this.#timer = this.#setTimer(() => this.#scan(), this.#delay);
  }

  #scan() {
    this.#timer = null;
    this.#scans++;
    // Only complete lines: a number split across two chunks must not be read as a shorter one.
    const end = Math.max(this.#pending.lastIndexOf("\n"), this.#pending.lastIndexOf("\r"));
    if (end < 0) {
      // No line break yet; keep a bounded partial line for the next scan.
      if (this.#pending.length > 1024) this.#pending = this.#pending.slice(-1024);
      return;
    }
    const complete = this.#pending.slice(0, end + 1);
    this.#pending = this.#pending.slice(end + 1).slice(-1024);
    const now = this.#now();
    for (const hit of findPorts(complete)) {
      const last = this.#lastSeen.get(hit.port);
      if (last !== undefined && now - last < this.#repeat) continue;
      this.#lastSeen.set(hit.port, now);
      this.onPort(hit);
    }
  }

  /** Forget a port so it can be offered again (the person dismissed it, or its forward was stopped). */
  forget(port: number) {
    this.#lastSeen.delete(port);
  }

  dispose() {
    if (this.#timer !== null) this.#clearTimer(this.#timer);
    this.#timer = null;
    this.#pending = "";
    this.#lastSeen.clear();
  }
}
