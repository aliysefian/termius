<script lang="ts">
  import { onMount } from "svelte";
  import { Loader2, Trash2 } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { pruneProjects, summarizePrune } from "$lib/containerdata";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type PruneItem, type PruneKind, type PruneResult } from "$lib/types";

  let { sourceKey, what }: { sourceKey: string; what: "images" | "volumes" | "networks" } = $props();

  // Images can be limited to untagged ones, or widened to every one nothing uses.
  let allImages = $state(false);
  let items = $state<PruneItem[] | null>(null);
  let selected = $state<Set<string>>(new Set());
  let loading = $state(true);
  let error = $state<string | null>(null);
  let phase = $state<"choose" | "running" | "done">("choose");
  let results = $state<PruneResult[]>([]);

  const kind = $derived<PruneKind>(what === "images" ? { kind: "images", all: allImages } : what === "volumes" ? { kind: "volumes" } : { kind: "networks" });
  const noun = $derived({ images: "image", volumes: "volume", networks: "network" }[what]);
  const picked = $derived((items ?? []).filter((i) => selected.has(i.id)));
  const projects = $derived(pruneProjects(picked));
  const summary = $derived(summarizePrune(results));
  const allChecked = $derived(!!items?.length && picked.length === items.length);

  async function load() {
    const src = containers.sources[sourceKey];
    if (!src) {
      error = "This source was closed.";
      loading = false;
      return;
    }
    loading = true;
    error = null;
    try {
      items = await api.containers.pruneItems(src.sessionId, src.runtime, kind);
      // Volumes hold data: nothing is ticked until someone ticks it. The rest start with all of them.
      selected = new Set(what === "volumes" ? [] : items.map((i) => i.id));
    } catch (e) {
      error = errorMessage(e);
      items = null;
    } finally {
      loading = false;
    }
  }

  onMount(load);

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  function toggleAll() {
    selected = allChecked ? new Set() : new Set((items ?? []).map((i) => i.id));
  }

  async function run() {
    const src = containers.sources[sourceKey];
    if (!src || picked.length === 0) return;
    if (!(await containers.confirmPrune(sourceKey, picked.length === 1 ? noun : `${noun}s`, picked.length))) return;
    phase = "running";
    try {
      results = await api.containers.pruneRun(src.sessionId, src.runtime, kind, picked.map((i) => i.id));
    } catch (e) {
      error = errorMessage(e);
      results = [];
    }
    phase = "done";
    void containers.refresh(sourceKey);
  }
</script>

<Modal title="Remove unused {what}" onclose={() => (ui.modal = null)} width="max-w-2xl">
  {#if phase === "done"}
    <div class="space-y-3">
      <p class="rounded-md border px-3 py-2 text-sm {summary.failed.length ? 'border-warning/40 bg-warning/10 text-warning' : 'border-success/30 bg-success/10 text-success'}" role="status">
        Removed {summary.removed} of {results.length} {results.length === 1 ? noun : `${noun}s`}.
      </p>
      {#if error}<p class="whitespace-pre-wrap text-sm text-danger" role="alert">{error}</p>{/if}
      {#if summary.failed.length}
        <p class="text-xs text-fg-muted">These were left alone:</p>
        <ul class="max-h-56 space-y-1 overflow-auto text-xs">
          {#each summary.failed as r (r.id)}
            <li class="rounded border border-line px-2 py-1"><span class="font-mono">{r.id}</span> <span class="text-fg-muted">· {r.error}</span></li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <div class="space-y-3">
      <p class="text-sm text-fg-muted">
        {#if what === "images"}
          {allImages ? "Every image no container, running or stopped, uses." : "Untagged images no container uses."}
        {:else if what === "volumes"}
          Volumes no container, running or stopped, mounts. <strong class="text-warning">The data in a removed volume is gone for good.</strong>
        {:else}
          Networks no container is attached to. The built-in <code>bridge</code>, <code>host</code> and <code>none</code> are never listed.
        {/if}
        Nothing is removed until you press the button, and only what is ticked below.
      </p>
      {#if what === "images"}
        <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" bind:checked={allImages} onchange={load} disabled={phase === "running"} /> Also tagged images that no container uses</label>
      {/if}

      {#if loading}
        <div class="flex items-center gap-2 py-8 text-sm text-fg-muted"><Loader2 size={15} class="animate-spin" /> Checking what is unused…</div>
      {:else if error && !items}
        <p class="whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error}</p>
      {:else if items && items.length === 0}
        <p class="py-6 text-center text-sm text-fg-muted">Nothing is unused.</p>
      {:else if items}
        <div class="flex items-center justify-between text-xs text-fg-muted">
          <label class="flex items-center gap-2"><input type="checkbox" class="accent-input" checked={allChecked} onchange={toggleAll} aria-label="Select all" /> {picked.length} of {items.length} selected</label>
          {#if what === "volumes" && picked.length === 0}<span>Tick the ones you are sure about.</span>{/if}
        </div>
        <ul class="max-h-80 overflow-auto rounded-md border border-line" aria-label="Unused {what}">
          {#each items as it (it.id)}
            <li class="flex items-center gap-2 border-b border-line/60 px-3 py-1.5 last:border-b-0 hover:bg-panel-hover/50">
              <input type="checkbox" class="accent-input" checked={selected.has(it.id)} onchange={() => toggle(it.id)} aria-label="Remove {it.label}" disabled={phase === "running"} />
              <span class="min-w-0 flex-1 truncate font-mono text-xs" title={it.id}>{it.label}</span>
              {#if it.project}<Badge tone="accent" title="Compose project">{it.project}</Badge>{/if}
              <span class="shrink-0 text-xs text-fg-muted">{it.detail}</span>
            </li>
          {/each}
        </ul>
        {#if projects.length}
          <p class="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
            Includes things that belong to Compose {projects.length === 1 ? "project" : "projects"} {projects.join(", ")}. They are unused now, but a project that is brought up again would have to create them anew.
          </p>
        {/if}
      {/if}
    </div>
  {/if}
  {#snippet footer()}
    {#if phase === "done"}
      <button class="btn-primary" onclick={() => (ui.modal = null)}>Done</button>
    {:else}
      <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-danger border border-danger/40" onclick={run} disabled={picked.length === 0 || phase === "running" || loading}>
        {#if phase === "running"}<Loader2 size={14} class="animate-spin" /> Removing…{:else}<Trash2 size={14} /> Remove {picked.length || ""} {picked.length === 1 ? noun : `${noun}s`}{/if}
      </button>
    {/if}
  {/snippet}
</Modal>
