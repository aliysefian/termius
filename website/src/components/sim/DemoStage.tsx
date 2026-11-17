import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Player } from "@/engine/player";
import { targetAnchor, targetId } from "@/engine/state";
import { useDemoState } from "@/hooks/usePlayer";
import { ClickIndicator } from "./ClickIndicator";
import { SimApp, STAGE_H, STAGE_W } from "./SimApp";
import { SimulatedCursor } from "./SimulatedCursor";

/**
 * Renders the simulated app for a player and drives the cursor.
 * The app is laid out at a fixed logical size and scaled with a transform, so every target keeps the same
 * normalised position at any viewport width and the cursor can never drift off its target.
 */
export function DemoStage({ player }: { player: Player }) {
  const state = useDemoState(player);
  const outer = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const cursor = useRef<HTMLDivElement>(null);
  const known = useRef(new Map<string, { x: number; y: number }>());
  const [scale, setScale] = useState(1);
  const rippleBox = useRef<HTMLDivElement>(null);
  const [ripple, setRipple] = useState<{ key: number; double: boolean } | null>(null);
  const lastClick = useRef(-1);

  useLayoutEffect(() => {
    const el = outer.current;
    if (!el) return;
    const apply = () => setScale(el.clientWidth / STAGE_W);
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  /** Normalised centre (or anchor) of a target inside the stage, remembering the last place it was seen. */
  const resolve = useCallback((target: string) => {
    const root = stage.current;
    if (!root) return known.current.get(target) ?? null;
    const el = root.querySelector<HTMLElement>(`[data-demo="${CSS.escape(targetId(target))}"]`);
    if (el) {
      const r = el.getBoundingClientRect();
      const s = root.getBoundingClientRect();
      if (r.width > 0 && s.width > 0) {
        const { fx, fy } = targetAnchor(target);
        const pos = { x: (r.left + r.width * fx - s.left) / s.width, y: (r.top + r.height * fy - s.top) / s.height };
        known.current.set(target, pos);
        return pos;
      }
    }
    return known.current.get(target) ?? null;
  }, []);

  const update = useCallback(() => {
    const c = cursor.current;
    if (!c) return;
    const f = player.timeline.cursorAt(player.time, resolve);
    c.style.opacity = f.visible ? "1" : "0";
    c.style.transform = `translate3d(${(f.x * STAGE_W).toFixed(1)}px, ${(f.y * STAGE_H).toFixed(1)}px, 0)`;
    // The ring rides with the pointer, so it stays on the target even when the layout settles after a seek.
    if (rippleBox.current) rippleBox.current.style.transform = `translate3d(${(f.x * STAGE_W).toFixed(1)}px, ${(f.y * STAGE_H).toFixed(1)}px, 0)`;
    const svg = c.firstElementChild as SVGElement | null;
    if (svg) svg.style.transform = f.pressed ? "scale(0.82)" : "scale(1)";
    if (f.clickIndex !== lastClick.current) {
      lastClick.current = f.clickIndex;
      if (f.clickIndex >= 0 && f.clickAge < 600 && f.visible) setRipple({ key: f.clickIndex * 1000 + Math.floor(player.time), double: f.clickDouble });
      else setRipple(null);
    } else if (f.clickAge > 700) {
      setRipple((r) => (r ? null : r));
    }
  }, [player, resolve]);

  useEffect(() => {
    update();
    return player.onFrame(update);
  }, [player, update]);
  // Re-resolve after every commit so a pointer never rests on a stale layout.
  useLayoutEffect(update, [state, scale, update]);

  if (!state) return null;
  return (
    <div ref={outer} className="relative w-full overflow-hidden" style={{ aspectRatio: `${STAGE_W} / ${STAGE_H}` }}>
      <div ref={stage} className="absolute left-0 top-0 origin-top-left" style={{ width: STAGE_W, height: STAGE_H, transform: `scale(${scale})` }} data-testid="sim-stage" data-sim-view={state.view} data-sim-panel={state.panel ?? ""}>
        <SimApp state={state} />
        <div ref={rippleBox} aria-hidden="true" className="pointer-events-none absolute left-0 top-0 z-40">
          {ripple && <ClickIndicator key={ripple.key} double={ripple.double} />}
        </div>
        <SimulatedCursor ref={cursor} />
      </div>
    </div>
  );
}
