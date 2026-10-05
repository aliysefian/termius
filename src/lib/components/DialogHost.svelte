<script lang="ts">
  import { TriangleAlert } from "lucide-svelte";
  import { dialogs } from "$lib/dialogs.svelte";
  import { pushLayer, trapTab } from "$lib/layers";

  const p = $derived(dialogs.pending);
  let value = $state("");
  let typed = $state("");
  let checked = $state(false);
  let box = $state<HTMLDivElement>();
  let input = $state<HTMLInputElement>();
  let typedInput = $state<HTMLInputElement>();
  let cancelBtn = $state<HTMLButtonElement>();
  let okBtn = $state<HTMLButtonElement>();

  // Each new dialog starts fresh; dangerous ones focus Cancel so a stray
  // Enter never deletes anything.
  $effect(() => {
    if (!p) return;
    value = p.value;
    typed = "";
    checked = false;
    queueMicrotask(() => {
      if (p.kind === "text") input?.select();
      else if (p.requireText) typedInput?.focus();
      else if (p.danger) cancelBtn?.focus();
      else okBtn?.focus();
    });
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!p || (p.requireText && typed.trim() !== p.requireText)) return;
    dialogs.close(p.kind === "text" ? value : true, checked);
  }

  function cancel() {
    dialogs.close(p?.kind === "text" ? null : false, false);
  }

  // Escape reaches this dialog only, never the form under it; focus comes
  // back to whatever had it when the dialog closes.
  $effect(() => {
    if (!p) return;
    const opener = document.activeElement as HTMLElement | null;
    const unlayer = pushLayer(cancel);
    return () => {
      unlayer();
      if (opener && document.contains(opener)) opener.focus();
    };
  });
</script>

{#if p}
  <div class="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 p-4" role="presentation" onclick={(e) => e.target === e.currentTarget && cancel()}>
    <div bind:this={box} class="w-full max-w-md outline-none" role="alertdialog" aria-modal="true" aria-labelledby="dlg-title" tabindex="-1" onkeydown={(e) => box && trapTab(e, box)}>
    <form class="rounded-xl border {p.danger ? 'border-danger/40' : 'border-line'} bg-panel p-5 shadow-2xl" onsubmit={submit}>
      <h2 id="dlg-title" class="flex items-center gap-2 text-sm font-semibold {p.danger ? 'text-danger' : ''}">
        {#if p.danger}<TriangleAlert size={16} />{/if}
        {p.title}
      </h2>
      {#if p.message}
        <p class="mt-2 text-sm whitespace-pre-line text-fg-muted">{p.message}</p>
      {/if}
      {#if p.kind === "text"}
        <input bind:this={input} class="input mt-3" bind:value placeholder={p.placeholder} spellcheck="false" aria-label={p.title} />
      {/if}
      {#if p.requireText}
        <label class="mt-3 block text-xs text-fg-muted" for="dlg-typed">Type <strong class="font-mono text-fg">{p.requireText}</strong> to continue</label>
        <input id="dlg-typed" bind:this={typedInput} class="input mt-1 font-mono" bind:value={typed} spellcheck="false" autocomplete="off" />
      {/if}
      {#if p.checkbox}
        <label class="mt-3 flex items-center gap-2 text-xs text-fg-muted">
          <input type="checkbox" class="accent-input" bind:checked /> {p.checkbox}
        </label>
      {/if}
      <div class="mt-5 flex justify-end gap-2">
        <button bind:this={cancelBtn} type="button" class="btn-secondary" onclick={cancel}>Cancel</button>
        <button
          bind:this={okBtn}
          type="submit"
          class={p.danger ? "btn-danger border border-danger/40" : "btn-primary"}
          disabled={(p.kind === "text" && !value.trim()) || (!!p.requireText && typed.trim() !== p.requireText)}
        >
          {p.confirm}
        </button>
      </div>
    </form>
    </div>
  </div>
{/if}
