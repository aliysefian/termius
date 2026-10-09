// Scheduled runbooks: when one is due. Pure. A schedule only fires while the app is open (there is no background
// service). A time that passed while it was closed is, by default, skipped rather than caught up, so opening the app
// never starts a burst of old runs; a schedule can instead ask to run once for the time it missed.

export type When =
  | { kind: "every"; minutes: number }
  | { kind: "daily"; at: string }
  | { kind: "weekly"; days: number[]; at: string }
  /** One time only, an instant (ms since the epoch). The schedule turns itself off after it starts. */
  | { kind: "once"; at: number };

export interface Schedule {
  id: string;
  runbookId: string;
  params: Record<string, string>;
  hostIds: string[];
  when: When;
  enabled: boolean;
  /** Without this, a run that includes a production host is skipped (and said so). */
  allowProduction: boolean;
  /** When it last started (ms since the epoch), or when it was made, so "every 30 minutes" counts from then. */
  lastRun: number;
  /** The time zone the clock times (daily, weekly) are read in, like "Europe/Berlin". Missing: this computer's. */
  tz?: string;
  /** What to do with a time that passed while the app was closed: skip it (default) or run once now. */
  missed?: "skip" | "run_once";
  /** Run the hosts that failed again, up to this many times (0 to 3), `retryMinutes` apart. The timer lives in the app: closing it ends the retries. */
  retries?: number;
  retryMinutes?: number;
  /** On a host where a run fails, also run the runbook's rollback steps. Off unless chosen when the schedule was made. */
  rollback?: boolean;
}

export const MAX_RETRIES = 3;
export const DEFAULT_RETRY_MINUTES = 15;

/** A missed time older than this is skipped. */
export const GRACE_MS = 10 * 60 * 1000;
export const MIN_EVERY = 5;
export const MAX_EVERY = 7 * 24 * 60;
export const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export function parseTime(at: string): { h: number; m: number } | null {
  const m = /^([01]?\d|2[0-3]):([0-5]\d)$/.exec(at.trim());
  return m ? { h: Number(m[1]), m: Number(m[2]) } : null;
}

/** What is wrong with a schedule's timing, or null. */
export function problem(w: When): string | null {
  if (w.kind === "every") return Number.isInteger(w.minutes) && w.minutes >= MIN_EVERY && w.minutes <= MAX_EVERY ? null : `Every ${MIN_EVERY} minutes to 7 days (${MAX_EVERY} minutes).`;
  if (w.kind === "once") return Number.isFinite(w.at) && w.at > 0 ? null : "Choose the date and time.";
  if (!parseTime(w.at)) return "The time is hours and minutes, like 02:30.";
  if (w.kind === "weekly" && (w.days.length === 0 || w.days.some((d) => !Number.isInteger(d) || d < 0 || d > 6))) return "Choose at least one day.";
  return null;
}

