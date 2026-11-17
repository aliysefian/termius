import { useEffect, useState } from "react";
import type { TutorialDefinition } from "@/engine/types";
import { loadTutorial } from "@/tutorials/registry";

const cache = new Map<string, TutorialDefinition>();

/** Loads a tutorial's definition on demand. Returns null while loading. */
export function useTutorialDef(id: string): { def: TutorialDefinition | null; error: boolean } {
  const [state, setState] = useState<{ id: string; def: TutorialDefinition | null; error: boolean }>(() => ({ id, def: cache.get(id) ?? null, error: false }));
  useEffect(() => {
    let live = true;
    const hit = cache.get(id);
    if (hit) {
      setState({ id, def: hit, error: false });
      return;
    }
    loadTutorial(id)
      .then((def) => {
        cache.set(id, def);
        if (live) setState({ id, def, error: false });
      })
      .catch(() => live && setState({ id, def: null, error: true }));
    return () => {
      live = false;
    };
  }, [id]);
  return state.id === id ? { def: state.def, error: state.error } : { def: cache.get(id) ?? null, error: false };
}
