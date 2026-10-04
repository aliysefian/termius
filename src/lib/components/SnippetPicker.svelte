<script lang="ts">
  import { ClipboardPaste, Code, Megaphone, Play, Search } from "lucide-svelte";
  import { runSnippet } from "$lib/runsnippet";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let open = $state(false);
  let query = $state("");
  let broadcast = $state(false);

  const list = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return vaultStore.snippets.filter(
      (s) => !q || s.data!.label.toLowerCase().includes(q) || s.data!.command.toLowerCase().includes(q),
    );
  });
  const canBroadcast = $derived((ui.activeTab?.panes.length ?? 0) > 1);

  function run(command: string, execute: boolean) {
    open = false;
    void runSnippet(command, { execute, scope: broadcast && canBroadcast ? "tab" : "pane" });
  }
</script>

<div class="relative">
  <button
    class="icon-btn h-8 w-8 {open ? 'bg-panel-hover text-fg' : ''}"
    title="Run snippet"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <Code size={15} />
  </button>
  {#if open}
    <button class="fixed inset-0 z-30 cursor-default" aria-label="Close snippets" onclick={() => (open = false)}></button>
    <div class="absolute right-0 top-9 z-40 w-80 rounded-lg border border-line bg-panel shadow-2xl">
      <div class="border-b border-line p-2">
        <div class="relative">
          <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
          <!-- svelte-ignore a11y_autofocus -->
          <input class="input py-1.5 pl-8 text-xs" placeholder="Find snippet…" bind:value={query} autofocus />
        </div>
        {#if canBroadcast}
          <label class="mt-2 flex items-center gap-2 px-1 text-xs text-fg-muted">
            <input type="checkbox" bind:checked={broadcast} class="accent-input" />
            <Megaphone size={12} /> Send to every pane in this tab
          </label>
        {/if}
      </div>
      <div class="max-h-72 overflow-y-auto p-1">
        {#each list as s (s.id)}
          {@const d = s.data!}
          <div class="group flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-panel-hover">
            <div class="min-w-0 flex-1">
              <div class="truncate text-sm">{d.label}</div>
              <div class="truncate font-mono text-xs text-fg-muted">{d.command}</div>
            </div>
            <button class="icon-btn h-7 w-7" title="Paste without running" onclick={() => run(d.command, false)}><ClipboardPaste size={13} /></button>
            <button class="icon-btn h-7 w-7 text-accent" title="Run" onclick={() => run(d.command, true)}><Play size={13} /></button>
          </div>
        {:else}
          <p class="px-3 py-6 text-center text-xs text-fg-muted">
            {vaultStore.snippets.length ? "No matches." : "No snippets yet. Add some under Snippets."}
          </p>
        {/each}
      </div>
    </div>
  {/if}
</div>
