import { useEffect, useMemo, useSyncExternalStore } from "react";
import { Player, type PlayerOptions } from "@/engine/player";
import { Timeline } from "@/engine/timeline";
import type { TutorialDefinition } from "@/engine/types";

export function useTimeline(def: TutorialDefinition | null): Timeline | null {
  return useMemo(() => (def ? new Timeline(def) : null), [def]);
}

/** One Player per timeline; its clock is cancelled when the component unmounts. */
export function usePlayer(timeline: Timeline | null, options: PlayerOptions = {}): Player | null {
  const { speed, loop, range, startAt } = options;
  const loopHold = loop?.hold;
  const from = range?.[0];
  const to = range?.[1];
  const player = useMemo(
    () => (timeline ? new Player(timeline, { speed, loop: loopHold === undefined ? undefined : { hold: loopHold }, range: from === undefined || to === undefined ? undefined : [from, to], startAt }) : null),
    // speed and startAt only seed a new player; changing them later goes through setSpeed()/seek().
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [timeline, loopHold, from, to],
  );
  useEffect(() => {
    return () => player?.destroy();
  }, [player]);
  return player;
}

/** Re-renders only when the visible state changes (a step boundary, a typed character, a line of output). */
export function useDemoState(player: Player | null) {
  const key = useSyncExternalStore(
    player ? player.subscribe : () => () => {},
    () => (player ? player.timeline.keyAt(player.time) : ""),
  );
  return useMemo(() => {
    if (!player) return null;
    return player.timeline.stateAt(player.time);
    // `key` is the memo key on purpose: the state is identical until it changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [player, key]);
}

/** The time and status, re-rendering on every frame. Use only in small leaf components (the progress bar). */
export function usePlayerSnapshot(player: Player | null) {
  return useSyncExternalStore(
    player ? player.subscribe : () => () => {},
    player ? player.getSnapshot : () => EMPTY,
  );
}
const EMPTY = { time: 0, status: "idle" as const, speed: 1 };

/** Subscribe to a primitive derived from the player; re-renders only when that value changes. */
export function usePlayerSelect<T extends string | number | boolean | null>(player: Player | null, select: (p: Player) => T, fallback: T): T {
  return useSyncExternalStore(
    player ? player.subscribe : () => () => {},
    () => (player ? select(player) : fallback),
  );
}
