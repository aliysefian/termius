<script lang="ts">
  import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-svelte";
  import type { Message } from "$lib/stores/databases.svelte";

  let { messages }: { messages: Message[] } = $props();
  const icon = { success: CircleCheck, error: CircleAlert, warning: TriangleAlert, info: Info } as const;
  const tone = { success: "text-success", error: "text-danger", warning: "text-warning", info: "text-fg-muted" } as const;
</script>

{#if messages.length === 0}
  <p class="px-4 py-6 text-center text-xs text-fg-muted">Nothing has run in this tab yet. What the server says, and how long each run took, appears here.</p>
{:else}
  <ul class="divide-y divide-line/60" aria-label="Messages">
    {#each messages as m, i (m.at + ":" + i)}
      {@const Icon = icon[m.kind]}
      <li class="flex gap-2.5 px-3 py-2 text-xs">
        <Icon size={14} class="mt-0.5 shrink-0 {tone[m.kind]}" />
        <div class="min-w-0 flex-1">
          <p class="whitespace-pre-wrap break-words {m.kind === 'error' ? 'font-mono text-danger' : ''}">{m.text}</p>
          {#if m.sql}<p class="mt-0.5 truncate font-mono text-[11px] text-fg-muted" title={m.sql}>{m.sql.replace(/\s+/g, " ")}</p>{/if}
        </div>
        <time class="shrink-0 text-[11px] text-fg-muted" datetime={new Date(m.at).toISOString()}>{new Date(m.at).toLocaleTimeString()}</time>
      </li>
    {/each}
  </ul>
{/if}
