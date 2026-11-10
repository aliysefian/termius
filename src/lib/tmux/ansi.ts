// A captured pane (`tmux capture-pane -e`) as styled text. Only colours and attributes are kept: every other escape
// sequence and control character is dropped, so nothing in a pane can move the cursor, set a title, or put markup on the
// page. The result is data (lines of spans) that a component draws as text nodes, never as HTML.

export interface Span {
  text: string;
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
}

interface Style {
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  inverse?: boolean;
}

/** xterm's 16 colours. */
export const PALETTE16 = ["#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5", "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff"];

function color256(n: number): string | undefined {
  if (n < 0 || n > 255) return undefined;
  if (n < 16) return PALETTE16[n];
  if (n >= 232) {
    const v = 8 + (n - 232) * 10;
    return `rgb(${v},${v},${v})`;
  }
  const i = n - 16;
  const level = (x: number) => (x === 0 ? 0 : 55 + x * 40);
  return `rgb(${level(Math.floor(i / 36))},${level(Math.floor((i % 36) / 6))},${level(i % 6)})`;
}

const byte = (n: number | undefined) => (n !== undefined && n >= 0 && n <= 255 ? n : undefined);

function applySgr(style: Style, params: number[]): Style {
  const s = { ...style };
  const p = params.length ? params : [0];
  for (let i = 0; i < p.length; i++) {
    const n = p[i];
    if (n === 0) {
      for (const k of Object.keys(s)) delete (s as Record<string, unknown>)[k];
    } else if (n === 1) s.bold = true;
    else if (n === 2) s.dim = true;
    else if (n === 3) s.italic = true;
    else if (n === 4) s.underline = true;
    else if (n === 7) s.inverse = true;
    else if (n === 9) s.strike = true;
    else if (n === 22) {
      s.bold = false;
      s.dim = false;
    } else if (n === 23) s.italic = false;
    else if (n === 24) s.underline = false;
    else if (n === 27) s.inverse = false;
    else if (n === 29) s.strike = false;
    else if (n >= 30 && n <= 37) s.fg = PALETTE16[n - 30];
    else if (n >= 90 && n <= 97) s.fg = PALETTE16[n - 90 + 8];
    else if (n >= 40 && n <= 47) s.bg = PALETTE16[n - 40];
    else if (n >= 100 && n <= 107) s.bg = PALETTE16[n - 100 + 8];
    else if (n === 39) s.fg = undefined;
    else if (n === 49) s.bg = undefined;
    else if (n === 38 || n === 48) {
      const target = n === 38 ? "fg" : "bg";
      if (p[i + 1] === 5) {
        const c = byte(p[i + 2]);
        if (c !== undefined) s[target] = color256(c);
        i += 2;
      } else if (p[i + 1] === 2) {
        const [r, g, b] = [byte(p[i + 2]), byte(p[i + 3]), byte(p[i + 4])];
        if (r !== undefined && g !== undefined && b !== undefined) s[target] = `rgb(${r},${g},${b})`;
        i += 4;
      }
    }
  }
  return s;
}

function toSpan(text: string, st: Style): Span {
  const span: Span = { text };
  const [fg, bg] = st.inverse ? [st.bg ?? "var(--ansi-bg, #111)", st.fg ?? "var(--ansi-fg, #ddd)"] : [st.fg, st.bg];
  if (fg) span.fg = fg;
  if (bg) span.bg = bg;
  if (st.bold) span.bold = true;
  if (st.dim) span.dim = true;
  if (st.italic) span.italic = true;
  if (st.underline) span.underline = true;
  if (st.strike) span.strike = true;
  return span;
}

/** Lines of spans. `maxLines` keeps the last lines only. */
export function parseAnsi(input: string, maxLines = 5000): Span[][] {
  const lines: Span[][] = [[]];
  let style: Style = {};
  let buf = "";
  const flush = () => {
    if (buf) {
      const line = lines[lines.length - 1];
      const prev = line[line.length - 1];
      const next = toSpan(buf, style);
      // Merge neighbours that look the same.
      if (prev && prev.fg === next.fg && prev.bg === next.bg && !prev.bold === !next.bold && !prev.dim === !next.dim && !prev.italic === !next.italic && !prev.underline === !next.underline && !prev.strike === !next.strike) prev.text += buf;
      else line.push(next);
      buf = "";
    }
  };
  const n = input.length;
  for (let i = 0; i < n; i++) {
    const ch = input[i];
    if (ch === "\x1b") {
      const next = input[i + 1];
      if (next === "[") {
        // CSI: parameters, intermediates, one final byte.
        let j = i + 2;
        while (j < n && input.charCodeAt(j) >= 0x30 && input.charCodeAt(j) <= 0x3f) j++;
        while (j < n && input.charCodeAt(j) >= 0x20 && input.charCodeAt(j) <= 0x2f) j++;
        const final = input[j];
        if (final === "m") {
          const raw = input.slice(i + 2, j);
          if (!/[^0-9;:]/.test(raw)) {
            flush();
            style = applySgr(style, raw === "" ? [] : raw.split(/[;:]/).map((x) => (x === "" ? 0 : Number(x))));
          }
        }
        i = j;
      } else if (next === "]" || next === "P" || next === "_" || next === "^" || next === "X") {
        // OSC, DCS, APC, PM, SOS: up to BEL or ST, dropped whole.
        let j = i + 2;
        while (j < n && input[j] !== "\x07" && !(input[j] === "\x1b" && input[j + 1] === "\\")) j++;
        i = input[j] === "\x1b" ? j + 1 : j;
      } else if (next !== undefined) {
        // A two-character escape (charset selection takes a third).
        i += "()*+-./".includes(next) ? 2 : 1;
      }
    } else if (ch === "\n") {
      flush();
      lines.push([]);
    } else if (ch === "\t") {
      buf += "    ";
    } else if (ch === "\r") {
      // A carriage return inside a captured line is a rewritten line: the text before it is no longer on screen.
      if (input[i + 1] !== "\n") {
        buf = "";
        lines[lines.length - 1] = [];
      }
    } else if (ch >= " " && ch !== "\x7f" && !(ch >= "\x80" && ch <= "\x9f")) {
      buf += ch;
    }
  }
  flush();
  return lines.length > maxLines ? lines.slice(lines.length - maxLines) : lines;
}

/** The text of the lines with all styling removed. */
export const plainText = (lines: Span[][]): string => lines.map((l) => l.map((s) => s.text).join("")).join("\n");
