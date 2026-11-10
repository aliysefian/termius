<script lang="ts">
  import { Settings2 } from "lucide-svelte";
  import { tmuxMonitor as mon } from "$lib/stores/tmuxmonitor.svelte";
  import { PREVIEW_LIMITS, clampPrefs } from "$lib/tmux/monitor";
  import type { Uuid } from "$lib/types";

  /** Given when opened from a terminal: the host that panel is about. */
  let { hostId = null }: { hostId?: Uuid | null } = $props();

  let open = $state(false);
  const num = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
  const setPreview = (patch: Partial<typeof mon.prefs.preview>) => (mon.prefs.preview = clampPrefs({ ...mon.prefs.preview, ...patch }));
</script>

<div class="relative">
  <button class="grid h-6 w-6 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" title="Tmux settings" aria-label="Tmux settings" aria-expanded={open} onclick={() => (open = !open)}><Settings2 size={13} /></button>
  {#if open}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close settings" tabindex="-1" onclick={() => (open = false)}></button>
    <div class="absolute right-0 top-7 z-50 w-72 space-y-3 rounded-xl border border-line bg-panel p-3 text-xs shadow-xl" role="dialog" aria-label="Tmux settings">
      <fieldset class="space-y-1.5">
        <legend class="mb-1 font-semibold text-fg">Panel beside terminals</legend>
        <label class="flex items-center gap-2"><input type="checkbox" bind:checked={mon.prefs.panel.enabled} /> Show the tmux panel in terminals</label>
        <label class="flex items-center gap-2"><input type="checkbox" bind:checked={mon.prefs.panel.autoOpen} disabled={!mon.prefs.panel.enabled} /> Open it when a host has tmux sessions</label>
        {#if hostId}
          <label class="flex items-center gap-2"><input type="checkbox" checked={mon.panelEnabledFor(hostId)} disabled={!mon.prefs.panel.enabled} onchange={(e) => mon.setPanelHost(hostId, e.currentTarget.checked)} /> Enabled for this host</label>
        {/if}
        <div class="flex items-center gap-2">
          <span class="text-fg-muted">Side</span>
          <select class="rounded border border-line bg-base px-1 py-0.5" bind:value={mon.prefs.panel.side}><option value="right">Right</option><option value="left">Left</option></select>
          <span class="ml-2 text-fg-muted">Width</span>
          <input class="w-16 rounded border border-line bg-base px-1 py-0.5" type="number" min="200" max="560" step="10" value={mon.prefs.panel.width} onchange={(e) => (mon.prefs.panel.width = Math.min(560, Math.max(200, num(e) || 280)))} />
        </div>
      </fieldset>
      <fieldset class="space-y-1.5">
        <legend class="mb-1 font-semibold text-fg">Refresh</legend>
        <label class="flex items-center justify-between gap-2">Session list, every <span class="flex items-center gap-1"><input class="w-14 rounded border border-line bg-base px-1 py-0.5" type="number" min="3" max="60" value={mon.prefs.discoverMs / 1000} onchange={(e) => { mon.prefs.discoverMs = Math.min(60, Math.max(3, num(e) || 4)) * 1000; }} /> s</span></label>
        <label class="flex items-center justify-between gap-2">Selected pane, every <span class="flex items-center gap-1"><input class="w-14 rounded border border-line bg-base px-1 py-0.5" type="number" min={PREVIEW_LIMITS.selectedMs[0] / 1000} max={PREVIEW_LIMITS.selectedMs[1] / 1000} value={mon.prefs.preview.selectedMs / 1000} onchange={(e) => setPreview({ selectedMs: num(e) * 1000 })} /> s</span></label>
        <label class="flex items-center justify-between gap-2">Other panes, every <span class="flex items-center gap-1"><input class="w-14 rounded border border-line bg-base px-1 py-0.5" type="number" min={PREVIEW_LIMITS.thumbnailMs[0] / 1000} max={PREVIEW_LIMITS.thumbnailMs[1] / 1000} value={mon.prefs.preview.thumbnailMs / 1000} onchange={(e) => setPreview({ thumbnailMs: num(e) * 1000 })} /> s</span></label>
        <label class="flex items-center justify-between gap-2">Panes drawn at once <input class="w-14 rounded border border-line bg-base px-1 py-0.5" type="number" min={PREVIEW_LIMITS.maxThumbnails[0]} max={PREVIEW_LIMITS.maxThumbnails[1]} value={mon.prefs.preview.maxThumbnails} onchange={(e) => setPreview({ maxThumbnails: num(e) })} /></label>
        <p class="text-[11px] text-fg-muted">Only what is on screen is read, and nothing while this page is hidden. Pane text is never saved.</p>
      </fieldset>
    </div>
  {/if}
</div>
