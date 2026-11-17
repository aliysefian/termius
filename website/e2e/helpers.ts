import type { Page } from "@playwright/test";

export const B = "/termius";

/** Wait until the tutorial player for `id` has registered itself on the page. */
export async function waitForPlayer(page: Page, id: string) {
  await page.waitForFunction((id) => Boolean(window.__demoPlayers?.[id]), id);
}

export const TUTORIALS = ["create-ssh-connection", "host-management", "terminal-workflow", "tmux-sessions", "sftp-file-transfer", "port-forwarding", "ssh-agent-keys", "run-on-many-hosts"];

interface Probe {
  id: string;
  index: number;
  type: string;
  target: string;
  at: number;
  dx: number;
  dy: number;
  found: boolean;
}

/**
 * Seeks to the moment each pointer step is on its target and measures how far the cursor's tip is from the
 * target element's anchor, in CSS pixels. Runs inside the page so it sees the real layout at this viewport.
 */
export async function measureCursorAlignment(page: Page, id: string): Promise<Probe[]> {
  return page.evaluate(async (id) => {
    const player = window.__demoPlayers![id]!;
    player.pause();
    const raf = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
    const out: Probe[] = [];
    const cursor = document.querySelector<HTMLElement>('[data-testid="sim-cursor"]')!;
    const stage = document.querySelector<HTMLElement>('[data-testid="sim-stage"]')!;
    for (const item of player.timeline.items) {
      const s = item.step;
      if (s.type !== "moveCursor" && s.type !== "click" && s.type !== "doubleClick") continue;
      const at = s.type === "moveCursor" ? item.end - 1 : item.start + 40;
      // Visit the step from its predecessor so the layout is the one a viewer would see.
      player.seek(Math.max(0, item.start - 1));
      await raf();
      player.seek(at);
      await raf();
      await raf();
      const [rawId, anchor] = s.target.split("@");
      const el = stage.querySelector<HTMLElement>(`[data-demo="${CSS.escape(rawId!)}"]`);
      if (!el) {
        out.push({ id, index: item.index, type: s.type, target: s.target, at, dx: Infinity, dy: Infinity, found: false });
        continue;
      }
      const [fx, fy] = anchor ? anchor.split(",").map(Number) : [0.5, 0.5];
      const r = el.getBoundingClientRect();
      const c = cursor.getBoundingClientRect();
      const scale = stage.getBoundingClientRect().width / 1100;
      // The arrow's tip is 2px right and 1.5px down from the cursor box origin, in stage units.
      const tipX = c.left + 2 * scale;
      const tipY = c.top + 1.5 * scale;
      out.push({ id, index: item.index, type: s.type, target: s.target, at, dx: tipX - (r.left + r.width * (fx ?? 0.5)), dy: tipY - (r.top + r.height * (fy ?? 0.5)), found: true });
    }
    return out;
  }, id);
}
