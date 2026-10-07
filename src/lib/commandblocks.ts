// Command blocks: each command the shell reported (OSC 133) as a stretch of lines with an outcome. Pure helpers;
// the terminal pane keeps the live markers and draws the bar in the margin.

export interface Block {
  id: number;
  command: string;
  exit: number | null;
  durationMs: number;
  pinned: boolean;
  /** First line of the block (the command line) and last line of its output, in buffer lines. */
  start: number;
  end: number;
}

export const MAX_BLOCKS = 200;

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s < 10 ? s.toFixed(1) : Math.round(s)} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${Math.round(s - m * 60)} s`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

export const failed = (b: Pick<Block, "exit">) => b.exit !== null && b.exit !== 0;

/** The line the bar gets as a tooltip. */
export function summary(b: Block): string {
  const outcome = b.exit === null ? "finished" : b.exit === 0 ? "succeeded" : `failed (exit ${b.exit})`;
  return `${b.command.split("\n")[0].slice(0, 80)} · ${outcome} · ${formatDuration(b.durationMs)}`;
}

/** The block that covers a buffer line, if any. */
export function blockAt(blocks: Block[], line: number): Block | null {
  for (let i = blocks.length - 1; i >= 0; i--) if (line >= blocks[i].start && line <= blocks[i].end) return blocks[i];
  return null;
}

/** The nearest failed block before (dir -1) or after (dir 1) a line. Pinned blocks count when `only` is "pinned". */
export function nextMatching(blocks: Block[], line: number, dir: -1 | 1, only: "failed" | "pinned" = "failed"): Block | null {
  const ok = (b: Block) => (only === "failed" ? failed(b) : b.pinned);
  let best: Block | null = null;
  for (const b of blocks) {
    if (!ok(b)) continue;
    if (dir === -1 && b.start < line && (!best || b.start > best.start)) best = b;
    if (dir === 1 && b.start > line && (!best || b.start < best.start)) best = b;
  }
  return best;
}

/** Blocks that touch the visible lines. */
export const visibleBlocks = <T extends Block>(blocks: T[], top: number, rows: number): T[] => blocks.filter((b) => b.end >= top && b.start < top + rows);

/** Drop the oldest unpinned blocks past the limit. */
export function trim<T extends Block>(blocks: T[], max = MAX_BLOCKS): T[] {
  let extra = blocks.length - max;
  if (extra <= 0) return blocks;
  return blocks.filter((b) => (b.pinned || extra-- <= 0));
}
