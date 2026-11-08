<script lang="ts">
  import { usageTone } from "$lib/hostview";

  let { label, pct, stale = false }: { label: string; pct: number | null; stale?: boolean } = $props();
  const tone = $derived(usageTone(pct));
  const colour = $derived(tone === "bad" ? "bg-danger" : tone === "warn" ? "bg-warning" : "bg-accent");
</script>

<span class="flex items-center gap-1 {stale ? 'opacity-50' : ''}">
  <span class="text-fg-muted">{label}</span>
  <span class="inline-block h-1.5 w-8 overflow-hidden rounded-full bg-line/70" aria-hidden="true">{#if pct !== null}<span class="block h-full rounded-full {colour}" style="width: {Math.max(0, Math.min(100, pct))}%"></span>{/if}</span>
  <span class="w-7 text-right tabular-nums text-fg">{pct === null ? "—" : `${Math.round(pct)}%`}</span>
</span>
