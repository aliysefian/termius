<script lang="ts">
  // One tmux pane as a terminal of its own. tmux decides its size; what it prints arrives through `route`, and
  // what is typed goes back as `send-keys`. Scrolling and selecting are xterm's, so the wheel and a drag work
  // without tmux's copy mode. Scrolling past the top asks tmux for older lines.
  import { onDestroy, onMount } from "svelte";
  import { Terminal, type ITerminalOptions } from "@xterm/xterm";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { X } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { TmuxControlClient } from "$lib/tmux/TmuxControlClient";

  let {
    client,
    paneId,
    cols,
    rows,
    options,
    focused,
    visible,
    route,
    onfocus,
    onclose,
  }: {
    client: TmuxControlClient;
    paneId: number;
    cols: number;
    rows: number;
    options: ITerminalOptions;
    /** This pane has tmux's focus (and the keyboard, if the view is on screen). */
    focused: boolean;
    /** Its window is the one on screen. */
    visible: boolean;
    /** Where the view delivers this pane's output. */
    route: Map<number, (data: Uint8Array) => void>;
    onfocus: () => void;
    onclose: () => void;
  } = $props();

  let host: HTMLDivElement;
  let term: Terminal | undefined;

  /** Lines of history asked for so far; each ask doubles it, up to the cap. */
  let history = 0;
  const HISTORY_FIRST = 1000;
  const HISTORY_MAX = 64_000;
  let exhausted = false;
  let loading = false;
  /**
   * While a snapshot is on its way, output is dropped (it is in the snapshot) until the reply is read, then
   * held (it is not) until the snapshot is drawn.
   */
  let phase: "live" | "drop" | "hold" = "drop";
  let held: Uint8Array[] = [];

  function handleOutput(data: Uint8Array) {
    if (phase === "live") term?.write(data);
    else if (phase === "hold") held.push(data);
  }

  /** Draw the pane as it is now, with `lines` of history before it. Keeps the scroll position when history grows. */
  async function restore(lines: number) {
    if (!term) return;
    phase = "drop";
    held = [];
    try {
      const before = term.buffer.active.length;
      const snap = await client.snapshotPane(paneId, lines, () => (phase = "hold"));
      if (!term) return;
      term.reset();
      term.write(snap.lines.join("\r\n"));
      if (snap.alternateLines) term.write("\x1b[?1049h\x1b[H" + snap.alternateLines.join("\r\n"));
      term.write(`\x1b[${snap.cursor.y + 1};${snap.cursor.x + 1}H`);
      // Capture holds a screenful of blank rows when the pane is mostly empty; fewer lines than asked for means
      // that is all the history there is.
      if (lines > 0 && snap.lines.length < lines + term.rows) exhausted = true;
      if (lines > 0 && !snap.alternateLines) term.write("", () => term?.scrollToLine(Math.max(0, term.buffer.active.length - before)));
    } catch {
      // The pane went away while asking.
    } finally {
      phase = "live";
      for (const d of held) term?.write(d);
      held = [];
    }
  }

  async function olderLines() {
    if (loading || exhausted || phase !== "live" || !term || term.buffer.active.type !== "normal") return;
    loading = true;
    history = Math.min(HISTORY_MAX, history === 0 ? HISTORY_FIRST : history * 2);
    if (history >= HISTORY_MAX) exhausted = true;
    await restore(history);
    loading = false;
  }

  function wheel(e: WheelEvent) {
    // At the top of what is held locally, scrolling up fetches more from tmux.
    if (e.deltaY < 0 && term && term.buffer.active.viewportY === 0) void olderLines();
  }

  const bytes = (s: string) => Uint8Array.from(s, (c) => c.charCodeAt(0));

  onMount(() => {
    term = new Terminal({ ...options, cols, rows, allowProposedApi: true, macOptionIsMeta: true });
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(
      new WebLinksAddon((e, uri) => {
        if (e.ctrlKey || e.metaKey) void openUrl(uri);
      }),
    );
    term.open(host);
    term.onData((d) => client.sendKeys(paneId, d));
    term.onBinary((d) => client.sendKeys(paneId, bytes(d)));
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey || !e.shiftKey) return true;
      if (e.code === "KeyC") {
        const text = term?.getSelection();
        if (text) void writeText(text);
        return false;
      }
      if (e.code === "KeyV") {
        // Otherwise the webview also fires a native paste event, which xterm pastes a second time.
        e.preventDefault();
        void readText().then((t) => t && term?.paste(t));
        return false;
      }
      return true;
    });
    term.textarea?.addEventListener("focus", onfocus);
    host.addEventListener("wheel", wheel, { passive: true });
    route.set(paneId, handleOutput);
    void restore(0);
  });

  onDestroy(() => {
    route.delete(paneId);
    host?.removeEventListener("wheel", wheel);
    term?.dispose();
    term = undefined;
  });

  $effect(() => {
    if (!term) return;
    if (term.cols !== cols || term.rows !== rows) term.resize(cols, rows);
  });

  $effect(() => {
    if (!term) return;
    for (const [k, v] of Object.entries(options)) (term.options as Record<string, unknown>)[k] = v;
  });

  $effect(() => {
    if (visible && term) {
      term.refresh(0, term.rows - 1);
      if (focused) term.focus();
    }
  });
</script>

<div class="group relative h-full w-full overflow-hidden" role="presentation">
  <div bind:this={host} class="h-full w-full"></div>
  <!-- The pane's size and a close button, only while the pointer is over it. -->
  <div class="pointer-events-none absolute right-1.5 top-1 flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
    <span class="rounded-sm border border-line bg-base/90 px-1.5 py-0.5 font-mono text-xs tabular-nums text-fg-muted">{cols}×{rows}</span>
    <button
      class="pointer-events-auto flex h-5 w-5 items-center justify-center rounded-sm border border-line bg-base/90 text-fg-muted hover:text-danger"
      title="Close pane (kill-pane)"
      aria-label="Close pane"
      onclick={onclose}
    >
      <X size={12} />
    </button>
  </div>
</div>
