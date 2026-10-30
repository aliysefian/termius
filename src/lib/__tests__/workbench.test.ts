// The workbench's layout state: Shell focus puts every panel away and brings back exactly what was showing, and a
// terminal living in the bottom panel is kept (never closed) when the panel is hidden.
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null), Channel: class {}, convertFileSrc: (p: string) => p }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));

const { ui } = await import("$lib/stores/ui.svelte");
const { settings } = await import("$lib/stores/settings.svelte");

function reset() {
  const p = settings.prefs;
  p.sidebarHidden = false;
  p.rightPanelOpen = false;
  p.bottomPanelOpen = false;
  p.shellFocus = false;
  p.focusRestore = { left: true, right: false, bottom: false };
  ui.tabs = [];
  ui.dockTerminals = [];
  ui.activeDockTerminal = null;
  ui.view = "hosts";
}

describe("Shell focus", () => {
  beforeEach(reset);

  it("hides every panel, then restores exactly what was showing", () => {
    const p = settings.prefs;
    p.rightPanelOpen = true;
    p.bottomPanelOpen = false;
    ui.toggleShellFocus();
    expect(p.shellFocus).toBe(true);
    expect([p.sidebarHidden, p.rightPanelOpen, p.bottomPanelOpen]).toEqual([true, false, false]);
    ui.toggleShellFocus();
    expect(p.shellFocus).toBe(false);
    // The bottom panel was not showing before, so it stays away: focus mode does not force everything open.
    expect([p.sidebarHidden, p.rightPanelOpen, p.bottomPanelOpen]).toEqual([false, true, false]);
  });

  it("keeps the panel sizes and tabs while focused", () => {
    const p = settings.prefs;
    p.rightPanelOpen = true;
    p.rightPanelWidth = 410;
    p.rightPanelTab = "processes";
    p.bottomPanelHeight = 300;
    ui.toggleShellFocus();
    ui.toggleShellFocus();
    expect([p.rightPanelWidth, p.rightPanelTab, p.bottomPanelHeight]).toEqual([410, "processes", 300]);
  });

  it("is left by opening a panel on purpose", () => {
    ui.toggleShellFocus();
    ui.toggleRightPanel();
    expect(settings.prefs.shellFocus).toBe(false);
    expect(settings.prefs.rightPanelOpen).toBe(true);
  });

  it("each panel toggles on its own", () => {
    ui.toggleBottomPanel();
    expect([settings.prefs.rightPanelOpen, settings.prefs.bottomPanelOpen]).toEqual([false, true]);
    ui.toggleRightPanel();
    ui.toggleBottomPanel();
    expect([settings.prefs.rightPanelOpen, settings.prefs.bottomPanelOpen]).toEqual([true, false]);
  });

  it("brings a page back to the terminals when a panel is opened from it", () => {
    ui.view = "settings";
    ui.toggleBottomPanel();
    expect(ui.view).toBe("hosts");
  });
});

describe("terminals in the bottom panel", () => {
  beforeEach(reset);

  it("are not closed when the panel is hidden or Shell focus is on", () => {
    ui.openLocal();
    ui.addDockTerminal();
    expect(ui.dockTerminals).toHaveLength(1);
    expect(settings.prefs.bottomPanelOpen).toBe(true);
    const id = ui.dockTerminals[0].pane.id;
    ui.toggleBottomPanel();
    ui.toggleShellFocus();
    expect(ui.dockTerminals.map((d) => d.pane.id)).toEqual([id]);
  });

  it("open a second shell beside the first, and closing one leaves the other", () => {
    ui.openLocal();
    ui.addDockTerminal();
    ui.addDockTerminal();
    const [a, b] = ui.dockTerminals.map((d) => d.pane.id);
    expect(ui.activeDockTerminal).toBe(b);
    ui.closeDockTerminal(b);
    expect(ui.dockTerminals.map((d) => d.pane.id)).toEqual([a]);
    expect(ui.activeDockTerminal).toBe(a);
  });

  it("never repeat the startup command the tab was opened with", () => {
    ui.openLocal("htop");
    ui.addDockTerminal();
    const t = ui.dockTerminals[0].pane.target;
    expect(t.kind === "local" && t.command).toBeFalsy();
  });
});
