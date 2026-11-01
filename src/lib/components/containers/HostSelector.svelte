<script lang="ts">
  // The one place to choose which computer's containers are shown: the open ones to switch between (or close), and
  // a search to open another. Opening one leaves the others, and every terminal and tunnel, as they were.
  import { Check, ChevronDown, Loader2, Server, X } from "lucide-svelte";
  import Badge from "../Badge.svelte";
  import Combobox from "../Combobox.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import { containers, LOCAL } from "$lib/stores/containers.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo } from "$lib/types";

  let open = $state(false);
  let picker = $state("");
  let root = $state<HTMLDivElement>();

  const current = $derived(containers.activeKey);
  const label = $derived(current ? containers.label(current) : "Choose a host");
  const connecting = $derived(Object.keys(containers.connecting).filter((k) => containers.connecting[k]));
  const options = $derived([{ value: LOCAL, label: "This computer", detail: "docker, podman or nerdctl installed here" }, ...hostOptions(vaultStore.hosts)]);
  const env = (key: string) => (key === LOCAL ? undefined : envInfo(vaultStore.effectiveEnv(vaultStore.hostById.get(key)?.data)));
  const activeEnv = $derived(current ? env(current) : undefined);

  async function choose(key: string) {
    if (!key) return;
    picker = "";
    open = false;
    await containers.switchTo(key);
  }

  function outside(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onpointerdown={outside} onkeydown={(e) => e.key === "Escape" && open && ((open = false), e.stopPropagation())} />

<div class="relative" bind:this={root}>
  <button class="flex h-9 min-w-[12rem] items-center gap-2 rounded-lg border border-line bg-panel px-3 text-left text-sm hover:border-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" aria-haspopup="listbox" aria-expanded={open} aria-label="Host: {label}" onclick={() => (open = !open)}>
    <Server size={15} class="shrink-0 text-fg-muted" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate">{label}</span>
    {#if activeEnv?.short}<Badge tone={activeEnv.tone ?? "neutral"}>{activeEnv.short}</Badge>{/if}
    {#if current && containers.connecting[current]}<Loader2 size={13} class="animate-spin text-fg-muted" aria-label="Connecting" />{/if}
    <ChevronDown size={14} class="shrink-0 text-fg-muted" aria-hidden="true" />
  </button>

  {#if open}
    <div class="anim-pop absolute left-0 top-full z-40 mt-1 w-80 rounded-xl border border-line bg-panel p-2 shadow-xl">
      {#if Object.keys(containers.sources).length}
        <p class="px-2 pb-1 pt-0.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Open</p>
        <ul role="listbox" aria-label="Open hosts">
          {#each Object.values(containers.sources) as s (s.key)}
            {@const e = env(s.key)}
            <li role="option" aria-selected={current === s.key} class="flex items-center gap-1 rounded-md hover:bg-panel-hover">
              <button class="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left text-sm" onclick={() => choose(s.key)}>
                <span class="w-3.5 shrink-0">{#if current === s.key}<Check size={13} class="text-accent" />{/if}</span>
                <span class="min-w-0 flex-1 truncate">{s.label}</span>
                {#if e?.short}<Badge tone={e.tone ?? "neutral"}>{e.short}</Badge>{/if}
                <span class="text-[11px] text-fg-muted">{s.runtime}</span>
              </button>
              <button class="icon-btn mr-1 h-6 w-6" title="Close {s.label}" aria-label="Close {s.label}" onclick={() => void containers.close(s.key)}><X size={12} /></button>
            </li>
          {/each}
        </ul>
      {/if}
      {#each connecting as k (k)}
        <p class="flex items-center gap-2 px-2 py-1.5 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> Connecting to {containers.label(k)}…</p>
      {/each}
      <p class="px-2 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Open another</p>
      <Combobox bind:value={picker} {options} placeholder="Search hosts…" ariaLabel="Open a host" emptyText="No host matches" onchange={choose} />
    </div>
  {/if}
</div>
