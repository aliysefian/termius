// The terminal's metrics: which host a terminal in front belongs to, whether its toolbar and side panel are on, and the
// readings that feed them. It does not collect anything itself: it subscribes to the shared host-samples store (one timer
// per host, one connection per host, shared with every other view), and lets go the moment the host is off, the terminal
// is not the active one, or the vault locks. Opening or closing the panel only changes what is drawn beside the terminal;
// the SSH session is never touched.
import { PAGE_VIEWS, ui } from "$lib/stores/ui.svelte";
import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
import { hostSamples } from "$lib/stores/hostsamples.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { PANEL_DEFAULT, clampWidth, loadPrefs, metricsHost, savePrefs, wantedInterval, type HostChoice, type TerminalPrefs } from "$lib/terminalmetrics";
import { untrack } from "svelte";

class TerminalMetrics {
  prefs = $state<TerminalPrefs>(loadPrefs());

  /** The saved SSH host of the focused pane, when this computer can read it unattended; else null. */
  hostId = $derived.by(() => {
    const tab = ui.activeTab;
    const pane = tab?.panes.find((p) => p.id === tab.activePaneId);
    const id = metricsHost(pane?.target, vaultStore.hostById);
    return id && vaultStore.unlocked && hostMetrics.canMonitor(id) ? id : null;
  });

  /** Whether the terminal itself is what is on screen (not Settings, the Dashboard or another page). */
  terminalVisible = $derived(!PAGE_VIEWS.includes(ui.view) && ui.view !== "sftp");

  choice = $derived<HostChoice | undefined>(this.hostId ? this.prefs.hosts[this.hostId] : undefined);
  toolbarOn = $derived(!!this.choice?.enabled);
  /** The panel is open for the focused host and the terminal is on screen. */
  panelShown = $derived(!!this.choice?.panel && this.terminalVisible);
  every = $derived(wantedInterval(this.choice, this.panelShown, this.prefs.live));

  #sub: ReturnType<typeof hostSamples.subscribe> | null = null;

  constructor() {
    $effect.root(() => {
      $effect(() => savePrefs($state.snapshot(this.prefs)));
      // One subscription for the focused host while it is on; dropped when it changes or turns off.
      $effect(() => {
        const id = this.hostId;
        if (!id || !this.toolbarOn) return;
        const sub = untrack(() => hostSamples.subscribe(id, this.every ?? 0));
        this.#sub = sub;
        return () => {
          sub();
          this.#sub = null;
        };
      });
      $effect(() => {
        // Read it first: `#sub?.set(this.every)` would not look at `every` at all while there is no subscription yet,
        // and the effect would never learn that it changed.
        const ms = this.every ?? 0;
        this.#sub?.set(ms);
      });
    });
  }

  #set(id: string, c: HostChoice | null) {
    const hosts = { ...this.prefs.hosts };
    if (c) hosts[id] = c;
    else delete hosts[id];
    this.prefs.hosts = hosts;
  }

  /** Open or close the panel of the focused host. Opening turns the toolbar on as well. */
  togglePanel() {
    const id = this.hostId;
    if (!id) return;
    const open = !this.prefs.hosts[id]?.panel;
    this.#set(id, { enabled: true, panel: open });
  }

  closePanel() {
    const id = this.hostId;
    if (id && this.prefs.hosts[id]) this.#set(id, { enabled: true, panel: false });
  }

  /** Stop reading the focused host for the terminal: no toolbar, no panel. */
  turnOff() {
    const id = this.hostId;
    if (id) this.#set(id, null);
  }

  setSide(side: TerminalPrefs["side"]) {
    this.prefs.side = side;
  }

  setWidth(w: number) {
    this.prefs.width = clampWidth(w, typeof window === "undefined" ? 1600 : window.innerWidth);
  }

  resetWidth() {
    this.prefs.width = PANEL_DEFAULT;
  }

  setLive(ms: TerminalPrefs["live"]) {
    this.prefs.live = ms;
  }
}

export const terminalMetrics = new TerminalMetrics();
