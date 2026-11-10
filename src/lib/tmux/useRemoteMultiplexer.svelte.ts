// Per-pane multiplexer state for the interface: what tmux and screen sessions the host has, whether the pane is
// showing a tmux session natively (control mode), and the actions the bar and the native view call.
//
// The control stream runs in the pane's own shell channel, as in iTerm2: `tmux -CC` is typed at the prompt, tmux
// answers with a DCS and then protocol lines, and from then on every byte of the pane goes through the client
// (see `filter`) instead of the terminal. Detaching returns the shell, with everything still running on the host.
import * as api from "$lib/api";
import { ssh } from "$lib/ssh";
import { ui } from "$lib/stores/ui.svelte";
import { errorMessage } from "$lib/types";
import { TmuxControlClient } from "./TmuxControlClient";
import { DEFAULT_SOCKET } from "./monitor";
import { attachCommand, fromSnapshot, parseProbe, prefixSequence, supportsControlMode, validSessionName, type MultiplexerProbe, type MuxAction } from "./discovery";

export interface PaneMultiplexer {
  /** What the host has, once asked. */
  probe: MultiplexerProbe | null;
  probing: boolean;
  /** The user closed the bar for this pane. */
  dismissed: boolean;
  /** Control mode is being started or is running; the native view shows once the client is `ready`. */
  native: boolean;
  /** Bumped whenever the client's windows or layout change, so views redraw. */
  rev: number;
}

/** How long to wait for tmux to start control mode before deciding it can't. */
const START_TIMEOUT_MS = 6000;
/** How long to wait for a detach to complete before dropping the client anyway. */
const DETACH_TIMEOUT_MS = 3000;

const clients = new Map<string, TmuxControlClient>();

const blank = (): PaneMultiplexer => ({ probe: null, probing: false, dismissed: false, native: false, rev: 0 });

class Multiplexers {
  byPane = $state<Record<string, PaneMultiplexer>>({});

  get(paneId: string): PaneMultiplexer {
    return this.byPane[paneId] ?? blank();
  }

  private ensure(paneId: string): PaneMultiplexer {
    if (!this.byPane[paneId]) this.byPane[paneId] = blank();
    return this.byPane[paneId];
  }

  client(paneId: string): TmuxControlClient | undefined {
    return clients.get(paneId);
  }

  /**
   * Ask the host which sessions it has. tmux's sessions come from the shared engine (`src-tauri/src/tmux`), which reads them
   * safely whatever the names hold, and screen's from the pane's own probe. Quiet: a host that says nothing just gets no bar.
   *
   * A request that arrives while one is running is not dropped (the answer in flight may predate what the caller just
   * did): it runs once more when the first ends. A failed refresh keeps what the bar already shows.
   */
  async refresh(paneId: string) {
    const m = this.ensure(paneId);
    if (m.probing) {
      this.#again.add(paneId);
      return;
    }
    m.probing = true;
    try {
      const hostId = this.#hostOf(paneId);
      const [out, snap] = await Promise.all([
        api.multiplexer.probe(paneId),
        hostId ? api.tmuxMonitor.snapshot(hostId, DEFAULT_SOCKET, 1500).catch(() => null) : Promise.resolve(null),
      ]);
      if (out === null) {
        // Not a connected SSH session: nothing to list, and nothing worth keeping.
        m.probe = null;
      } else {
        const probe = parseProbe(out);
        if (snap && (snap.state.kind === "ready" || snap.state.kind === "no_server" || snap.state.kind === "empty")) probe.tmux = fromSnapshot(snap);
        m.probe = probe;
      }
    } catch {
      // Keep the last list: a connection that blinked is not a host with no sessions.
    } finally {
      m.probing = false;
    }
    if (this.#again.delete(paneId)) await this.refresh(paneId);
  }

  #again = new Set<string>();

  /** The saved host a pane is connected to, when it is one (a quick-connect has none). */
  #hostOf(paneId: string): string | null {
    for (const t of ui.tabs) {
      const p = t.panes.find((x) => x.id === paneId);
      if (p?.target.kind === "host") return p.target.hostId;
    }
    return null;
  }

  /**
   * Bytes from the pane's session. Returns what the terminal should show: all of it normally, none of the
   * protocol while a control session runs.
   */
  filter(paneId: string, bytes: Uint8Array): Uint8Array {
    const c = clients.get(paneId);
    return c ? c.feed(bytes) : bytes;
  }

