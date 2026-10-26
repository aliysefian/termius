<script lang="ts">
  import { ArrowRightLeft, ChevronLeft, ChevronRight, Columns2, LayoutPanelTop, Plus, Rows2, SquareTerminal, Unplug, X } from "lucide-svelte";
  import { askText } from "$lib/dialogs.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { supportsControlMode, validSessionName, type ScreenSession, type TmuxSession } from "$lib/tmux/discovery";
  import { useRemoteMultiplexer } from "$lib/tmux/useRemoteMultiplexer.svelte";

  let { paneId }: { paneId: string } = $props();

  const mux = useRemoteMultiplexer(() => paneId);
  const key = (s: TmuxSession | ScreenSession) => ("attached" in s ? s.name : s.id);
  const isTmux = (s: TmuxSession | ScreenSession): s is TmuxSession => "attached" in s;

  const probe = $derived(mux.state.probe);
  const tmux = $derived(probe?.tmux.sessions ?? []);
  const screen = $derived(probe?.screen ?? []);
  /** tmux sessions when there are any, else screen's. */
  const kind = $derived<"tmux" | "screen">(tmux.length > 0 ? "tmux" : "screen");
  const sessions = $derived<(TmuxSession | ScreenSession)[]>(kind === "tmux" ? tmux : screen);
  const native = $derived(kind === "tmux" && supportsControlMode(probe?.tmux.version ?? null));
  const visible = $derived(!mux.state.native && !mux.state.dismissed && sessions.length > 0);

  let picked = $state<string | null>(null);
  let hovered = $state<string | null>(null);
  /** Pills shown before "+N more". */
  const MAX_PILLS = 5;

  /** What to attach to: the pill the user picked, else the first session nobody is attached to, else the first. */
  const target = $derived(
    sessions.find((s) => key(s) === picked) ??
      sessions.find((s) => ("attached" in s ? s.attached === 0 : s.state === "detached")) ??
      sessions[0],
  );

  /** `0: zsh (1 win)` for tmux's numbered sessions, which are named after nothing; `deploy (3 wins)` otherwise. */
  function pillLabel(s: TmuxSession | ScreenSession): string {
    if (!isTmux(s)) return s.name;
    const first = s.windowList[0]?.name;
    const label = /^\d+$/.test(s.name) && first ? `${s.name}: ${first}` : s.name;
    return `${label} (${s.windows} ${s.windows === 1 ? "win" : "wins"})`;
  }

  const when = (secs: number) => (secs > 0 ? new Date(secs * 1000).toLocaleString() : "never");

  async function create() {
    const name = await askText(kind === "tmux" ? "Name for the new tmux session (leave empty to let tmux number it)" : "Name for the new screen session (optional)", "", {
      title: "New session",
      confirm: "Create",
    });
    if (name === null) return;
    const trimmed = name.trim();
    if (trimmed && kind === "tmux" && !validSessionName(trimmed)) {
      ui.notify("error", "A tmux session name can't contain '.' or ':'.");
      return;
    }
    if (kind === "tmux" && native) mux.createNative(trimmed);
    else mux.newStandard(kind, trimmed);
  }

  const count = $derived(`${sessions.length} active ${kind} ${sessions.length === 1 ? "session" : "sessions"} found`);
</script>

