<script lang="ts">
  import { X } from "lucide-svelte";
  import { changeKind, ensureStepId, kindOf, setStepField, setStepPath, STEP_KINDS, type Json, type Obj, type StepKind } from "$lib/runbookdoc";
  import { fieldsFor, KIND_BY, type Field } from "$lib/runbookschema";
  import { ask } from "$lib/dialogs.svelte";
  import { runbooks as rb } from "$lib/stores/runbooks.svelte";

  let { index, rollback = false, problems, onclose }: { index: number; rollback?: boolean; problems: string[]; onclose: () => void } = $props();

  const list = $derived((rollback ? rb.loaded.model?.rollback : rb.loaded.model?.steps) ?? []);
  const step = $derived((list[index] ?? {}) as Obj);
  const kind = $derived(kindOf(step));
  const body = $derived((kind && step[kind] && typeof step[kind] === "object" ? (step[kind] as Obj) : {}) as Obj);
  const fields = $derived(kind ? fieldsFor(kind, body) : []);
  const params = $derived(rb.loaded.model?.params ?? []);
  /** The steps before this one, which a condition may look at (for a rollback step, all the steps and the rollback steps before it). */
  interface Before {
    id: string;
    name: string;
    at: number;
    rb: boolean;
  }
  const entry = (s: Obj, i: number, rbFlag: boolean): Before => ({ id: typeof s.id === "string" ? s.id : "", name: typeof s.name === "string" && s.name ? s.name : `Step ${i + 1}`, at: i, rb: rbFlag });
  const before = $derived<Before[]>([...(rollback ? (rb.loaded.model?.steps ?? []).map((s, i) => entry(s, i, false)) : []), ...list.slice(0, index).map((s, i) => entry(s, i, rollback))]);
  const keyOf = (b: Before) => b.id || `#${b.rb ? "r" : "s"}${b.at}`;
  const when = $derived((step.when && typeof step.when === "object" ? step.when : null) as Obj | null);
  const whenKind = $derived(!when ? "always" : when.step !== undefined || when.exit !== undefined || when.exit_not !== undefined ? "step" : "param");

  const set = (key: string, value: Json | undefined) => rb.apply(setStepField(rb.text, index, key, value, rollback));
  const setIn = (path: string[], value: Json | undefined) => rb.apply(setStepPath(rb.text, index, path, value, rollback));

  /** A number field's value: whole numbers only, empty removes it. */
  function number(v: string): number | undefined {
    const n = v.trim() === "" ? undefined : Number(v);
    return n !== undefined && Number.isFinite(n) ? n : undefined;
  }

  function fieldValue(f: Field): string {
    const v = body[f.key];
    return v === undefined || v === null ? "" : String(v);
  }

  function setField(f: Field, raw: string | boolean) {
    if (f.type === "bool") setIn([kind!, f.key], raw === true ? true : undefined);
    else if (f.type === "number") setIn([kind!, f.key], number(String(raw)));
    else if (kind === "run") set("run", String(raw));
    else setIn([kind!, f.key], String(raw));
  }

  async function changeTo(to: StepKind) {
    if (to === kind) return;
    const had = kind ? Object.keys(body).length > 0 : false;
    if (had && !(await ask(`Change this step to "${KIND_BY[to].label}"? Its current settings are replaced; its name, condition and timeout stay.`, { title: "Change the step type", confirm: "Change" }))) return;
    rb.apply(changeKind(rb.text, index, to, rollback));
  }

  // -- conditions ------------------------------------------------------------------------------------

  function setWhen(kindOfWhen: string) {
    if (kindOfWhen === "always") return set("when", undefined);
    if (kindOfWhen === "step") {
      const first = before.at(-1);
      if (!first) return;
      pickStep(first);
    } else {
      const p = params.find((x) => ["text", "number", "boolean", undefined].includes(x.kind as string));
      if (p) rb.apply(setStepField(rb.text, index, "when", { param: p.name as string, equals: "" }, rollback));
    }
  }

  /** Choose the step a condition looks at, giving it an id if it has none. */
  function pickStep(b: Before) {
    let text = rb.text;
    let want = b.id;
    if (!want) {
      const made = ensureStepId(text, b.at, b.rb);
      if ("error" in made) return void rb.apply(made);
      text = made.text;
      want = made.id;
    }
    const cur = when ?? {};
    const next: Obj = { step: want };
    if (cur.exit !== undefined) next.exit = cur.exit;
    else if (cur.exit_not !== undefined) next.exit_not = cur.exit_not;
    else next.exit = 0;
    rb.apply(setStepField(text, index, "when", next, rollback));
  }

  function setWhenField(key: string, value: Json | undefined, exclusive: string[] = []) {
    const next: Obj = { ...(when ?? {}) };
    for (const k of exclusive) delete next[k];
    if (value === undefined) delete next[key];
    else next[key] = value;
    rb.apply(setStepField(rb.text, index, "when", next, rollback));
  }

  const params2 = $derived(params.filter((p) => ["text", "number", "boolean", undefined].includes(p.kind as string)));
