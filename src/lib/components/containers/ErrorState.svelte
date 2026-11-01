<script lang="ts">
  import { CircleAlert, Container, Loader2, PlugZap, RefreshCw } from "lucide-svelte";
  import { classifyFailure } from "$lib/containerview";

  let { message, loading = false, onretry, onreconnect }: { message: string; loading?: boolean; onretry: () => void; onreconnect: () => void } = $props();

  const f = $derived(classifyFailure(message));
</script>

<div class="m-auto max-w-md p-8 text-center" role="alert">
  <div class="mx-auto flex h-11 w-11 items-center justify-center rounded-full {f.kind === 'no-runtime' ? 'bg-fg-muted/10 text-fg-muted' : 'bg-danger/10 text-danger'}">
    {#if f.kind === "no-runtime"}<Container size={20} />{:else if f.kind === "disconnected"}<PlugZap size={20} />{:else}<CircleAlert size={20} />{/if}
  </div>
  <h2 class="mt-3 text-[15px] font-semibold">{f.title}</h2>
  <p class="mt-1 text-sm text-fg-muted">{f.text}</p>
  {#if f.kind !== "other" && message !== f.text}
    <details class="mt-3 text-left">
      <summary class="cursor-pointer text-xs text-fg-muted hover:text-fg">What the runtime said</summary>
      <pre class="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-md border border-line bg-base p-2 font-mono text-[11px]">{message}</pre>
    </details>
  {/if}
  <div class="mt-4 flex justify-center gap-2">
    {#if f.actions.includes("retry")}<button class="btn-primary" disabled={loading} onclick={onretry}>{#if loading}<Loader2 size={14} class="animate-spin" />{:else}<RefreshCw size={14} />{/if} Retry</button>{/if}
    {#if f.actions.includes("reconnect")}<button class="{f.actions.includes('retry') ? 'btn-secondary' : 'btn-primary'}" onclick={onreconnect}><PlugZap size={14} /> {f.kind === "disconnected" ? "Reconnect" : "Check connection"}</button>{/if}
  </div>
</div>
