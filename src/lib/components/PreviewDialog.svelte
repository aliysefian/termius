<script lang="ts">
  import Modal from "./Modal.svelte";
  import { formatBytes, type FileEntry } from "$lib/types";
  import type { Preview } from "$lib/sftp";
  import Spinner from "./Spinner.svelte";
  import { isMarkdownName, renderMarkdown } from "$lib/markdown";

  let { entry, preview, onclose }: { entry: FileEntry; preview: Preview | null; onclose: () => void } = $props();
  const image = $derived(/\.(png|jpe?g|gif|webp|bmp|svg|ico)$/i.test(entry.name));
  const markdown = $derived(isMarkdownName(entry.name) && preview?.kind === "text" && preview.text != null);
  let showSource = $state(false);
  const mime = $derived(
    entry.name.toLowerCase().endsWith(".svg") ? "image/svg+xml" : `image/${entry.name.split(".").pop()?.toLowerCase().replace("jpg", "jpeg")}`,
  );
</script>

<Modal title={entry.name} onclose={onclose} width="max-w-3xl">
  <div class="mb-2 flex items-center justify-between gap-2">
    <p class="text-xs text-fg-muted">{formatBytes(entry.size)}{preview?.truncated ? " · showing the first 512 KB" : ""}</p>
    {#if markdown}
      <div class="flex overflow-hidden rounded-md border border-line text-xs" role="group" aria-label="View">
        <button class="px-2 py-0.5 {showSource ? 'text-fg-muted hover:text-fg' : 'bg-accent/20 text-fg'}" aria-pressed={!showSource} onclick={() => (showSource = false)}>Rendered</button>
        <button class="border-l border-line px-2 py-0.5 {showSource ? 'bg-accent/20 text-fg' : 'text-fg-muted hover:text-fg'}" aria-pressed={showSource} onclick={() => (showSource = true)}>Source</button>
      </div>
    {/if}
  </div>
  {#if !preview}
    <div class="flex justify-center py-8"><Spinner label="Loading…" /></div>
  {:else if image && preview.base64}
    <img class="mx-auto max-h-[70vh] max-w-full rounded" src="data:{mime};base64,{preview.base64}" alt={entry.name} />
  {:else if markdown && !showSource}
    <div class="md max-h-[70vh] overflow-auto rounded-md border border-line bg-base p-4 text-sm leading-relaxed" data-testid="markdown-preview">{@html renderMarkdown(preview.text ?? "", { headings: "full" })}</div>
  {:else if preview.kind === "text"}
    <pre class="max-h-[70vh] overflow-auto rounded-md border border-line bg-base p-3 font-mono text-xs leading-snug whitespace-pre">{preview.text}</pre>
  {:else}
    <p class="py-8 text-center text-sm text-fg-muted">Binary file; nothing to show.</p>
  {/if}
</Modal>

<style>
  .md > :global(:first-child) { margin-top: 0; }
  .md :global(p) { margin: 0 0 0.75rem; }
  .md :global(h1), .md :global(h2), .md :global(h3), .md :global(h4), .md :global(h5), .md :global(h6) { font-weight: 600; line-height: 1.3; margin: 1.25rem 0 0.5rem; }
  .md :global(h1) { font-size: 1.5rem; border-bottom: 1px solid var(--color-line); padding-bottom: 0.25rem; }
  .md :global(h2) { font-size: 1.25rem; border-bottom: 1px solid var(--color-line); padding-bottom: 0.2rem; }
  .md :global(h3) { font-size: 1.1rem; }
  .md :global(h4), .md :global(h5), .md :global(h6) { font-size: 0.95rem; }
  .md :global(ul) { list-style: disc; padding-left: 1.5rem; margin: 0 0 0.75rem; }
  .md :global(ol) { list-style: decimal; padding-left: 1.5rem; margin: 0 0 0.75rem; }
  .md :global(code) { font-family: var(--font-mono); font-size: 0.85em; background: var(--color-panel); padding: 0.05rem 0.3rem; border-radius: 0.25rem; }
  .md :global(pre) { background: var(--color-panel); padding: 0.75rem; border-radius: 0.375rem; overflow-x: auto; margin: 0 0 0.75rem; }
  .md :global(pre code) { background: none; padding: 0; }
  .md :global(a) { color: var(--color-accent); text-decoration: underline; }
  .md :global(blockquote) { border-left: 3px solid var(--color-line); padding-left: 0.75rem; color: var(--color-fg-muted); margin: 0 0 0.75rem; }
  .md :global(hr) { border: 0; border-top: 1px solid var(--color-line); margin: 1rem 0; }
  .md :global(table) { border-collapse: collapse; margin: 0 0 0.75rem; display: block; overflow-x: auto; }
  .md :global(th), .md :global(td) { border: 1px solid var(--color-line); padding: 0.25rem 0.6rem; }
  .md :global(th) { background: var(--color-panel); font-weight: 600; }
</style>
