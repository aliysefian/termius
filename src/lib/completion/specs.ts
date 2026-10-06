// The bundled command specs, loaded one command at a time. Only the small index
// (names and one-line descriptions) is part of the main bundle; a spec is
// fetched the first time its command is typed.
import type { Node, Specs } from "./command";
import index from "./specs/index.json";

const loaders = import.meta.glob<Node>("./specs/*.json", { import: "default" });
const loaded = new Map<string, Node>();
const loading = new Map<string, Promise<Node | undefined>>();

const names = Object.entries(index as Record<string, string>);
const known = new Set(names.map(([n]) => n));

/** Start loading a spec; resolves when it can be used. */
export function loadSpec(command: string): Promise<Node | undefined> {
  const have = loaded.get(command);
  if (have) return Promise.resolve(have);
  const load = loaders[`./specs/${command}.json`];
  if (!load || !known.has(command)) return Promise.resolve(undefined);
  let p = loading.get(command);
  if (!p) {
    p = load()
      .then((spec) => {
        loaded.set(command, spec);
        return spec;
      })
      // A chunk that can't be fetched just means no suggestions from it.
      .catch(() => undefined)
      .finally(() => loading.delete(command));
    loading.set(command, p);
  }
  return p;
}

export const specs: Specs = {
  known: (c) => known.has(c),
  get: (c) => loaded.get(c),
  names: () => names,
};
