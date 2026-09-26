<script lang="ts">
  import { Play } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import { runSnippet } from "$lib/runsnippet";
  import { ui, type SnippetRunOpts } from "$lib/stores/ui.svelte";
  import { lastValues } from "$lib/snippetvalues";

  let { command, names, opts }: { command: string; names: string[]; opts: SnippetRunOpts } = $props();

  // svelte-ignore state_referenced_locally
  let values = $state<Record<string, string>>(Object.fromEntries(names.map((n) => [n, lastValues.get(n) ?? ""])));

  function submit(e: SubmitEvent) {
    e.preventDefault();
    for (const [k, v] of Object.entries(values)) lastValues.set(k, v);
    ui.modal = null;
    void runSnippet(command, opts, $state.snapshot(values));
  }
</script>

<Modal title="Snippet values" onclose={() => (ui.modal = null)} width="max-w-md">
  <form id="vars-form" onsubmit={submit} class="space-y-3">
    <pre class="max-h-24 overflow-auto rounded-md bg-base px-3 py-2 font-mono text-xs text-fg-muted">{command}</pre>
    {#each names as name, i (name)}
      <div>
        <label class="label" for="var-{name}">{name}</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="var-{name}" class="input font-mono" bind:value={values[name]} autofocus={i === 0} spellcheck="false" />
      </div>
    {/each}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="vars-form"><Play size={14} /> {opts.execute ? "Run" : "Paste"}</button>
  {/snippet}
</Modal>
