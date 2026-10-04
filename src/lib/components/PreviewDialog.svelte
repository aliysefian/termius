<script lang="ts">
  import Modal from "./Modal.svelte";
  import { formatBytes, type FileEntry } from "$lib/types";
  import type { Preview } from "$lib/sftp";
  import Spinner from "./Spinner.svelte";

  let { entry, preview, onclose }: { entry: FileEntry; preview: Preview | null; onclose: () => void } = $props();
  const image = $derived(/\.(png|jpe?g|gif|webp|bmp|svg|ico)$/i.test(entry.name));
  const mime = $derived(
    entry.name.toLowerCase().endsWith(".svg") ? "image/svg+xml" : `image/${entry.name.split(".").pop()?.toLowerCase().replace("jpg", "jpeg")}`,
  );
</script>

<Modal title={entry.name} onclose={onclose} width="max-w-3xl">
  <p class="mb-2 text-xs text-fg-muted">{formatBytes(entry.size)}{preview?.truncated ? " · showing the first 512 KB" : ""}</p>
  {#if !preview}
    <div class="flex justify-center py-8"><Spinner label="Loading…" /></div>
  {:else if image && preview.base64}
    <img class="mx-auto max-h-[70vh] max-w-full rounded" src="data:{mime};base64,{preview.base64}" alt={entry.name} />
  {:else if preview.kind === "text"}
    <pre class="max-h-[70vh] overflow-auto rounded-md border border-line bg-base p-3 font-mono text-xs leading-snug whitespace-pre">{preview.text}</pre>
  {:else}
    <p class="py-8 text-center text-sm text-fg-muted">Binary file; nothing to show.</p>
  {/if}
</Modal>