</script>

<section class="space-y-4 rounded-[10px] border border-accent/40 bg-base/60 p-3.5" aria-label="Edit step {index + 1}" data-testid="step-editor">
  <header class="flex items-center gap-2">
    <h4 class="flex-1 text-xs font-semibold uppercase tracking-wide text-fg-muted">Step {index + 1}{rollback ? " (rollback)" : ""}</h4>
    <button class="icon-btn h-6 w-6" title="Close the editor" aria-label="Close the step editor" onclick={onclose}><X size={14} /></button>
  </header>

  {#each problems as p (p)}<p class="rounded-md border border-danger/30 bg-danger/10 px-2.5 py-1.5 text-xs text-danger" role="alert">{p}</p>{/each}

  <div class="grid gap-3 sm:grid-cols-2">
    <label class="block text-xs"><span class="mb-1 block font-medium">Name</span>
      <input class="input h-8 w-full text-sm" value={typeof step.name === "string" ? step.name : ""} placeholder="What this step does" onchange={(e) => set("name", e.currentTarget.value.trim() || undefined)} />
    </label>
    <label class="block text-xs"><span class="mb-1 block font-medium">Type</span>
      <select class="input h-8 w-full text-sm" value={kind ?? ""} onchange={(e) => void changeTo(e.currentTarget.value as StepKind)} aria-label="Step type">
        {#if !kind}<option value="" disabled>Choose…</option>{/if}
        {#each STEP_KINDS as k (k)}<option value={k}>{KIND_BY[k].label}</option>{/each}
      </select>
    </label>
  </div>

  {#if kind}
    <p class="text-[11px] text-fg-muted">{KIND_BY[kind].about}{KIND_BY[kind].local ? " It acts on this computer, not on the host." : ""}</p>
    <div class="space-y-3">
      {#each fields as f (f.key)}
        {@const val = kind === "run" ? String(step.run ?? "") : fieldValue(f)}
        <div>
          {#if f.type === "bool"}
            <label class="flex items-start gap-2 text-xs"><input type="checkbox" class="mt-0.5 accent-input" checked={body[f.key] === true} onchange={(e) => setField(f, e.currentTarget.checked)} /> <span><span class="font-medium">{f.label}</span>{#if f.help}<span class="block text-[11px] text-fg-muted">{f.help}</span>{/if}</span></label>
          {:else}
            <label class="block text-xs" for="sf-{f.key}"><span class="mb-1 block font-medium">{f.label}{#if f.required}<span class="text-danger" aria-hidden="true"> *</span>{/if}</span></label>
            {#if f.type === "multiline"}
              <textarea id="sf-{f.key}" class="input w-full resize-y text-xs leading-5 {f.mono ? 'font-mono' : ''}" rows="3" spellcheck="false" value={val} onchange={(e) => setField(f, e.currentTarget.value)}></textarea>
            {:else if f.type === "select"}
              <select id="sf-{f.key}" class="input h-8 w-full text-sm" value={val} onchange={(e) => setField(f, e.currentTarget.value)}>
                {#if !f.required}<option value="">Default</option>{/if}
                {#each f.options ?? [] as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
              </select>
            {:else if f.type === "variable"}
              <select id="sf-{f.key}" class="input h-8 w-full font-mono text-xs" value={val} onchange={(e) => setField(f, e.currentTarget.value)}>
                <option value="">Choose a variable…</option>
                {#each params.filter((p) => (f.variableKinds ?? []).includes(p.kind as "file" | "dir")) as p (p.name)}<option value={`{{${p.name}}}`}>{`{{${p.name}}}`} {p.label ? `· ${p.label}` : ""}</option>{/each}
              </select>
              {#if !params.some((p) => (f.variableKinds ?? []).includes(p.kind as "file" | "dir"))}
                <p class="mt-1 text-[11px] text-warning">Add a variable of kind "{f.variableKinds?.[0]}" in the Variables tab first.</p>
              {/if}
            {:else}
              <input id="sf-{f.key}" class="input h-8 w-full text-sm {f.mono ? 'font-mono text-xs' : ''}" type="text" inputmode={f.type === "number" ? "numeric" : undefined} value={val} placeholder={f.placeholder} min={f.min} max={f.max} spellcheck="false" onchange={(e) => setField(f, e.currentTarget.value)} />
            {/if}
            {#if f.help}<p class="mt-1 text-[11px] text-fg-muted">{f.help}</p>{/if}
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <fieldset class="space-y-3 border-t border-line pt-3">
    <legend class="sr-only">Conditions and error handling</legend>
    <div>
      <span class="mb-1 block text-xs font-medium">Run this step</span>
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <select class="input h-8 w-auto text-xs" value={whenKind} onchange={(e) => setWhen(e.currentTarget.value)} aria-label="When to run">
          <option value="always">Always</option>
          <option value="step" disabled={before.length === 0}>Depending on an earlier step's exit code</option>
          <option value="param" disabled={params2.length === 0}>Depending on a variable</option>
        </select>
        {#if when && whenKind === "step"}
          <select class="input h-8 w-auto text-xs" value={typeof when.step === "string" ? when.step : ""} aria-label="Earlier step" onchange={(e) => { const s = before.find((b) => keyOf(b) === e.currentTarget.value); if (s) pickStep(s); }}>
            {#each before as b (keyOf(b))}<option value={keyOf(b)}>{b.rb ? "Rollback " : ""}{b.at + 1}. {b.name}</option>{/each}
          </select>
          <select class="input h-8 w-auto text-xs" value={when.exit_not !== undefined ? "exit_not" : "exit"} aria-label="Comparison" onchange={(e) => setWhenField(e.currentTarget.value, Number(when.exit ?? when.exit_not ?? 0), ["exit", "exit_not"])}>
            <option value="exit">exited with</option><option value="exit_not">did not exit with</option>
          </select>
          <input class="input h-8 w-16 text-xs" type="text" inputmode="numeric" value={String(when.exit ?? when.exit_not ?? 0)} aria-label="Exit code" onchange={(e) => setWhenField(when.exit_not !== undefined ? "exit_not" : "exit", number(e.currentTarget.value) ?? 0)} />
        {:else if when && whenKind === "param"}
          <select class="input h-8 w-auto text-xs" value={typeof when.param === "string" ? when.param : ""} aria-label="Variable" onchange={(e) => setWhenField("param", e.currentTarget.value)}>
            {#each params2 as p (p.name)}<option value={String(p.name)}>{String(p.name)}</option>{/each}
          </select>
          <select class="input h-8 w-auto text-xs" value={when.not_equals !== undefined ? "not_equals" : "equals"} aria-label="Comparison" onchange={(e) => setWhenField(e.currentTarget.value, String(when.equals ?? when.not_equals ?? ""), ["equals", "not_equals"])}>
            <option value="equals">is</option><option value="not_equals">is not</option>
          </select>
          <input class="input h-8 w-32 text-xs" value={String(when.equals ?? when.not_equals ?? "")} aria-label="Value" onchange={(e) => setWhenField(when.not_equals !== undefined ? "not_equals" : "equals", e.currentTarget.value)} />
        {/if}
      </div>
    </div>

    <div class="grid gap-3 sm:grid-cols-3">
      <label class="block text-xs"><span class="mb-1 block font-medium">If it fails</span>
        <select class="input h-8 w-full text-xs" value={typeof step.on_error === "string" ? step.on_error : ""} onchange={(e) => set("on_error", e.currentTarget.value || undefined)}>
          <option value="">Runbook default</option><option value="stop">Stop this host</option><option value="continue">Carry on</option>
        </select>
      </label>
      <label class="block text-xs"><span class="mb-1 block font-medium">Timeout (seconds)</span>
        <input class="input h-8 w-full text-xs" inputmode="numeric" placeholder="default" value={typeof step.timeout_secs === "number" ? String(step.timeout_secs) : ""} onchange={(e) => set("timeout_secs", number(e.currentTarget.value))} />
      </label>
      {#if kind !== "wait" && kind !== "delay"}
        <label class="block text-xs"><span class="mb-1 block font-medium">Retries (0–5)</span>
          <input class="input h-8 w-full text-xs" inputmode="numeric" placeholder="none" value={typeof step.retries === "number" ? String(step.retries) : ""} onchange={(e) => set("retries", number(e.currentTarget.value))} />
        </label>
      {/if}
    </div>
    {#if typeof step.retries === "number" && step.retries > 0}
      <p class="text-[11px] text-warning">A failed step is run again. Only use retries for steps that are safe to repeat; nothing checks that.</p>
    {/if}
  </fieldset>
</section>
