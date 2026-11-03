<script lang="ts">
  import { CheckCheck, CircleAlert, CircleCheck, Download, FileText, Save, Sparkles } from "lucide-svelte";
  import Builder from "./Builder.svelte";
  import SettingsTab from "./SettingsTab.svelte";
  import VariablesTab from "./VariablesTab.svelte";
  import YamlEditor from "./YamlEditor.svelte";
  import { formatOf, wouldLoseComments } from "$lib/runbookdoc";
  import { ask } from "$lib/dialogs.svelte";
  import { runbooks as rb, type EditorMode } from "$lib/stores/runbooks.svelte";

  let { onbrowse }: { onbrowse: () => void } = $props();

  let yamlEditor = $state<{ focus(): void; goTo(line: number): void }>();
  const MODES: { id: EditorMode; label: string }[] = [
    { id: "builder", label: "Builder" },
    { id: "yaml", label: "YAML" },
    { id: "variables", label: "Variables" },
    { id: "settings", label: "Settings" },
  ];
  const problems = $derived(rb.check?.problems ?? []);
  const format = $derived(formatOf(rb.text));
  const hasText = $derived(rb.text.trim() !== "");

  function key(e: KeyboardEvent) {
    const i = MODES.findIndex((m) => m.id === rb.mode);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? MODES.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = MODES[(to + MODES.length) % MODES.length];
    rb.mode = next.id;
    queueMicrotask(() => document.getElementById(`rb-mode-${next.id}`)?.focus());
  }

  async function toggleFormat() {
    const to = format === "json" ? "yaml" : "json";
    if (to === "json" && wouldLoseComments(rb.text, "json") && !(await ask("JSON has no comments, so the comments in this document would be lost. Convert anyway?", { title: "Convert to JSON", confirm: "Convert", danger: true }))) return;
    await rb.toYaml(to);
  }

  const stepLines = $derived(rb.where.steps.map((w) => w.line));
  const rollbackLines = $derived(rb.where.rollback.map((w) => w.line));
</script>

