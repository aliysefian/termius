<script lang="ts">
  import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-svelte";
  import { addParam, moveParam, removeParam, renameParam, updateParam, type Obj } from "$lib/runbookdoc";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  const params = $derived(rb.loaded.model?.params ?? []);
  const blocked = $derived(rb.loaded.unsupported.length > 0 || !rb.loaded.model);
  let open = $state<number | null>(null);

  const KINDS: { value: string; label: string; help: string }[] = [
    { value: "text", label: "String", help: "Any text. Add choices to make it a choice list." },
    { value: "number", label: "Number", help: "A decimal number, checked when you run." },
    { value: "boolean", label: "Boolean", help: "true or false." },
    { value: "secret", label: "Secret", help: "Typed each time you run, shown as dots, never saved in the document, a schedule or the record." },
    { value: "file", label: "File", help: "A file on this computer, chosen when you run. Only an upload step can use it." },
    { value: "dir", label: "Folder", help: "A folder on this computer, chosen when you run. Only a download step can save into it." },
  ];

  const kindOf = (p: Obj) => (typeof p.kind === "string" ? p.kind : "text");

  function create() {
    const names = new Set(params.map((p) => String(p.name)));
    let n = 1;
    while (names.has(`variable_${n}`)) n++;
    if (rb.apply(addParam(rb.text, { name: `variable_${n}`, label: `Variable ${n}` }))) open = params.length;
  }

  /** Save the variable as a whole with the given change (undefined removes a field). */
  function change(i: number, key: string, value: unknown) {
    const next: Obj = { ...(params[i] as Obj) };
    if (value === undefined || value === "" || value === false || (Array.isArray(value) && value.length === 0)) delete next[key];
    else next[key] = value as never;
    // A kind that can't have a default or choices drops them, so the document stays valid.
    const k = kindOf(next);
    if (k === "secret" || k === "file" || k === "dir") delete next.default;
    if (k === "secret" || k === "file" || k === "dir" || k === "boolean") delete next.choices;
    rb.apply(updateParam(rb.text, i, next));
  }

  function rename(i: number, to: string) {
    const name = to.trim();
    if (name && name !== params[i].name) rb.apply(renameParam(rb.text, i, name));
  }

  /** The names of the variables the steps use that no variable defines. */
  const undefinedNames = $derived((rb.check?.problems ?? []).filter((p) => p.message.includes("isn't a parameter")).map((p) => p.message));
</script>

