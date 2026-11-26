<script lang="ts">
  import { AlertTriangle, CheckCircle2, Loader2, RotateCw } from "lucide-svelte";
  import { infra } from "$lib/stores/infra.svelte";

  let open = $state(false);
  const total = $derived(infra.hostIds.length);
  const read = $derived(infra.coverage.read);
  const failed = $derived(infra.failures.length);
  const pct = $derived(total ? Math.round((read / total) * 100) : 0);
  const reasons = $derived(infra.failures.map((f) => ({ ...f, label: infra.hostLabel(f.hostId) })));
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") open = false; }} />

<div class="relative">
  <button
    class="inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-xs {failed ? 'border-warning/50 text-warning' : 'border-line text-fg-muted'} hover:bg-panel-hover"
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => (open = !open)}
    data-testid="infra-discovery"
  >
    {#if infra.discovering}<Loader2 size={13} class="animate-spin" />{:else if failed}<AlertTriangle size={13} />{:else}<CheckCircle2 size={13} />{/if}
    <span class="tabular-nums">{read}/{total}</span> hosts{#if failed}<span class="font-medium">· {failed} failed</span>{/if}
  </button>
  {#if open}
    <button class="fixed inset-0 z-30 cursor-default" aria-label="Close discovery details" tabindex="-1" onclick={() => (open = false)}></button>
    <div class="absolute right-0 top-full z-40 mt-1.5 w-[22rem] max-w-[90vw] rounded-lg border border-line bg-panel p-3 text-xs shadow-2xl" role="dialog" aria-label="Discovery status">
      <h3 class="text-sm font-semibold">Discovery status</h3>
      <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-base" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={read} aria-label="Hosts read">
        <div class="h-full bg-accent" style="width: {pct}%"></div>
      </div>
      <p class="mt-1.5 tabular-nums">{read} / {total} hosts scanned</p>
      <ul class="mt-2 space-y-0.5">
        <li class="flex items-center gap-1.5 text-success"><CheckCircle2 size={12} />{Math.max(0, read - failed)} successful</li>
        <li class="flex items-center gap-1.5 {failed ? 'text-warning' : 'text-fg-muted'}"><AlertTriangle size={12} />{failed} failed</li>
        {#if infra.skipped}<li class="text-fg-muted">{infra.skipped} skipped or not supported (no shell, or no systemd)</li>{/if}
        <li class="text-fg-muted">{Math.max(0, total - read - failed)} not read yet</li>
      </ul>
      {#if reasons.length}
        <div class="mt-2 max-h-48 space-y-1.5 overflow-auto rounded-md bg-base p-2">
          {#each reasons as f (f.hostId)}
            <div><div class="font-medium">{f.label}</div>{#each f.errors as e (e)}<div class="break-words text-fg-muted">{e}</div>{/each}</div>
          {/each}
        </div>
      {/if}
      <p class="mt-2 text-[11px] text-fg-muted">What was read stays shown; a failed host keeps its last known state, marked stale.</p>
      <div class="mt-2 flex gap-2">
        <button class="btn-secondary py-1 text-xs" disabled={infra.discovering || !failed} onclick={() => infra.retryFailed()}><RotateCw size={12} /> Retry failed</button>
        <button class="btn-secondary py-1 text-xs" onclick={() => (open = false)}>Close</button>
      </div>
    </div>
  {/if}
</div>
