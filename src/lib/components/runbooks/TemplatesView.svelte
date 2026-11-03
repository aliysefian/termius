<script lang="ts">
  import { Eye, Search, ShieldAlert, ShieldCheck } from "lucide-svelte";
  import Modal from "../Modal.svelte";
  import { searchTemplates, TEMPLATES, type Template } from "$lib/runbooktemplates";
  import { CATEGORIES } from "$lib/runbooklib";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  let query = $state("");
  let category = $state<string | null>(null);
  let preview = $state<Template | null>(null);
  const shown = $derived(searchTemplates(TEMPLATES, query, category));
  const categories = $derived((CATEGORIES as readonly string[]).filter((c) => TEMPLATES.some((t) => t.category === c)));

  async function use(t: Template) {
    if (await rb.newFrom(t.body)) preview = null;
  }
</script>

<div class="flex h-full min-h-0 flex-col" data-testid="templates-view">
  <header class="space-y-3 border-b border-line px-5 py-3">
    <div class="flex flex-wrap items-center gap-3">
      <div>
        <h2 class="text-[15px] font-semibold">Templates</h2>
        <p class="text-xs text-fg-muted">Complete runbooks to start from. Using one makes a copy you can change. Nothing runs until you choose hosts and run it.</p>
      </div>
      <div class="relative ml-auto w-64">
        <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input h-8 w-full pl-8 text-xs" type="search" placeholder="Search templates" aria-label="Search templates" bind:value={query} />
      </div>
    </div>
    <div class="flex flex-wrap gap-1.5" role="group" aria-label="Category">
      <button class="rounded-md border px-2.5 py-0.5 text-xs {category === null ? 'border-accent/50 bg-accent/15 text-accent' : 'border-line text-fg-muted hover:bg-panel-hover'}" aria-pressed={category === null} onclick={() => (category = null)}>All</button>
      {#each categories as c (c)}<button class="rounded-md border px-2.5 py-0.5 text-xs {category === c ? 'border-accent/50 bg-accent/15 text-accent' : 'border-line text-fg-muted hover:bg-panel-hover'}" aria-pressed={category === c} onclick={() => (category = c)}>{c}</button>{/each}
    </div>
  </header>
  <div class="min-h-0 flex-1 overflow-y-auto p-5">
    {#if shown.length === 0}
      <p class="py-10 text-center text-sm text-fg-muted">No template matches.</p>
    {:else}
      <ul class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {#each shown as t (t.id)}
          <li class="flex flex-col rounded-xl border border-line bg-panel p-4" data-testid="template-card">
            <div class="flex items-start gap-2">
              <h3 class="min-w-0 flex-1 text-sm font-semibold">{t.name}</h3>
              <span class="shrink-0 rounded border border-line px-1.5 text-[10px] text-fg-muted">{t.category}</span>
            </div>
            <p class="mt-1.5 flex items-center gap-1.5 text-[11px] {t.effect === 'read-only' ? 'text-success' : 'text-warning'}">
              {#if t.effect === "read-only"}<ShieldCheck size={12} /> Read-only{:else}<ShieldAlert size={12} /> Changes the host{/if}
            </p>
            <p class="mt-1.5 flex-1 text-xs text-fg-muted">{t.description}</p>
            {#if t.variables.length}
              <p class="mt-2 flex flex-wrap gap-1 text-[10px] text-fg-muted" aria-label="Variables">{#each t.variables as v (v.name)}<span class="rounded border border-line px-1.5" title={v.label}>{v.name}{v.required ? " *" : ""}</span>{/each}</p>
            {/if}
            <div class="mt-3 flex items-center gap-2">
              <span class="text-[11px] text-fg-muted">{t.steps} step{t.steps === 1 ? "" : "s"}</span>
              <div class="flex-1"></div>
              <button class="btn-secondary h-7 px-2 text-xs" onclick={() => (preview = t)}><Eye size={12} /> View</button>
              <button class="btn-primary h-7 px-2.5 text-xs" onclick={() => use(t)} data-testid="use-template">Use template</button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

{#if preview}
  {@const t = preview}
  <Modal title={t.name} width="max-w-3xl" onclose={() => (preview = null)}>
    <div class="space-y-3 text-sm">
      <p class="text-xs {t.effect === 'read-only' ? 'text-success' : 'text-warning'}">{t.effect === "read-only" ? "Read-only." : "Changes the host:"} <span class="text-fg-muted">{t.description}</span></p>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre class="max-h-96 overflow-auto rounded-lg border border-line bg-base p-3 font-mono text-xs leading-5" tabindex="0" aria-label="The runbook's document">{t.body}</pre>
    </div>
    {#snippet footer()}
      <button class="btn-secondary" onclick={() => (preview = null)}>Close</button>
      <button class="btn-primary" onclick={() => use(t)}>Use template</button>
    {/snippet}
  </Modal>
{/if}
