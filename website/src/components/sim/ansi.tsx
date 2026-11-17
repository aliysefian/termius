import type { ReactNode } from "react";

const COLORS: Record<number, string> = {
  30: "#6b7a90",
  31: "#f5707f",
  32: "#2dd59a",
  33: "#f5b942",
  34: "#7cb4ff",
  35: "#c4a8ff",
  36: "#5fd6e8",
  37: "#e8edf5",
  90: "#6b7a90",
};

/** Colour helpers so demo scripts stay readable: c.g("ok"), c.r("failed"). */
const wrap = (code: number) => (s: string) => `\u001b[${code}m${s}\u001b[0m`;
export const c = {
  gray: wrap(90),
  r: wrap(31),
  g: wrap(32),
  y: wrap(33),
  b: wrap(34),
  m: wrap(35),
  cy: wrap(36),
  bold: wrap(1),
};

/** Renders a line with a minimal subset of SGR colour codes. Never executes or interprets anything else. */
export function renderAnsi(line: string): ReactNode {
  // eslint-disable-next-line no-control-regex
  const parts = line.split(/\u001b\[(\d+)m/);
  const out: ReactNode[] = [];
  let color: string | undefined;
  let bold = false;
  for (let i = 0; i < parts.length; i++) {
    if (i % 2 === 1) {
      const code = Number(parts[i]);
      if (code === 0) {
        color = undefined;
        bold = false;
      } else if (code === 1) bold = true;
      else if (COLORS[code]) color = COLORS[code];
    } else if (parts[i]) {
      out.push(
        <span key={i} style={{ color, fontWeight: bold ? 700 : undefined }}>
          {parts[i]}
        </span>,
      );
    }
  }
  return out;
}

export function stripAnsi(line: string): string {
  // eslint-disable-next-line no-control-regex
  return line.replace(/\u001b\[\d+m/g, "");
}
