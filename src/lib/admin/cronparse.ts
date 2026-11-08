// Reading a cron schedule: checking that it is one, and saying it in words. Used by the Cron view now and by its editor
// later. Pure. It follows Vixie cron (what Debian, Ubuntu, RHEL's cronie and most others use): five fields, `*`, lists,
// ranges, steps, month and weekday names, `@macros`, and Sunday as 0 or 7.

export interface Field {
  name: string;
  min: number;
  max: number;
  names?: string[];
}

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const DAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

export const FIELDS: Field[] = [
  { name: "minute", min: 0, max: 59 },
  { name: "hour", min: 0, max: 23 },
  { name: "day of month", min: 1, max: 31 },
  { name: "month", min: 1, max: 12, names: MONTHS },
  { name: "day of week", min: 0, max: 7, names: DAYS },
];

export const MACROS: Record<string, string> = {
  "@reboot": "At every start-up of the host",
  "@yearly": "Once a year, on 1 January at 00:00",
  "@annually": "Once a year, on 1 January at 00:00",
  "@monthly": "Once a month, on the 1st at 00:00",
  "@weekly": "Once a week, on Sunday at 00:00",
  "@daily": "Once a day, at 00:00",
  "@midnight": "Once a day, at 00:00",
  "@hourly": "Once an hour, at minute 0",
};

const MACRO_EXPANDS: Record<string, string> = {
  "@yearly": "0 0 1 1 *",
  "@annually": "0 0 1 1 *",
  "@monthly": "0 0 1 * *",
  "@weekly": "0 0 * * 0",
  "@daily": "0 0 * * *",
  "@midnight": "0 0 * * *",
  "@hourly": "0 * * * *",
};

export type Parsed = { ok: true; macro: string | null; fields: number[][] ; star: boolean[] } | { ok: false; error: string };

function value(token: string, f: Field): number | null {
  const t = token.toLowerCase();
  if (f.names) {
    const at = f.names.indexOf(t.slice(0, 3));
    if (t.length >= 3 && at >= 0 && /^[a-z]+$/.test(t)) return f.name === "month" ? at + 1 : at;
  }
  return /^\d+$/.test(t) ? Number(t) : null;
}

/** The values one field allows, or an error saying what is wrong with it. */
function expandField(text: string, f: Field): number[] | string {
  const out = new Set<number>();
  for (const part of text.split(",")) {
    if (part === "") return `The ${f.name} field has an empty item.`;
    const [range, stepText, ...extra] = part.split("/");
    if (extra.length) return `The ${f.name} field has more than one step in “${part}”.`;
    let step = 1;
    if (stepText !== undefined) {
      if (!/^\d+$/.test(stepText) || Number(stepText) < 1) return `The step in “${part}” must be a whole number from 1.`;
      step = Number(stepText);
    }
    let lo: number | null;
    let hi: number | null;
    if (range === "*") {
      lo = f.min;
      hi = f.max;
    } else if (range.includes("-")) {
      const [a, b, ...more] = range.split("-");
      if (more.length) return `“${range}” is not a range.`;
      lo = value(a, f);
      hi = value(b, f);
    } else {
      lo = value(range, f);
      // `5/10` means "from 5, every 10", to the end of the field.
      hi = stepText !== undefined ? f.max : lo;
    }
    if (lo === null || hi === null) return `“${range}” is not a valid ${f.name}.`;
    if (lo < f.min || hi > f.max) return `The ${f.name} must be from ${f.min} to ${f.max} (“${part}” is outside that).`;
    if (lo > hi) return `“${range}” runs backwards.`;
    for (let v = lo; v <= hi; v += step) out.add(f.name === "day of week" && v === 7 ? 0 : v);
  }
  return [...out].sort((a, b) => a - b);
}

