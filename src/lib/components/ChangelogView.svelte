<script lang="ts">
  import { History } from "lucide-svelte";
  import { renderMarkdown } from "$lib/markdown";
  import changelogSource from "../../../CHANGELOG.md?raw";

  // Drop the leading "# Changelog" heading and its intro line: the page
  // already has its own title, and "also shown in the app itself" reads
  // oddly from inside the app itself.
  const body = changelogSource.replace(/^# Changelog\n\n[\s\S]*?\n\n/, "");
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-3xl space-y-4">
    <h1 class="flex items-center gap-2 text-lg font-semibold"><History size={20} class="text-accent" /> Changelog</h1>
    <div class="changelog space-y-6 text-sm">
      {@html renderMarkdown(body)}
    </div>
  </div>
</div>

<style>
  .changelog :global(h2) {
    margin-top: 1.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    padding-bottom: 0.25rem;
    border-bottom: 1px solid var(--color-line);
  }
  .changelog :global(h2:first-child) {
    margin-top: 0;
  }
  .changelog :global(ul) {
    margin-top: 0.5rem;
    list-style: disc;
    padding-left: 1.25rem;
  }
  .changelog :global(li) {
    margin-bottom: 0.35rem;
  }
  .changelog :global(li strong) {
    color: var(--color-fg);
  }
  .changelog :global(p) {
    color: var(--color-fg-muted);
    font-size: 0.75rem;
  }
  .changelog :global(code) {
    background: var(--color-base);
    border-radius: 0.25rem;
    padding: 0.05rem 0.3rem;
    font-size: 0.85em;
  }
</style>