<div class="mx-auto max-w-3xl space-y-4 p-4" data-testid="variables-tab">
  <div class="flex items-start gap-3">
    <p class="flex-1 text-xs text-fg-muted">A variable is a value you give when the runbook runs. Use it in a step as <code>{"{{name}}"}</code>, or <code>{"{{name|q}}"}</code> to quote it for the shell. <code>{"{{host}}"}</code> and <code>{"{{label}}"}</code> are the host's own and need no variable.</p>
    <button class="btn-secondary shrink-0 text-xs" disabled={blocked} onclick={create} data-testid="add-variable"><Plus size={13} /> Add variable</button>
  </div>
  {#if blocked}<p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">{rb.loaded.unsupported[0] ?? "The document can't be read."} Edit variables in the YAML view.</p>{/if}
  {#each undefinedNames as m (m)}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs text-danger">{m}</p>{/each}

  {#if params.length === 0}
    <p class="rounded-lg border border-dashed border-line px-4 py-8 text-center text-sm text-fg-muted">No variables. This runbook runs the same way every time.</p>
  {/if}

  <ul class="space-y-2">
    {#each params as p, i (i)}
      {@const kind = kindOf(p)}
      {@const info = KINDS.find((k) => k.value === kind)}
      <li class="rounded-[10px] border border-line bg-panel" data-testid="variable-row">
        <div class="flex items-center gap-2 px-3 py-2">
          <button class="min-w-0 flex-1 text-left" aria-expanded={open === i} onclick={() => (open = open === i ? null : i)}>
            <span class="flex flex-wrap items-center gap-2"><code class="font-mono text-sm font-medium">{String(p.name)}</code><span class="rounded bg-fg-muted/15 px-1.5 text-[10px] uppercase tracking-wide text-fg-muted">{info?.label ?? kind}</span>
              {#if p.optional}<span class="text-[10px] text-fg-muted">optional</span>{:else if p.default === undefined && kind !== "secret"}<span class="text-[10px] text-warning">required</span>{/if}
              {#if Array.isArray(p.choices) && p.choices.length}<span class="text-[10px] text-fg-muted">{p.choices.length} choices</span>{/if}</span>
            <span class="block truncate text-xs text-fg-muted">{typeof p.label === "string" && p.label ? p.label : ""}{typeof p.description === "string" && p.description ? ` · ${p.description}` : ""}</span>
          </button>
          <button class="icon-btn h-6 w-6" aria-label="Move {String(p.name)} up" disabled={i === 0 || blocked} onclick={() => rb.apply(moveParam(rb.text, i, i - 1))}><ArrowUp size={13} /></button>
          <button class="icon-btn h-6 w-6" aria-label="Move {String(p.name)} down" disabled={i === params.length - 1 || blocked} onclick={() => rb.apply(moveParam(rb.text, i, i + 1))}><ArrowDown size={13} /></button>
          <button class="icon-btn h-6 w-6 text-danger" aria-label="Delete {String(p.name)}" disabled={blocked} onclick={() => { rb.apply(removeParam(rb.text, i)); open = null; }}><Trash2 size={13} /></button>
        </div>
        {#if open === i}
          <div class="grid gap-3 border-t border-line p-3 sm:grid-cols-2">
            <label class="block text-xs"><span class="mb-1 block font-medium">Name</span>
              <input class="input h-8 w-full font-mono text-xs" value={String(p.name)} spellcheck="false" onchange={(e) => rename(i, e.currentTarget.value)} />
              <span class="mt-1 block text-[11px] text-fg-muted">Lowercase letters, digits and _. Renaming changes every use of it.</span>
            </label>
            <label class="block text-xs"><span class="mb-1 block font-medium">Type</span>
              <select class="input h-8 w-full text-xs" value={kind} onchange={(e) => change(i, "kind", e.currentTarget.value === "text" ? undefined : e.currentTarget.value)}>
                {#each KINDS as k (k.value)}<option value={k.value}>{k.label}</option>{/each}
              </select>
              <span class="mt-1 block text-[11px] text-fg-muted">{info?.help}</span>
            </label>
            <label class="block text-xs"><span class="mb-1 block font-medium">Label</span>
              <input class="input h-8 w-full text-xs" value={typeof p.label === "string" ? p.label : ""} onchange={(e) => change(i, "label", e.currentTarget.value.trim())} />
            </label>
            <label class="block text-xs"><span class="mb-1 block font-medium">Description</span>
              <input class="input h-8 w-full text-xs" value={typeof p.description === "string" ? p.description : ""} onchange={(e) => change(i, "description", e.currentTarget.value.trim())} />
            </label>
            {#if kind !== "secret" && kind !== "file" && kind !== "dir"}
              <label class="block text-xs"><span class="mb-1 block font-medium">Default value</span>
                {#if kind === "boolean"}
                  <select class="input h-8 w-full text-xs" value={typeof p.default === "string" ? p.default : ""} onchange={(e) => change(i, "default", e.currentTarget.value || undefined)}><option value="">None</option><option value="true">true</option><option value="false">false</option></select>
                {:else}
                  <input class="input h-8 w-full font-mono text-xs" value={typeof p.default === "string" ? p.default : ""} onchange={(e) => change(i, "default", e.currentTarget.value)} />
                {/if}
              </label>
            {/if}
            {#if kind === "text" || kind === "number"}
              <label class="block text-xs"><span class="mb-1 block font-medium">Choices</span>
                <input class="input h-8 w-full text-xs" placeholder="a, b, c (leave empty for free text)" value={Array.isArray(p.choices) ? p.choices.join(", ") : ""} onchange={(e) => change(i, "choices", e.currentTarget.value.split(",").map((c) => c.trim()).filter(Boolean))} />
              </label>
            {/if}
            <label class="flex items-start gap-2 text-xs sm:col-span-2"><input type="checkbox" class="mt-0.5 accent-input" checked={p.optional === true} onchange={(e) => change(i, "optional", e.currentTarget.checked)} /> <span><span class="font-medium">Optional</span><span class="block text-[11px] text-fg-muted">Without a value, the run goes on with an empty one.</span></span></label>
            {#if kind === "secret"}<p class="rounded-md border border-line bg-base/50 px-3 py-2 text-[11px] text-fg-muted sm:col-span-2">A secret is never written into the document. It is hidden from the log, the preview, error messages and the record of the run. It does appear in the command the host runs, so a step that uses it should read it from where it can't be seen by other users of the host.</p>{/if}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</div>
