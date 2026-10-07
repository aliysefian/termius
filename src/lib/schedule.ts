// Scheduled runbooks: when one is due. Pure. A schedule only fires while the app is open; a time that passed while
// it was closed (or the computer asleep) is skipped, not caught up, so opening the app never starts a burst of
// old runs.

export type When =
  | { kind: "every"; minutes: number }
  | { kind: "daily"; at: string }
  | { kind: "weekly"; days: number[]; at: string };

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
}

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
  if (!parseTime(w.at)) return "The time is hours and minutes, like 02:30.";
  if (w.kind === "weekly" && (w.days.length === 0 || w.days.some((d) => !Number.isInteger(d) || d < 0 || d > 6))) return "Choose at least one day.";
  return null;
}

/** The most recent time at or before `now` this schedule was meant to run (ms), or null for "every" schedules. */
function lastSlot(w: When, now: number): number | null {
  if (w.kind === "every") return null;
  const t = parseTime(w.at);
  if (!t) return null;
  const at = new Date(now);
  for (let back = 0; back <= 7; back++) {
    const d = new Date(at.getFullYear(), at.getMonth(), at.getDate() - back, t.h, t.m, 0, 0);
    if (d.getTime() > now) continue;
    if (w.kind === "daily" || w.days.includes(d.getDay())) return d.getTime();
  }
  return null;
}

/** Whether to start it now. */
export function isDue(s: Schedule, now: number): boolean {
  if (!s.enabled || problem(s.when)) return false;
  if (s.when.kind === "every") return now - s.lastRun >= s.when.minutes * 60_000;
  const slot = lastSlot(s.when, now);
  return slot !== null && slot > s.lastRun && now - slot <= GRACE_MS;
}

/** The next time it will run (ms), for showing; null when it can't. */
export function nextRun(s: Schedule, now: number): number | null {
  if (problem(s.when)) return null;
  if (s.when.kind === "every") return Math.max(now, s.lastRun + s.when.minutes * 60_000);
  const t = parseTime(s.when.at)!;
  const at = new Date(now);
  for (let ahead = 0; ahead <= 8; ahead++) {
    const d = new Date(at.getFullYear(), at.getMonth(), at.getDate() + ahead, t.h, t.m, 0, 0);
    if (d.getTime() <= now) continue;
    if (s.when.kind === "daily" || s.when.days.includes(d.getDay())) return d.getTime();
  }
  return null;
}

export function describe(w: When): string {
  if (w.kind === "every") return w.minutes % 60 === 0 && w.minutes >= 60 ? `every ${w.minutes / 60 === 1 ? "hour" : `${w.minutes / 60} hours`}` : `every ${w.minutes} minutes`;
  if (w.kind === "daily") return `every day at ${w.at}`;
  return `${w.days.length === 7 ? "every day" : w.days.map((d) => DAYS[d]).join(", ")} at ${w.at}`;
}
