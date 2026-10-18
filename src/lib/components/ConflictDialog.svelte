<script lang="ts">
  import { onMount } from "svelte";
  import { CheckCircle2, Columns2, FileDiff, Loader2, Rows2 } from "lucide-svelte";
  import { canDiff, compareSides, DIFF_MAX_BYTES, sizeDeltaText, type Decision, type SideInfo } from "$lib/conflict";
  import { diffLines, hunks, splitRows, type DiffResult } from "$lib/diff";
  import { fileops } from "$lib/sftp";
  import { errorMessage, formatBytes } from "$lib/types";

  // A file is about to be copied over one that is already there. Shows both side by side (size, time and SHA-256),
  // says so when they are the same file, can show exactly which lines would change, and asks what to do.
  let {
    src,
    dst,
    srcSession,
    dstSession,
    srcLabel,
    dstLabel,
    remaining,
    onanswer,
    oncancel,
  }: {
    src: SideInfo;
    dst: SideInfo;
    srcSession: string;
    dstSession: string;
    srcLabel: string;
    dstLabel: string;
    /** Clashes after this one. */
    remaining: number;
    onanswer: (decision: Decision, applyToAll: boolean) => void;
    oncancel: () => void;
  } = $props();

  // The checksums arrive as the places compute them; both are asked for at once.
  let srcHash = $state<string | null | undefined>(undefined);
  let dstHash = $state<string | null | undefined>(undefined);
  let hashNote = $state<string | null>(null);

  const cmp = $derived(compareSides({ ...src, sha256: srcHash ?? undefined }, { ...dst, sha256: dstHash ?? undefined }));
  const pending = $derived(srcHash === undefined || dstHash === undefined);
  const diffable = $derived(canDiff(src, dst));

  onMount(() => {
    if (src.isDir || dst.isDir) {
      srcHash = dstHash = null;
      return;
    }
    const ask = (session: string, path: string, set: (h: string | null) => void) =>
      fileops
        .checksum(session, path)
        .then((c) => set(c.sha256))
        .catch((e) => {
          set(null);
          hashNote = errorMessage(e);
        });
    void ask(srcSession, src.path, (h) => (srcHash = h));
    void ask(dstSession, dst.path, (h) => (dstHash = h));
  });

  // -- the comparison -----------------------------------------------------------------------------------------

  let diff = $state<DiffResult | null>(null);
  let diffError = $state<string | null>(null);
  let diffLoading = $state(false);
  let layout = $state<"unified" | "split">("unified");

  async function compare() {
    diffLoading = true;
    diffError = null;
    try {
      // The text that is there now is the "before"; the incoming file is the "after".
      const [before, after] = await Promise.all([fileops.readText(dstSession, dst.path, DIFF_MAX_BYTES), fileops.readText(srcSession, src.path, DIFF_MAX_BYTES)]);
      diff = diffLines(before.text, after.text);
    } catch (e) {
      diffError = `Can't compare these as text: ${errorMessage(e)}`;
    } finally {
      diffLoading = false;
    }
  }

  const blocks = $derived(diff ? hunks(diff.lines, 3) : []);
  const rows = $derived(diff ? splitRows(diff.lines) : []);

  let applyToAll = $state(false);

  const when = (s: number | null) => (s === null ? "unknown" : new Date(s * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "medium" }));
  const lineClass = (k: string) => (k === "add" ? "bg-success/10 text-success" : k === "del" ? "bg-danger/10 text-danger" : "text-fg-muted");
  const mark = (k: string) => (k === "add" ? "+" : k === "del" ? "−" : " ");
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") oncancel(); }} />

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" role="presentation">
  <div class="flex max-h-[92vh] w-full max-w-2xl flex-col rounded-lg border border-line bg-panel p-4" role="dialog" aria-modal="true" aria-label="{src.name} already exists" data-testid="conflict-dialog">
    <h2 class="text-sm font-semibold">
      <span class="font-mono">{src.name}</span> is already in the destination folder
    </h2>
    <p class="mt-1 text-xs text-fg-muted">Nothing is changed until you choose.</p>

    <div class="mt-3 grid grid-cols-[6rem_1fr_1fr] gap-x-3 gap-y-1.5 rounded-md border border-line bg-base/40 p-3 text-xs" role="group" aria-label="Comparison of the two files">
      <div></div>
      <div class="truncate font-medium" title={srcLabel}>Incoming · <span class="text-fg-muted">{srcLabel}</span></div>
      <div class="truncate font-medium" title={dstLabel}>Existing · <span class="text-fg-muted">{dstLabel}</span></div>

      <div class="text-fg-muted">Size</div>
      <div class="font-mono tabular-nums" data-testid="size-src">{src.isDir ? "folder" : `${formatBytes(src.size)} (${src.size.toLocaleString()} B)`}</div>
      <div class="font-mono tabular-nums" data-testid="size-dst">{dst.isDir ? "folder" : `${formatBytes(dst.size)} (${dst.size.toLocaleString()} B)`}</div>

      <div class="text-fg-muted">Modified</div>
      <div class="font-mono tabular-nums {cmp.newer === 'source' ? 'text-success' : ''}">{when(src.modified)}{#if cmp.newer === "source"} · newer{/if}</div>
      <div class="font-mono tabular-nums {cmp.newer === 'destination' ? 'text-success' : ''}">{when(dst.modified)}{#if cmp.newer === "destination"} · newer{/if}</div>

      {#if !src.isDir && !dst.isDir}
        <div class="text-fg-muted">SHA-256</div>
        <div class="break-all font-mono text-[11px] tabular-nums" data-testid="hash-src">
          {#if srcHash === undefined}<Loader2 size={12} class="animate-spin text-fg-muted" />{:else}{srcHash ?? "unavailable"}{/if}
        </div>
        <div class="break-all font-mono text-[11px] tabular-nums" data-testid="hash-dst">
          {#if dstHash === undefined}<Loader2 size={12} class="animate-spin text-fg-muted" />{:else}{dstHash ?? "unavailable"}{/if}
        </div>
      {/if}
    </div>

    <div class="mt-2 min-h-5 text-xs" aria-live="polite">
      {#if cmp.identical === true}
        <span class="inline-flex items-center gap-1.5 rounded border border-success/30 bg-success/10 px-2 py-0.5 font-medium text-success" data-testid="identical-badge">
          <CheckCircle2 size={12} /> Identical file (SHA-256 match)
        </span>
      {:else if cmp.identical === false}
        <span class="text-fg-muted">They differ{#if !src.isDir && cmp.sizeDelta !== 0}: the incoming file is {sizeDeltaText(cmp.sizeDelta, formatBytes)}{/if}.</span>
      {:else if pending}
        <span class="text-fg-muted">Checking whether they are the same file…</span>
      {:else}
        <span class="text-fg-muted">Could not tell if they are the same{hashNote ? `: ${hashNote}` : ""}.</span>
      {/if}
    </div>

    {#if diffable && cmp.identical !== true}
      <div class="mt-2">
        {#if !diff && !diffLoading}
          <button class="btn-secondary px-3 py-1 text-xs" onclick={() => void compare()} data-testid="compare-button"><FileDiff size={13} /> Compare changes (Diff)</button>
        {/if}
        {#if diffLoading}<span class="inline-flex items-center gap-2 text-xs text-fg-muted"><Loader2 size={13} class="animate-spin" /> Reading both files…</span>{/if}
        {#if diffError}<p class="text-xs text-danger" role="alert">{diffError}</p>{/if}
      </div>
    {/if}

    {#if diff}
      <div class="mt-2 flex items-center gap-3 text-xs">
        <span class="font-mono tabular-nums text-success" data-testid="diff-added">+{diff.added}</span>
        <span class="font-mono tabular-nums text-danger" data-testid="diff-removed">−{diff.removed}</span>
        {#if diff.tooDifferent}<span class="text-fg-muted">too different to compare line by line</span>{/if}
        <span class="flex-1"></span>
        <div class="flex rounded-md border border-line p-0.5" role="group" aria-label="Diff layout">
          <button class="rounded px-2 py-0.5 {layout === 'unified' ? 'bg-accent text-white' : 'text-fg-muted hover:text-fg'}" aria-pressed={layout === "unified"} aria-label="Unified" title="One column" onclick={() => (layout = "unified")}><Rows2 size={12} /></button>
          <button class="rounded px-2 py-0.5 {layout === 'split' ? 'bg-accent text-white' : 'text-fg-muted hover:text-fg'}" aria-pressed={layout === "split"} aria-label="Side by side" title="Side by side" onclick={() => (layout = "split")}><Columns2 size={12} /></button>
        </div>
      </div>
      <div class="mt-1 min-h-0 flex-1 overflow-auto rounded-md border border-line bg-base font-mono text-xs tabular-nums" style:max-height="38vh" data-testid="diff-view" data-layout={layout}>
        {#if diff.added + diff.removed === 0}
          <p class="p-3 text-fg-muted">The text is identical (the files may differ in line endings or permissions only).</p>
        {:else if layout === "unified"}
          {#each blocks as b, bi (bi)}
            {#if b.type === "fold"}
              <div class="bg-panel px-3 py-0.5 text-center text-[11px] text-fg-muted">⋯ {b.count} unchanged lines</div>
            {:else}
              {#each b.lines as l, li (li)}
                <div class="flex whitespace-pre {lineClass(l.kind)}" data-kind={l.kind}>
                  <span class="w-10 shrink-0 select-none px-1 text-right text-fg-muted/70">{l.oldNo ?? ""}</span>
                  <span class="w-10 shrink-0 select-none px-1 text-right text-fg-muted/70">{l.newNo ?? ""}</span>
                  <span class="w-4 shrink-0 select-none text-center">{mark(l.kind)}</span>
                  <span class="min-w-0 flex-1 pr-3">{l.text}</span>
                </div>
              {/each}
            {/if}
          {/each}
        {:else}
          {#each rows as r, ri (ri)}
            <div class="grid grid-cols-2">
              <div class="flex whitespace-pre border-r border-line {r.left ? lineClass(r.left.kind) : 'bg-panel/40'}" data-kind={r.left?.kind ?? "none"}>
                <span class="w-10 shrink-0 select-none px-1 text-right text-fg-muted/70">{r.left?.oldNo ?? ""}</span>
                <span class="min-w-0 flex-1 overflow-hidden px-2">{r.left?.text ?? ""}</span>
              </div>
              <div class="flex whitespace-pre {r.right ? lineClass(r.right.kind) : 'bg-panel/40'}" data-kind={r.right?.kind ?? "none"}>
                <span class="w-10 shrink-0 select-none px-1 text-right text-fg-muted/70">{r.right?.newNo ?? ""}</span>
                <span class="min-w-0 flex-1 overflow-hidden px-2">{r.right?.text ?? ""}</span>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    {/if}

    {#if remaining > 0}
      <label class="mt-3 flex cursor-pointer items-center gap-2 text-xs">
        <input type="checkbox" class="accent-input" bind:checked={applyToAll} />
        Do this for the other {remaining} name{remaining === 1 ? "" : "s"} that already exist{remaining === 1 ? "s" : ""}
      </label>
    {/if}

    <div class="mt-3 flex flex-wrap items-center justify-end gap-2">
      <button class="btn-ghost mr-auto px-3 py-1.5 text-xs" onclick={oncancel}>Cancel the copy</button>
      <button class="btn-secondary px-3 py-1.5 text-xs {cmp.identical === true ? 'border-success text-success' : ''}" onclick={() => onanswer("skip", applyToAll)} data-testid="answer-skip">Skip</button>
      <button class="btn-secondary px-3 py-1.5 text-xs" onclick={() => onanswer("rename", applyToAll)} title="Copy it beside the existing one under a new name" data-testid="answer-rename">Keep both</button>
      <button class="btn-secondary px-3 py-1.5 text-xs" onclick={() => onanswer("backup", applyToAll)} title="Rename the existing file to .bak, then copy" data-testid="answer-backup">Backup &amp; replace</button>
      <button class="btn-primary px-3 py-1.5 text-xs" onclick={() => onanswer("overwrite", applyToAll)} data-testid="answer-overwrite">Overwrite</button>
    </div>
  </div>
</div>