  /** The SSH session ended or restarted: the control session is gone with it. */
  reset(paneId: string) {
    const c = clients.get(paneId);
    clients.delete(paneId);
    c?.dispose();
    delete this.byPane[paneId];
  }

  dismiss(paneId: string) {
    this.ensure(paneId).dismissed = true;
  }

  private type(paneId: string, command: string) {
    return ssh.write(paneId, command + "\r");
  }

  /** Type a plain command into the shell: the standard, non-native way in. */
  attachStandard(paneId: string, kind: "tmux" | "screen", name: string) {
    void this.type(paneId, kind === "tmux" ? attachCommand.tmuxStandard(name) : attachCommand.screen(name));
  }

  newStandard(paneId: string, kind: "tmux" | "screen", name: string) {
    void this.type(paneId, kind === "tmux" ? attachCommand.newStandard(name) : attachCommand.newScreen(name));
  }

  /** Attach to a session natively, or start one (`name` empty: tmux numbers it). */
  attachNative(paneId: string, how: { attach: string } | { create: string }) {
    const m = this.ensure(paneId);
    if (m.native) return;
    const version = m.probe?.tmux.version ?? null;
    const name = "attach" in how ? how.attach : how.create;
    if (!supportsControlMode(version)) {
      ui.notify("warning", "This host's tmux is too old for native mode (it needs 1.8). Attached the standard way instead.");
      return "attach" in how ? this.attachStandard(paneId, "tmux", name) : this.newStandard(paneId, "tmux", name);
    }
    if ("create" in how && name && !validSessionName(name)) {
      ui.notify("error", "A tmux session name can't contain '.' or ':'.");
      return;
    }
    const client = new TmuxControlClient({ write: (t) => void ssh.write(paneId, t), version });
    clients.set(paneId, client);
    m.native = true;
    client.on((e) => {
      if (e.type === "change") m.rev++;
      else if (e.type === "exit") this.ended(paneId, client);
    });
    // tmux never says it is in control mode if it refuses to start (no such session, a too-old build).
    setTimeout(() => {
      if (clients.get(paneId) === client && !client.parser.active && !client.ready) {
        ui.notify("error", "tmux didn't start control mode. Check the terminal for its message.");
        this.ended(paneId, client);
      }
    }, START_TIMEOUT_MS);
    void this.type(paneId, "attach" in how ? attachCommand.tmuxNative(name) : attachCommand.newNative(name));
  }

  private ended(paneId: string, client: TmuxControlClient) {
    if (clients.get(paneId) !== client) return;
    clients.delete(paneId);
    client.dispose();
    const m = this.byPane[paneId];
    if (m) m.native = false;
    // The sessions changed while we were in it.
    void this.refresh(paneId);
  }

  /** Leave the native view; every process keeps running on the host. */
  detach(paneId: string) {
    const c = clients.get(paneId);
    if (!c) return;
    c.detach();
    setTimeout(() => this.ended(paneId, c), DETACH_TIMEOUT_MS);
  }

  /** The quick toolbar's one-key versions of tmux and screen commands, typed into the shell. */
  prefix(paneId: string, kind: "tmux" | "screen", action: MuxAction) {
    const keys = prefixSequence(kind, action);
    if (keys) void ssh.write(paneId, keys).catch((e) => ui.notify("error", errorMessage(e)));
  }
}

export const multiplexers = new Multiplexers();

/** The multiplexer state of one pane, with its actions bound. Call during component setup. */
export function useRemoteMultiplexer(paneId: () => string) {
  return {
    get state() {
      return multiplexers.get(paneId());
    },
    get client() {
      // Reading `native` makes this re-evaluate when a control session starts or ends.
      return multiplexers.get(paneId()).native ? multiplexers.client(paneId()) : undefined;
    },
    refresh: () => multiplexers.refresh(paneId()),
    dismiss: () => multiplexers.dismiss(paneId()),
    attachNative: (name: string) => multiplexers.attachNative(paneId(), { attach: name }),
    createNative: (name: string) => multiplexers.attachNative(paneId(), { create: name }),
    attachStandard: (kind: "tmux" | "screen", name: string) => multiplexers.attachStandard(paneId(), kind, name),
    newStandard: (kind: "tmux" | "screen", name: string) => multiplexers.newStandard(paneId(), kind, name),
    detach: () => multiplexers.detach(paneId()),
    prefix: (kind: "tmux" | "screen", action: MuxAction) => multiplexers.prefix(paneId(), kind, action),
  };
}
