import { useCallback, useEffect, useRef, type RefObject } from "react";
import type { Player } from "@/engine/player";
import { useInView } from "./useInView";
import { usePrefersReducedMotion } from "./usePrefersReducedMotion";

/**
 * Plays only while the demo is on screen and the tab is visible, and never when the visitor asked for reduced
 * motion (they get the poster frame and manual controls) or has paused it themselves.
 */
export function useAutoplay(player: Player | null, ref: RefObject<Element | null>, enabled: boolean) {
  const reduced = usePrefersReducedMotion();
  const inView = useInView(ref, 0.1);
  const userPaused = useRef(false);

  // Reduced motion: rest on the poster frame.
  useEffect(() => {
    if (!player || !reduced) return;
    player.pause();
    player.seek(Math.min(player.timeline.posterTime, player.range[1]));
  }, [player, reduced]);

  useEffect(() => {
    if (!player || !enabled || reduced) return;
    const sync = () => {
      const visible = inView && document.visibilityState === "visible";
      if (visible && !userPaused.current) player.play();
      else if (!visible) player.pause();
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => {
      document.removeEventListener("visibilitychange", sync);
      player.pause();
    };
  }, [player, enabled, reduced, inView]);

  const onUserAction = useCallback((a: "pause" | "play") => {
    userPaused.current = a === "pause";
  }, []);
  return { reduced, onUserAction };
}