<section class="flex h-full min-h-0 flex-col" aria-label="Workflow editor" data-testid="editor-panel">
  {#if !hasText}
    <div class="flex flex-1 items-center justify-center p-8">
      <div class="max-w-sm text-center">
        <Sparkles size={28} class="mx-auto text-accent" />
        <h2 class="mt-3 text-[15px] font-semibold">Create your first automation</h2>
        <p class="mt-1 text-sm text-fg-muted">A runbook is a list of steps run on one host or many: commands, files, checks and Docker. Build it with the Builder or write it as YAML.</p>
        <div class="mt-4 flex flex-wrap justify-center gap-2">
          <button class="btn-primary text-sm" onclick={() => rb.newBlank()} data-testid="create-blank">Create blank runbook</button>
          <button class="btn-secondary text-sm" onclick={onbrowse}>Browse templates</button>
          <button class="btn-secondary text-sm" onclick={() => rb.importFile()}>Import YAML</button>
        </div>
      </div>
    </div>
  {:else}
    <header class="space-y-2 border-b border-line px-4 pt-3">
      <div class="flex items-center gap-2">
        <FileText size={16} class="shrink-0 text-accent" />
        <h2 class="min-w-0 truncate text-[15px] font-semibold" title={rb.name}>{rb.name || "Untitled"}</h2>
        {#if rb.dirty}<span class="h-2 w-2 shrink-0 rounded-full bg-warning" title="Unsaved changes" aria-label="Unsaved changes"></span>{/if}
        {#if rb.loaded.model?.category}<span class="rounded border border-line px-1.5 text-[10px] text-fg-muted">{rb.loaded.model.category}</span>{/if}
        <div class="flex-1"></div>
        <button class="btn-ghost h-7 px-2 text-xs" title={format === "yaml" ? "Convert to JSON" : "Convert to YAML"} onclick={toggleFormat} data-testid="format-toggle">{format === "yaml" ? "YAML" : "JSON"}</button>
        <button class="icon-btn h-7 w-7" title="Export to a file" aria-label="Export" onclick={() => rb.exportFile()}><Download size={14} /></button>
        <button class="btn-secondary h-7 px-2 text-xs" onclick={() => void rb.validateNow()} title="Validate (Ctrl+Enter)"><CheckCheck size={13} /> Validate</button>
        <button class="btn-primary h-7 px-2.5 text-xs" disabled={!rb.dirty} onclick={() => rb.save()} title="Save (Ctrl+S)" data-testid="save-runbook"><Save size={13} /> Save</button>
      </div>
      <div class="flex items-end gap-1" role="tablist" aria-label="Editor views">
        {#each MODES as m (m.id)}
          <button
            id="rb-mode-{m.id}"
            role="tab"
            aria-selected={rb.mode === m.id}
            tabindex={rb.mode === m.id ? 0 : -1}
            class="-mb-px border-b-2 px-3 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent {rb.mode === m.id ? 'border-accent font-medium text-fg' : 'border-transparent text-fg-muted hover:text-fg'}"
            onclick={() => (rb.mode = m.id)}
            onkeydown={key}
          >{m.label}{#if m.id === "variables" && rb.loaded.model?.params.length}<span class="ml-1 text-fg-muted">{rb.loaded.model.params.length}</span>{/if}{#if (m.id === "builder" || m.id === "yaml") && problems.length}<span class="ml-1.5 rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{problems.length}</span>{/if}</button>
        {/each}
        <div class="flex-1"></div>
        <p class="mb-1.5 flex items-center gap-1.5 text-xs" role="status" data-testid="doc-status">
          {#if problems.length}<CircleAlert size={13} class="text-danger" /> <span class="text-danger">{problems.length} problem{problems.length === 1 ? "" : "s"}</span>
          {:else if rb.check?.name}<CircleCheck size={13} class="text-success" /> <span class="text-success">Valid · {rb.check.steps} step{rb.check.steps === 1 ? "" : "s"}</span>{/if}
        </p>
      </div>
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto" id="rb-mode-panel" role="tabpanel" aria-labelledby="rb-mode-{rb.mode}">
      {#if rb.mode === "builder"}
        <Builder />
      {:else if rb.mode === "yaml"}
        <div class="flex h-full flex-col">
          {#if problems.length || rb.loaded.error}
            <ul class="max-h-28 shrink-0 space-y-0.5 overflow-y-auto border-b border-danger/30 bg-danger/10 px-4 py-2 text-xs text-danger" role="alert" data-testid="yaml-problems">
              {#if rb.loaded.error}<li>{rb.loaded.error.line ? `Line ${rb.loaded.error.line}: ` : ""}{rb.loaded.error.message}</li>
              {:else}{#each problems as p (p.message + p.step)}<li><button class="text-left hover:underline" onclick={() => (p.step !== null ? yamlEditor?.goTo(stepLines[p.step] ?? 1) : p.line ? yamlEditor?.goTo(p.line) : undefined)}>{p.step !== null ? `Step ${p.step + 1}: ` : p.line ? `Line ${p.line}: ` : ""}{p.message}</button></li>{/each}{/if}
            </ul>
          {/if}
          <div class="min-h-0 flex-1">
            <YamlEditor
              bind:this={yamlEditor}
              value={rb.text}
              problems={rb.loaded.error ? [] : problems}
              parseError={rb.loaded.error}
              {stepLines}
              {rollbackLines}
              onchange={(v) => (rb.text = v)}
              onsave={() => rb.save()}
              onformat={() => rb.tidy()}
              onvalidate={() => void rb.validateNow()}
            />
          </div>
        </div>
      {:else if rb.mode === "variables"}
        <VariablesTab />
      {:else}
        <SettingsTab />
      {/if}
    </div>
  {/if}
</section>
