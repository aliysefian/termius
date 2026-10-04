<script lang="ts">
  import Modal from "./Modal.svelte";
  import Kbd from "./Kbd.svelte";
  import { ACTIONS, comboFor, comboOf } from "$lib/shortcuts";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let recording = $state<string | null>(null);

  function onKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recording = null;
      return;
    }
    const combo = comboOf(e);
    if (!combo) return;
    settings.prefs.keybindings[recording] = combo;
    recording = null;
  }

  const fixed: [string, string][] = [
    ["Ctrl+1 … Ctrl+9", "Go to tab"],
    ["Ctrl+PageUp / PageDown", "Previous / next tab"],
    ["Ctrl+= / Ctrl+- / Ctrl+0", "Zoom in / out / reset"],
    ["Ctrl+scroll", "Zoom in / out"],
    ["Drag panel edge", "Resize the list panel (double-click resets)"],
    ["Ctrl+Shift+C / Ctrl+Shift+V", "Copy / paste in a terminal"],
    ["Ctrl+Shift+↑ / ↓", "Previous / next prompt (shell integration)"],
    ["Shift+drag", "Select text while tmux or vim has the mouse"],
  ];
  const changed = $derived(Object.keys(settings.prefs.keybindings).length);
</script>

<svelte:window onkeydowncapture={onKey} />

<Modal title="Keyboard shortcuts" onclose={() => (ui.modal = null)} width="max-w-2xl">
  <p class="mb-3 text-xs text-fg-muted">
    Click a shortcut to change it, then press the new keys (Escape cancels). Plain Ctrl combos reach the remote shell, so
    prefer Ctrl+Shift. On macOS, Cmd works in place of Ctrl.
  </p>
  <div class="grid gap-x-6 gap-y-1 sm:grid-cols-2">
    {#each ACTIONS as a (a.id)}
      {@const combo = comboFor(a)}
      <div class="flex items-center justify-between gap-2 text-xs">
        <span class="text-fg-muted">{a.label}</span>
        <span class="flex items-center gap-1">
          <button
            class="rounded border px-1.5 py-0.5 font-mono text-[11px] {recording === a.id ? 'border-accent bg-accent/15 text-accent' : settings.prefs.keybindings[a.id] !== undefined ? 'border-accent/50 bg-base' : 'border-line bg-base'}"
            onclick={() => (recording = recording === a.id ? null : a.id)}
            title="Change"
          >
            {recording === a.id ? "press keys…" : combo || "unbound"}
          </button>
          {#if settings.prefs.keybindings[a.id] !== undefined}
            <button class="text-[11px] text-fg-muted hover:text-fg" onclick={() => delete settings.prefs.keybindings[a.id]}>reset</button>
          {:else}
            <button class="text-[11px] text-fg-muted hover:text-fg" onclick={() => (settings.prefs.keybindings[a.id] = "")}>unbind</button>
          {/if}
        </span>
      </div>
    {/each}
  </div>
  <h3 class="mt-4 mb-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Fixed</h3>
  <div class="grid gap-x-6 gap-y-1 sm:grid-cols-2">
    {#each fixed as [k, what] (k)}
      <div class="flex items-center justify-between gap-2 text-xs">
        <span class="text-fg-muted">{what}</span>
        <Kbd keys={k} />
      </div>
    {/each}
  </div>
  {#snippet footer()}
    {#if changed}
      <button class="btn-ghost" onclick={() => (settings.prefs.keybindings = {})}>Reset all</button>
    {/if}
    <button class="btn-primary" onclick={() => (ui.modal = null)}>Done</button>
  {/snippet}
</Modal>