{#if visible && target}
  <div
    class="relative z-20 flex h-8 shrink-0 items-center justify-between gap-3 border-b border-line bg-panel/60 px-3 text-xs"
    role="region"
    aria-label="Terminal multiplexer sessions on this host"
  >
    <div class="flex min-w-0 items-center gap-2">
      <SquareTerminal size={13} class="shrink-0 text-fg-muted" aria-hidden="true" />
      <span class="shrink-0 whitespace-nowrap text-fg-muted">{count}</span>
      <div class="flex min-w-0 items-center gap-1" role="group" aria-label="Sessions">
        {#each sessions.slice(0, MAX_PILLS) as s (key(s))}
          <div class="relative" role="presentation" onmouseenter={() => (hovered = key(s))} onmouseleave={() => (hovered = null)}>
            <button
              class="max-w-48 truncate rounded-sm border px-2 py-0.5 font-mono tabular-nums text-xs transition-colors
                {key(s) === key(target) ? 'border-accent/60 bg-accent/10 text-fg' : 'border-line text-fg-muted hover:bg-panel-hover hover:text-fg'}"
              aria-pressed={key(s) === key(target)}
              aria-describedby={hovered === key(s) ? `mux-tip-${paneId}` : undefined}
              onclick={() => (picked = key(s))}
              onfocus={() => (hovered = key(s))}
              onblur={() => (hovered = null)}
            >
              {pillLabel(s)}
            </button>
            {#if hovered === key(s)}
              <div
                id="mux-tip-{paneId}"
                role="tooltip"
                class="absolute left-0 top-full z-30 mt-1 w-64 rounded-md border border-line bg-base p-2.5 font-mono text-[11px] text-fg shadow-xs"
              >
                {#if isTmux(s)}
                  <div class="mb-1.5 flex items-center justify-between text-fg-muted">
                    <span>{s.id} · {s.name}</span>
                    <span class="tabular-nums">{s.attached > 0 ? `${s.attached} attached` : "detached"}</span>
                  </div>
                  {#each s.windowList as w (w.index)}
                    <div class="flex items-baseline gap-2 tabular-nums">
                      <span class="text-fg-muted">{w.index}:</span>
                      <span class="min-w-0 flex-1 truncate">{w.name}</span>
                      <span class="shrink-0 text-accent">{w.command}</span>
                    </div>
                  {:else}
                    <div class="text-fg-muted">{s.windows} {s.windows === 1 ? "window" : "windows"}</div>
                  {/each}
                  <div class="mt-1.5 border-t border-line pt-1.5 text-fg-muted">
                    last attached <span class="tabular-nums text-fg">{when(s.lastAttached)}</span>
                  </div>
                {:else}
                  <div class="text-fg-muted">{s.id}</div>
                  <div class="tabular-nums">{s.state}</div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
        {#if sessions.length > MAX_PILLS}
          <span class="shrink-0 text-fg-muted tabular-nums">+{sessions.length - MAX_PILLS} more</span>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 items-center gap-1">
      {#if kind === "tmux"}
        <button
          class="inline-flex h-6 items-center gap-1.5 rounded-sm border border-line px-2 text-fg hover:bg-panel-hover disabled:cursor-not-allowed disabled:opacity-50"
          disabled={!native}
          title={native ? "Open this session in native tabs and splits (tmux -CC)" : "This host's tmux is older than 1.8 and has no native mode"}
          onclick={() => mux.attachNative(key(target))}
        >
          <LayoutPanelTop size={12} aria-hidden="true" /> Attach (Native GUI)
        </button>
      {/if}
      <button
        class="inline-flex h-6 items-center gap-1.5 rounded-sm border border-line px-2 text-fg-muted hover:bg-panel-hover hover:text-fg"
        title={kind === "tmux" ? "Type the attach command into the shell (inside tmux it switches to this session instead)" : "Type screen -r into the shell"}
        onclick={() => mux.attachStandard(kind, key(target))}
      >
        <ArrowRightLeft size={12} aria-hidden="true" /> {kind === "tmux" ? "Attach (Standard CLI)" : "Attach"}
      </button>
      <button class="inline-flex h-6 items-center gap-1.5 rounded-sm border border-line px-2 text-fg-muted hover:bg-panel-hover hover:text-fg" onclick={create}>
        <Plus size={12} aria-hidden="true" /> New Session
      </button>

      {#if !native}
        <!-- No native mode here (screen, or a tmux older than 1.8): one-click versions of the prefix keys. -->
        <span class="mx-1 h-4 w-px bg-line" aria-hidden="true"></span>
        <div class="flex items-center" role="group" aria-label="Send {kind} commands to the shell">
          <button class="icon-btn h-6 w-6" title="New window" aria-label="New window" onclick={() => mux.prefix(kind, "newWindow")}><Plus size={12} /></button>
          <button class="icon-btn h-6 w-6" title="Previous window" aria-label="Previous window" onclick={() => mux.prefix(kind, "prevWindow")}><ChevronLeft size={12} /></button>
          <button class="icon-btn h-6 w-6" title="Next window" aria-label="Next window" onclick={() => mux.prefix(kind, "nextWindow")}><ChevronRight size={12} /></button>
          {#if kind === "tmux"}
            <button class="icon-btn h-6 w-6" title="Split side by side" aria-label="Split side by side" onclick={() => mux.prefix(kind, "splitRight")}><Columns2 size={12} /></button>
            <button class="icon-btn h-6 w-6" title="Split top and bottom" aria-label="Split top and bottom" onclick={() => mux.prefix(kind, "splitDown")}><Rows2 size={12} /></button>
          {/if}
          <button class="icon-btn h-6 w-6" title="Detach" aria-label="Detach" onclick={() => mux.prefix(kind, "detach")}><Unplug size={12} /></button>
        </div>
      {/if}
      <button class="icon-btn h-6 w-6" title="Hide this bar" aria-label="Hide this bar" onclick={mux.dismiss}><X size={12} /></button>
    </div>
  </div>
{/if}
