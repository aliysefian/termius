<script lang="ts">
  import LogTable from "./LogTable.svelte";
  import LogToolbar from "./LogToolbar.svelte";
  import StreamConfig from "./StreamConfig.svelte";
  import StreamStatus from "./StreamStatus.svelte";
  import { ops } from "$lib/stores/ops.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let searchEl = $state<HTMLInputElement>();

  function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  // Ctrl/Cmd+F searches the logs while Logs is what is on screen (the terminal keeps its own).
  function key(e: KeyboardEvent) {
    if (ui.view !== "ops" || ops.tab !== "logs" || e.defaultPrevented) return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      focusSearch();
    }
  }
</script>

<svelte:window onkeydown={key} />

<div class="flex h-full min-h-0 min-w-0 flex-1 flex-col" data-testid="logs-workspace">
  <StreamConfig />
  <StreamStatus />
  <LogToolbar bind:searchEl />
  <LogTable onfocussearch={focusSearch} />
</div>
