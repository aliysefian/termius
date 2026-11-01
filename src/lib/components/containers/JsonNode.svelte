<script lang="ts">
  // One node of a JSON tree: objects and arrays fold, and a value copies on a click. A search opens the branches that
  // contain a match and marks the matching text.
  import { ChevronDown, ChevronRight, Copy } from "lucide-svelte";
  import JsonNode from "./JsonNode.svelte";
  import { jsonMatches, jsonPreview } from "$lib/jsontree";

  let { label, value, depth = 0, query = "", oncopy }: { label: string | null; value: unknown; depth?: number; query?: string; oncopy: (text: string) => void } = $props();

  const isBranch = $derived(value !== null && typeof value === "object");
  const entries = $derived(isBranch ? (Array.isArray(value) ? (value as unknown[]).map((v, i) => [String(i), v] as [string, unknown]) : Object.entries(value as Record<string, unknown>)) : []);
  // svelte-ignore state_referenced_locally
  let userOpen = $state<boolean | null>(null);
  const searching = $derived(query.trim() !== "");
  const hit = $derived(searching && jsonMatches(value, label, query));
  const open = $derived(userOpen ?? (searching ? hit : depth < 2));
  const text = $derived(typeof value === "string" ? value : JSON.stringify(value));

  const mark = (s: string) => {
    const q = query.trim().toLowerCase();
    if (!q) return [{ t: s, h: false }];
    const out: { t: string; h: boolean }[] = [];
    const low = s.toLowerCase();
    let at = 0;
    for (;;) {
      const i = low.indexOf(q, at);
      if (i < 0) break;
      if (i > at) out.push({ t: s.slice(at, i), h: false });
      out.push({ t: s.slice(i, i + q.length), h: true });
      at = i + q.length;
    }
    if (at < s.length) out.push({ t: s.slice(at), h: false });
    return out.length ? out : [{ t: s, h: false }];
  };
</script>

{#if searching && !hit && depth > 0}
  <!-- Not part of what was searched for. -->
{:else}
  <div class="font-mono text-xs leading-5" role={depth === 0 ? "tree" : "group"}>
    <div class="group flex items-start gap-1 rounded px-1 hover:bg-panel-hover" role="treeitem" aria-selected="false" aria-expanded={isBranch ? open : undefined}>
      {#if isBranch}
        <button class="mt-0.5 shrink-0 text-fg-muted hover:text-fg" aria-label="{open ? 'Collapse' : 'Expand'} {label ?? 'root'}" onclick={() => (userOpen = !open)}>
          {#if open}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
        </button>
      {:else}<span class="w-3 shrink-0"></span>{/if}
      {#if label !== null}<span class="shrink-0 text-accent">{#each mark(label) as p, i (i)}{#if p.h}<mark class="rounded-sm bg-warning/40 text-fg">{p.t}</mark>{:else}{p.t}{/if}{/each}:</span>{/if}
      {#if isBranch}
        <span class="text-fg-muted">{jsonPreview(value)}</span>
      {:else}
        <span class="min-w-0 break-all {typeof value === 'string' ? 'text-success' : typeof value === 'number' ? 'text-warning' : 'text-fg-muted'}">
          {#each mark(typeof value === "string" ? JSON.stringify(value) : String(value)) as p, i (i)}{#if p.h}<mark class="rounded-sm bg-warning/40 text-fg">{p.t}</mark>{:else}{p.t}{/if}{/each}
        </span>
      {/if}
      <button class="ml-auto shrink-0 text-fg-muted opacity-0 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100" title="Copy {isBranch ? 'this branch as JSON' : 'the value'}" aria-label="Copy {label ?? 'value'}" onclick={() => oncopy(isBranch ? JSON.stringify(value, null, 2) : text)}><Copy size={11} /></button>
    </div>
    {#if isBranch && open}
      <div class="ml-3 border-l border-line pl-2">
        {#each entries as [k, v] (k)}<JsonNode label={k} value={v} depth={depth + 1} {query} {oncopy} />{/each}
      </div>
    {/if}
  </div>
{/if}