export function parseCron(expr: string): Parsed {
  const text = expr.trim().replace(/\s+/g, " ");
  if (!text) return { ok: false, error: "Enter a schedule." };
  if (text.startsWith("@")) {
    const m = text.toLowerCase();
    if (!(m in MACROS)) return { ok: false, error: `“${text}” is not a schedule macro. Use @reboot, @hourly, @daily, @weekly, @monthly or @yearly.` };
    return { ok: true, macro: m, fields: m === "@reboot" ? [] : parseCronFields(MACRO_EXPANDS[m]), star: [] };
  }
  const parts = text.split(" ");
  if (parts.length !== 5) return { ok: false, error: `A schedule has five fields (minute hour day-of-month month day-of-week); this has ${parts.length}.` };
  const fields: number[][] = [];
  for (let i = 0; i < 5; i++) {
    const r = expandField(parts[i], FIELDS[i]);
    if (typeof r === "string") return { ok: false, error: r };
    fields.push(r);
  }
  return { ok: true, macro: null, fields, star: parts.map((p) => p === "*" || p === "*/1") };
}

function parseCronFields(expr: string): number[][] {
  const r = parseCron(expr);
  return r.ok ? r.fields : [];
}

/** Null when it is a schedule cron will accept, else why not. */
export function validateCron(expr: string): string | null {
  const r = parseCron(expr);
  return r.ok ? null : r.error;
}

const pad = (n: number) => String(n).padStart(2, "0");
const DAY_NAMES = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTH_NAMES = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

const list = (xs: string[]) => (xs.length <= 1 ? (xs[0] ?? "") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);

/** Consecutive runs written as ranges: 1,2,3,5 gives "Monday to Wednesday and Friday". */
function runs(values: number[], name: (n: number) => string): string {
  const parts: string[] = [];
  for (let i = 0; i < values.length; ) {
    let j = i;
    while (j + 1 < values.length && values[j + 1] === values[j] + 1) j++;
    parts.push(j - i >= 2 ? `${name(values[i])} to ${name(values[j])}` : values.slice(i, j + 1).map(name).join(" and "));
    i = j + 1;
  }
  return list(parts);
}

function everyStep(vals: number[], min: number, max: number): number | null {
  if (vals.length < 3 || vals[0] !== min) return null;
  const step = vals[1] - vals[0];
  if (vals.some((v, i) => v !== min + i * step)) return null;
  return vals[vals.length - 1] + step > max ? step : null;
}

/** A schedule in a sentence, or the error that makes it not one. */
export function describeCron(expr: string): string {
  const p = parseCron(expr);
  if (!p.ok) return p.error;
  if (p.macro) return MACROS[p.macro];
  const [min, hour, dom, mon, dow] = p.fields;
  const [sMin, sHour, sDom, sMon, sDow] = p.star;

  let when: string;
  const everyMin = everyStep(min, 0, 59);
  if (sMin && sHour) when = "Every minute";
  else if (everyMin && sHour) when = `Every ${everyMin} minutes`;
  else if (min.length === 1 && sHour) when = min[0] === 0 ? "At the start of every hour" : `At minute ${min[0]} of every hour`;
  else if (min.length === 1 && hour.length === 1) when = `At ${pad(hour[0])}:${pad(min[0])}`;
  else if (min.length === 1 && everyStep(hour, 0, 23)) when = `At minute ${min[0]}, every ${everyStep(hour, 0, 23)} hours`;
  else if (min.length === 1 && !sHour) when = `At ${list(hour.map((h) => `${pad(h)}:${pad(min[0])}`))}`;
  else when = `At minutes ${list(min.map(String))}${sHour ? " of every hour" : ` of hours ${list(hour.map(String))}`}`;

  const days: string[] = [];
  if (!sDom) days.push(`on the ${list(dom.map(ordinal))}`);
  if (!sDow) days.push(`on ${runs(dow, (n) => DAY_NAMES[n])}`);
  // Cron's own rule: when both day fields are limited, a day matching EITHER one runs.
  const dayText = days.length === 2 ? `${days[0]} or ${days[1]}` : (days[0] ?? "");
  const monText = sMon ? "" : `in ${runs(mon, (n) => MONTH_NAMES[n - 1])}`;
  return [when, dayText, monText].filter(Boolean).join(", ").replace(/, in /, " in ").replace(/^(.)/, (c) => c.toUpperCase());
}

function ordinal(n: number): string {
  const s = ["th", "st", "nd", "rd"];
  const v = n % 100;
  return `${n}${s[(v - 20) % 10] ?? s[v] ?? s[0]}`;
}
