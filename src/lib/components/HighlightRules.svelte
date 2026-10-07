<script lang="ts">
  import { Plus, Trash2 } from "lucide-svelte";
  import { MAX_RULES, PRESETS, matchLine, newId, problem, withId, type HighlightRule } from "$lib/highlight";
  import { settings } from "$lib/stores/settings.svelte";

  let sample = $state("error: connection refused from 10.0.0.12 (warning: timeout after 30s)\nbuild passed, 4 tests done");

  const rules = $derived(settings.prefs.highlightRules);

  function add(p?: Omit<HighlightRule, "id">) {
    if (rules.length >= MAX_RULES) return;
    settings.prefs.highlightRules = [...rules, p ? withId(p) : { id: newId(), name: "", pattern: "", regex: false, matchCase: false, color: "#7a2630", enabled: true, notify: false }];
  }
  const remove = (id: string) => (settings.prefs.highlightRules = rules.filter((r) => r.id !== id));
  const missing = $derived(PRESETS.filter((p) => !rules.some((r) => r.name === p.name)));

  /** The sample as pieces: plain text and the parts a rule colours. */
  const preview = $derived(
    sample.split("\n").map((line) => {
      const out: { text: string; color?: string }[] = [];
      let at = 0;
      for (const m of matchLine($state.snapshot(rules) as HighlightRule[], line)) {
        if (m.from > at) out.push({ text: line.slice(at, m.from) });
        out.push({ text: line.slice(m.from, m.to), color: m.rule.color });
        at = m.to;
      }
      out.push({ text: line.slice(at) });
      return out;
    }),
  );
</script>

<div class="col-span-2 space-y-2" data-testid="highlight-rules">
  <div class="flex items-center justify-between gap-2">
    <div>
      <div class="text-sm font-medium">Highlight words</div>
      <div class="text-xs text-fg-muted">Colour words in terminal output. Earlier rules win where two overlap. Only the lines on screen are coloured.</div>
    </div>
    <button class="btn-ghost" onclick={() => add()} disabled={rules.length >= MAX_RULES}><Plus size={14} /> Add a rule</button>
  </div>
  {#if missing.length}
    <div class="flex flex-wrap items-center gap-1.5 text-xs text-fg-muted">
      Start from:
      {#each missing as p (p.name)}
        <button class="rounded border border-line px-2 py-0.5 hover:bg-hover" onclick={() => add(p)}>{p.name}</button>
      {/each}
    </div>
  {/if}
  {#each rules as r (r.id)}
    {@const bad = problem(r)}
    <div class="space-y-1 rounded-md border border-line p-2">
      <div class="flex items-center gap-2">
        <input type="checkbox" class="accent-input" bind:checked={r.enabled} aria-label="Use this rule" />
        <input class="input w-28" bind:value={r.name} placeholder="Name" aria-label="Rule name" />
        <input class="input min-w-0 flex-1 font-mono" bind:value={r.pattern} placeholder="Text or pattern" aria-label="Text or pattern" aria-invalid={bad ? "true" : undefined} />
        <input type="color" bind:value={r.color} class="h-8 w-9 cursor-pointer rounded border border-line bg-transparent" aria-label="Highlight colour" />
        <button class="btn-ghost" onclick={() => remove(r.id)} aria-label="Delete this rule"><Trash2 size={14} /></button>
      </div>
      <div class="flex flex-wrap items-center gap-4 pl-6 text-xs text-fg-muted">
        <label class="flex items-center gap-1"><input type="checkbox" bind:checked={r.regex} /> Pattern</label>
        <label class="flex items-center gap-1"><input type="checkbox" bind:checked={r.matchCase} /> Match case</label>
        <label class="flex items-center gap-1" title="A desktop notice when new output matches and the app is in the background."><input type="checkbox" bind:checked={r.notify} /> Notify me</label>
        {#if bad}<span class="text-danger">{bad}</span>{/if}
      </div>
    </div>
  {/each}
  <div>
    <label class="mb-1 block text-xs text-fg-muted" for="hl-sample">Try it on some text</label>
    <textarea id="hl-sample" class="input h-14 w-full font-mono text-xs" bind:value={sample}></textarea>
    <pre class="mt-1 overflow-auto rounded border border-line bg-base p-2 font-mono text-xs" data-testid="highlight-preview">{#each preview as line, i (i)}{#each line as piece, j (j)}{#if piece.color}<span style="background:{piece.color}">{piece.text}</span>{:else}{piece.text}{/if}{/each}{"\n"}{/each}</pre>
  </div>
</div>
