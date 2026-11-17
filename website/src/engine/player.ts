import type { Timeline } from "./timeline";

export type PlayerStatus = "idle" | "playing" | "paused" | "ended";

export interface Scheduler {
  request(cb: (now: number) => void): number;
  cancel(handle: number): void;
  now(): number;
}

export const browserScheduler: Scheduler = {
  request: (cb) => requestAnimationFrame(cb),
  cancel: (h) => cancelAnimationFrame(h),
  now: () => performance.now(),
};

export interface PlayerOptions {
  scheduler?: Scheduler;
  speed?: number;
  /** Loop the range, resting `hold` ms on the last frame first. */
  loop?: { hold: number };
  /** Play only this slice [from, to] of the timeline (a feature clip). */
  range?: [number, number];
  /** Begin paused at this time (reduced motion shows the poster). */
  startAt?: number;
}

export interface PlayerSnapshot {
  time: number;
  status: PlayerStatus;
  speed: number;
}

type Listener = () => void;

/**
 * The single clock. One requestAnimationFrame loop advances `time` by real elapsed time x speed; every
 * visual is a pure function of `time`, so there are no chained timeouts to race or leak. `advance()` moves
 * the clock by hand for tests and for frame-by-frame export.
 */
export class Player {
  private _time: number;
  private _speed: number;
  private _status: PlayerStatus = "idle";
  private handle: number | null = null;
  private last = 0;
  private holdLeft = 0;
  private listeners = new Set<Listener>();
  private frameListeners = new Set<Listener>();
  private snap: PlayerSnapshot;
  readonly scheduler: Scheduler;
  readonly range: [number, number];
  readonly loop: { hold: number } | null;

  constructor(readonly timeline: Timeline, opts: PlayerOptions = {}) {
    this.scheduler = opts.scheduler ?? browserScheduler;
    this._speed = opts.speed ?? 1;
    this.range = opts.range ?? [0, timeline.duration];
    this.loop = opts.loop ?? null;
    this._time = Math.min(Math.max(opts.startAt ?? this.range[0], this.range[0]), this.range[1]);
    this.snap = { time: this._time, status: "idle", speed: this._speed };
  }

  get time() {
    return this._time;
  }
  get status() {
    return this._status;
  }
  get speed() {
    return this._speed;
  }
  get playing() {
    return this._status === "playing";
  }
  /** Progress through the range, 0..1. */
  get progress() {
    const [a, b] = this.range;
    return b > a ? (this._time - a) / (b - a) : 1;
  }
  /** Stable object for useSyncExternalStore; replaced only when something changes. */
  getSnapshot = (): PlayerSnapshot => this.snap;

  subscribe = (l: Listener) => {
    this.listeners.add(l);
    return () => void this.listeners.delete(l);
  };

  /** Called on every clock update (for imperative visuals such as the cursor). */
  onFrame(l: Listener) {
    this.frameListeners.add(l);
    return () => void this.frameListeners.delete(l);
  }

  play() {
    if (this._status === "playing") return;
    if (this._time >= this.range[1] && !this.loop) this._time = this.range[0];
    this.holdLeft = 0;
    this._status = "playing";
    this.last = this.scheduler.now();
    this.schedule();
    this.emit();
  }

  pause() {
    if (this._status !== "playing") return;
    this._status = "paused";
    this.cancel();
    this.emit();
  }

  toggle() {
    if (this.playing) this.pause();
    else this.play();
  }

  restart() {
    this._time = this.range[0];
    this.holdLeft = 0;
    this._status = "paused";
    this.play();
  }

  seek(t: number) {
    this._time = Math.min(Math.max(t, this.range[0]), this.range[1]);
    this.holdLeft = 0;
    if (this._status === "ended" || this._status === "idle") this._status = "paused";
    this.emit();
  }

  /** Jump to the start of step i, optionally keeping it playing. */
  seekToStep(i: number) {
    const items = this.timeline.items;
    const item = items[Math.min(Math.max(i, 0), items.length - 1)];
    if (item) this.seek(item.start);
  }

  nextStep() {
    const i = this.timeline.indexAt(this._time);
    // Skip over zero-length steps so one press always moves visibly.
    let n = i + 1;
    while (n < this.timeline.items.length - 1 && this.timeline.items[n]!.end === this.timeline.items[n]!.start) n++;
    this.seekToStep(n);
  }

  prevStep() {
    const i = this.timeline.indexAt(this._time);
    const cur = this.timeline.items[i];
    // Within the first 600 ms of a step go to the previous one, otherwise back to this step's start.
    let n = cur && this._time - cur.start > 600 ? i : i - 1;
    while (n > 0 && this.timeline.items[n]!.end === this.timeline.items[n]!.start) n--;
    this.seekToStep(n);
  }

  nextChapter() {
    const c = this.timeline.chapters.find((c) => c.start > this._time + 1);
    if (c) this.seek(c.start);
  }

  prevChapter() {
    const cur = this.timeline.chapterAt(this._time);
    const list = this.timeline.chapters;
    if (!cur) return this.seek(this.range[0]);
    const idx = list.indexOf(cur);
    this.seek(this._time - cur.start > 800 || idx === 0 ? cur.start : list[idx - 1]!.start);
  }

  setSpeed(s: number) {
    this._speed = s;
    this.emit();
  }

  /** Stop the clock for good (unmount). Idempotent; the player can be resumed with play(). */
  destroy() {
    this.cancel();
    if (this._status === "playing") this._status = "paused";
    this.listeners.clear();
    this.frameListeners.clear();
  }

  /** Move the clock by `ms` of timeline time. Used by the rAF loop, tests and the video exporter. */
  advance(ms: number) {
    const [, end] = this.range;
    if (this.holdLeft > 0) {
      this.holdLeft -= ms;
      if (this.holdLeft <= 0) {
        this._time = this.range[0];
        this.holdLeft = 0;
      }
    } else {
      this._time += ms;
      if (this._time >= end) {
        this._time = end;
        if (this.loop) this.holdLeft = Math.max(1, this.loop.hold);
        else {
          this._status = "ended";
          this.cancel();
        }
      }
    }
    this.emit();
  }

  private schedule() {
    this.handle = this.scheduler.request(this.frame);
  }

  private cancel() {
    if (this.handle !== null) this.scheduler.cancel(this.handle);
    this.handle = null;
  }

  private frame = (now: number) => {
    this.handle = null;
    if (this._status !== "playing") return;
    // Cap a single step so a background-tab stall does not skip the whole tutorial.
    const dt = Math.min(now - this.last, 100);
    this.last = now;
    this.advance(dt * this._speed);
    if (this._status === "playing") this.schedule();
  };

  private emit() {
    const s = this.snap;
    // Snapshot identity changes only on status/speed changes or time moves, never on a no-op.
    if (s.time !== this._time || s.status !== this._status || s.speed !== this._speed) {
      this.snap = { time: this._time, status: this._status, speed: this._speed };
    }
    for (const l of [...this.listeners]) l();
    for (const l of [...this.frameListeners]) l();
  }
}