/** Whether `tz` names a time zone this system knows. */
export function validZone(tz: string): boolean {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

/** What is wrong with a schedule as a whole, or null. */
export function scheduleProblem(s: Schedule): string | null {
  if (s.tz && !validZone(s.tz)) return `"${s.tz}" is not a time zone this computer knows.`;
  if (s.retries !== undefined && (!Number.isInteger(s.retries) || s.retries < 0 || s.retries > MAX_RETRIES)) return `Retries: 0 to ${MAX_RETRIES}.`;
  if (s.retryMinutes !== undefined && (!Number.isInteger(s.retryMinutes) || s.retryMinutes < 1 || s.retryMinutes > 24 * 60)) return "Minutes between retries: 1 to 1440.";
  return problem(s.when);
}

// -- clock times in a time zone -----------------------------------------------------------------------------

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

interface Wall {
  y: number;
  mo: number;
  d: number;
  h: number;
  mi: number;
  wd: number;
}

/** What the clock and calendar read at instant `ms` in `tz` (this computer's zone when absent). */
function wall(ms: number, tz?: string): Wall {
  if (!tz) {
    const d = new Date(ms);
    return { y: d.getFullYear(), mo: d.getMonth() + 1, d: d.getDate(), h: d.getHours(), mi: d.getMinutes(), wd: d.getDay() };
  }
  const f = new Intl.DateTimeFormat("en-US", { timeZone: tz, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", weekday: "short" });
  const p: Record<string, string> = {};
  for (const part of f.formatToParts(new Date(ms))) p[part.type] = part.value;
  return { y: Number(p.year), mo: Number(p.month), d: Number(p.day), h: Number(p.hour) % 24, mi: Number(p.minute), wd: WEEKDAYS.indexOf(p.weekday) };
}

/**
 * The instant at which the clock in `tz` reads the given date and time. A time that doesn't exist (the hour skipped
 * when clocks go forward) lands just after the gap; one that happens twice (clocks going back) is the first.
 */
function instant(y: number, mo: number, d: number, h: number, mi: number, tz?: string): number {
  if (!tz) return new Date(y, mo - 1, d, h, mi, 0, 0).getTime();
  const want = Date.UTC(y, mo - 1, d, h, mi);
  let guess = want;
  for (let i = 0; i < 4; i++) {
    const w = wall(guess, tz);
    const diff = Date.UTC(w.y, w.mo - 1, w.d, w.h, w.mi) - want;
    if (diff === 0) break;
    guess -= diff;
  }
  return guess;
}

/** The calendar day `back` days away (negative: ahead) from `w`, with its weekday. */
function dayOffset(w: Wall, back: number): { y: number; mo: number; d: number; wd: number } {
  const t = new Date(Date.UTC(w.y, w.mo - 1, w.d - back));
  return { y: t.getUTCFullYear(), mo: t.getUTCMonth() + 1, d: t.getUTCDate(), wd: t.getUTCDay() };
}

/** The most recent time at or before `now` this schedule was meant to run (ms), or null for "every" schedules. */
function lastSlot(w: When, now: number, tz?: string): number | null {
  if (w.kind === "every") return null;
  if (w.kind === "once") return w.at <= now ? w.at : null;
  const t = parseTime(w.at);
  if (!t) return null;
  const here = wall(now, tz);
  for (let back = 0; back <= 8; back++) {
    const day = dayOffset(here, back);
    const slot = instant(day.y, day.mo, day.d, t.h, t.m, tz);
    if (slot > now) continue;
    if (w.kind === "daily" || w.days.includes(day.wd)) return slot;
  }
  return null;
}

/** Whether to start it now. */
export function isDue(s: Schedule, now: number): boolean {
  if (!s.enabled || scheduleProblem(s)) return false;
  if (s.when.kind === "every") return now - s.lastRun >= s.when.minutes * 60_000;
  const slot = lastSlot(s.when, now, s.tz);
  if (slot === null || slot <= s.lastRun) return false;
  // A time that passed while the app was closed: only a schedule that asked for it runs for it.
  return s.missed === "run_once" || now - slot <= GRACE_MS;
}

/** The next time it will run (ms), for showing; null when it can't. */
export function nextRun(s: Schedule, now: number): number | null {
  if (scheduleProblem(s)) return null;
  if (s.when.kind === "every") return Math.max(now, s.lastRun + s.when.minutes * 60_000);
  if (s.when.kind === "once") return s.enabled && s.when.at > s.lastRun ? Math.max(now, s.when.at) : null;
  const t = parseTime(s.when.at)!;
  const here = wall(now, s.tz);
  for (let ahead = 0; ahead <= 8; ahead++) {
    const day = dayOffset(here, -ahead);
    const slot = instant(day.y, day.mo, day.d, t.h, t.m, s.tz);
    if (slot <= now) continue;
    if (s.when.kind === "daily" || s.when.days.includes(day.wd)) return slot;
  }
  return null;
}

/** After a run in which `failed` hosts failed: which to try again and when, or null for no retry. `attempt` counts the retries made so far. */
export function retryPlan(s: Schedule, attempt: number, failed: string[], now: number): { hostIds: string[]; at: number } | null {
  if (!failed.length || attempt >= Math.min(s.retries ?? 0, MAX_RETRIES)) return null;
  return { hostIds: failed, at: now + (s.retryMinutes ?? DEFAULT_RETRY_MINUTES) * 60_000 };
}

export function describe(w: When, tz?: string): string {
  const zone = tz ? ` (${tz})` : "";
  if (w.kind === "every") return w.minutes % 60 === 0 && w.minutes >= 60 ? `every ${w.minutes / 60 === 1 ? "hour" : `${w.minutes / 60} hours`}` : `every ${w.minutes} minutes`;
  if (w.kind === "once") return `once, on ${new Date(w.at).toLocaleString()}`;
  if (w.kind === "daily") return `every day at ${w.at}${zone}`;
  return `${w.days.length === 7 ? "every day" : w.days.map((d) => DAYS[d]).join(", ")} at ${w.at}${zone}`;
}
