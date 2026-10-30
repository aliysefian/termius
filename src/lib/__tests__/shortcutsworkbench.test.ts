import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null), Channel: class {}, convertFileSrc: (p: string) => p }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));

const { ACTIONS } = await import("$lib/shortcuts");
const { DEFAULT_PREFS } = await import("$lib/stores/settings.svelte");

describe("workbench shortcuts", () => {
  const ids = ["sidebar", "right-panel", "bottom-panel", "shell-focus", "dock-terminal"];

  it("exist, each with its own key", () => {
    const combos = ACTIONS.filter((a) => a.combo).map((a) => a.combo);
    expect(new Set(combos).size).toBe(combos.length);
    for (const id of ids) expect(ACTIONS.find((a) => a.id === id)?.combo, id).toBeTruthy();
  });

  it("never take a plain Ctrl key, which belongs to the remote shell", () => {
    for (const id of ids) {
      const combo = ACTIONS.find((a) => a.id === id)!.combo;
      expect(/Alt|Shift/.test(combo), `${id}: ${combo}`).toBe(true);
    }
  });
});

describe("the icon-first defaults", () => {
  it("start with both side panels away and the rail without labels", () => {
    expect(DEFAULT_PREFS.rightPanelOpen).toBe(false);
    expect(DEFAULT_PREFS.bottomPanelOpen).toBe(false);
    expect(DEFAULT_PREFS.railLabels).toBe(false);
  });
});
