<script lang="ts">
  import { onDestroy } from "svelte";
  import { ChevronDown, ChevronRight, CircleStop, Copy, Loader2, Play, Search, X } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { hostContextFor } from "$lib/runsnippet";
  import { ask } from "$lib/dialogs.svelte";
  import { promptedVariables, render } from "$lib/snippetvars";
  import { lastValues } from "$lib/snippetvalues";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type ExecOutput, type Uuid } from "$lib/types";

  let { command: initial = "" }: { command?: string } = $props();

  // svelte-ignore state_referenced_locally
  let command = $state(initial);
  let picked = $state<Set<Uuid>>(new Set());
  let filter = $state("");
  let timeoutSecs = $state(60);
  /** "all": every host at once. "rolling": one at a time. "rolling-stop": one at a time, and stop at the first failure. */
  let pace = $state<"all" | "rolling" | "rolling-stop">("all");
  let values = $state<Record<string, string>>({});

  type Result =
    | { state: "queued" }
    | { state: "running" }
    | { state: "done"; output: ExecOutput }
    | { state: "failed"; message: string }
    | { state: "skipped" }
    | { state: "cancelled" };
  let runId = $state<string | null>(null);
  let running = $state(false);
  let results = $state<Record<Uuid, Result>>({});
  let expanded = $state<Set<Uuid>>(new Set());

  const names = $derived(promptedVariables(command));
  $effect(() => {
    for (const n of names) if (!(n in values)) values[n] = lastValues.get(n) ?? "";
  });

  const hosts = $derived(
    vaultStore.hosts.filter((h) => {
      const d = h.data;
      if (!d) return false;
      const q = filter.trim().toLowerCase();
      return !q || [d.label, d.hostname, d.group, ...d.tags].some((s) => s.toLowerCase().includes(q));
    }),
  );
  const unattended = (id: Uuid) => !!vaultStore.effectiveIdentity(vaultStore.hostById.get(id)?.data);

  function toggle(id: Uuid) {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    picked = next;
  }

  async function run() {
    const ids = [...picked];
    if (!command.trim() || ids.length === 0) return;
    const prod = ids
      .map((h) => vaultStore.hostById.get(h)?.data)
      .filter((d) => d?.environment === "production")
      .map((d) => d!.label);
    if (prod.length && !await ask(`This will run on ${prod.length} production host(s):\n\n${prod.join("\n")}\n\nRun anyway?`)) return;
    for (const [k, v] of Object.entries(values)) lastValues.set(k, v);
    const id = crypto.randomUUID();
    runId = id;
    running = true;
    results = Object.fromEntries(ids.map((h) => [h, { state: "queued" } as Result]));
    const jobs = ids.map((h) => ({ host_id: h, command: render(command, hostContextFor(h), $state.snapshot(values)) }));
    try {
      await api.runs.start(id, jobs, timeoutSecs, (e) => {
        if (runId !== id) return;
        if (e.event === "started") results[e.host_id] = { state: "running" };
        else if (e.event === "finished") results[e.host_id] = { state: "done", output: e.output };
        else if (e.event === "failed") results[e.host_id] = { state: "failed", message: e.message };
        else if (e.event === "skipped") results[e.host_id] = { state: "skipped" };
        else running = false;
      }, pace === "all" ? "parallel" : { sequential: { stop_on_failure: pace === "rolling-stop" } });
    } catch (err) {
      running = false;
      ui.notify("error", errorMessage(err));
    }
  }

  async function cancel() {
    if (!runId) return;
    await api.runs.cancel(runId);
    running = false;
    for (const [h, r] of Object.entries(results)) {
      if (r.state === "queued" || r.state === "running") results[h] = { state: "cancelled" };
    }
  }

  async function closeWindow() {
    if (running && !(await ask("Close this window and cancel the commands still running?", { title: "Commands are still running", confirm: "Cancel and close", danger: true }))) return;
    ui.modal = null;
  }

  onDestroy(() => {
    if (running && runId) void api.runs.cancel(runId);
  });

  const summary = $derived.by(() => {
    const rs = Object.values(results);
    return {
      ok: rs.filter((r) => r.state === "done" && r.output.exit_code === 0).length,
      bad: rs.filter((r) => r.state === "failed" || (r.state === "done" && r.output.exit_code !== 0)).length,
      total: rs.length,
    };
  });

  async function copy(text: string) {
    await writeText(text);
    ui.notify("info", "Output copied.");
  }

  function toggleExpand(id: Uuid) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }
</script>

<Modal
  title="Run on hosts"
  onclose={() => (ui.modal = null)}
  width="max-w-3xl"
  confirmClose={running ? { title: "Commands are still running", message: "Close this window and cancel the commands still running?", confirm: "Cancel and close" } : null}
>
  <div class="grid gap-4 md:grid-cols-[1fr_16rem]">
    <div class="min-w-0 space-y-3">
      <div>
        <label class="label" for="run-cmd">Command</label>
        <textarea
          id="run-cmd"
          class="input font-mono text-xs"
          rows="3"
          bind:value={command}
          spellcheck="false"
          placeholder={"uptime && df -h /  ({{host}}, {{user}} and your own {{names}} work here)"}
          disabled={running}
        ></textarea>
      </div>
      {#if names.length}
        <div class="grid grid-cols-2 gap-2">
          {#each names as n (n)}
            <div>
              <label class="label" for="rv-{n}">{n}</label>
              <input id="rv-{n}" class="input py-1.5 font-mono text-xs" bind:value={values[n]} disabled={running} />
            </div>
          {/each}
        </div>
      {/if}

      {#if Object.keys(results).length}
        <div class="flex items-center justify-between text-xs text-fg-muted">
          <span>{summary.ok} succeeded · {summary.bad} failed · {summary.total} hosts</span>
          {#if running}<span class="flex items-center gap-1"><Loader2 size={12} class="animate-spin" /> Running…</span>{/if}
        </div>
        <div class="max-h-80 divide-y divide-line overflow-y-auto rounded-md border border-line">
          {#each Object.entries(results) as [hid, r] (hid)}
            {@const h = vaultStore.hostById.get(hid)?.data}
            {@const open = expanded.has(hid)}
            <div>
              <button class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-panel-hover" onclick={() => toggleExpand(hid)}>
                {#if r.state === "done" || r.state === "failed"}
                  {#if open}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
                {:else}
                  <span class="w-[13px]"></span>
                {/if}
                <span class="min-w-0 flex-1 truncate">{h?.label ?? hid}</span>
                {#if r.state === "queued"}
                  <span class="text-xs text-fg-muted">Queued</span>
                {:else if r.state === "running"}
                  <Loader2 size={13} class="animate-spin text-accent" />
                {:else if r.state === "done"}
                  <span class="text-xs {r.output.exit_code === 0 ? 'text-success' : 'text-danger'}">
                    exit {r.output.exit_code ?? "?"} · {(r.output.duration_ms / 1000).toFixed(1)}s
                  </span>
                {:else if r.state === "failed"}
                  <span class="text-xs text-danger">Failed</span>
                {:else if r.state === "skipped"}
                  <span class="text-xs text-fg-muted">Skipped: an earlier host failed</span>
                {:else}
                  <span class="text-xs text-fg-muted">Cancelled</span>
                {/if}
              </button>
              {#if open && r.state === "done"}
                <div class="relative bg-base px-3 py-2">
                  <button class="icon-btn absolute right-2 top-2 h-6 w-6" title="Copy output" onclick={() => copy(r.output.stdout + r.output.stderr)}><Copy size={12} /></button>
                  <pre class="max-h-60 overflow-auto whitespace-pre-wrap font-mono text-xs">{r.output.stdout}{#if r.output.stderr}<span class="text-danger">{r.output.stderr}</span>{/if}</pre>
                  {#if r.output.truncated}<p class="mt-1 text-xs text-fg-muted">Output was cut off at 256 KB per stream.</p>{/if}
                </div>
              {:else if open && r.state === "failed"}
                <p class="bg-base px-3 py-2 text-xs text-danger">{r.message}</p>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div class="flex min-h-0 flex-col">
      <div class="mb-1 flex items-center justify-between">
        <span class="label mb-0">Hosts ({picked.size})</span>
        <div class="flex gap-1 text-xs">
          <button class="text-fg-muted hover:text-fg" disabled={running} onclick={() => (picked = new Set(hosts.filter((h) => unattended(h.id)).map((h) => h.id)))}>All</button>
          <span class="text-fg-muted">·</span>
          <button class="text-fg-muted hover:text-fg" disabled={running} onclick={() => (picked = new Set())}>None</button>
        </div>
      </div>
      <div class="relative mb-1">
        <Search size={12} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
        <input class="input py-1 pl-7 text-xs" placeholder="Filter" bind:value={filter} />
      </div>
      <div class="max-h-80 min-h-40 flex-1 overflow-y-auto rounded-md border border-line py-1">
        {#each hosts as h (h.id)}
          {@const ok = unattended(h.id)}
          <label class="flex items-center gap-2 px-2 py-1 text-sm {ok ? 'hover:bg-panel-hover' : 'opacity-50'}" title={ok ? "" : "Needs saved credentials to run unattended"}>
            <input type="checkbox" class="accent-input" checked={picked.has(h.id)} disabled={!ok || running} onchange={() => toggle(h.id)} />
            <span class="min-w-0 flex-1 truncate">{h.data?.label}</span>
            {#if h.data?.group}<span class="truncate text-[11px] text-fg-muted">{h.data.group}</span>{/if}
          </label>
        {:else}
          <p class="px-2 py-4 text-center text-xs text-fg-muted">No hosts.</p>
        {/each}
      </div>
      <label class="mt-2 flex items-center justify-between gap-2 text-xs text-fg-muted">
        Hosts
        <select class="input w-44 py-1 text-xs" bind:value={pace} disabled={running} aria-label="How to take the hosts">
          <option value="all">All at once</option>
          <option value="rolling">One at a time</option>
          <option value="rolling-stop">One at a time, stop at the first failure</option>
        </select>
      </label>
      <label class="mt-2 flex items-center justify-between text-xs text-fg-muted">
        Timeout per host
        <select class="input w-24 py-1 text-xs" bind:value={timeoutSecs} disabled={running}>
          {#each [10, 30, 60, 300, 900] as t (t)}<option value={t}>{t < 60 ? `${t}s` : `${t / 60} min`}</option>{/each}
        </select>
      </label>
    </div>
  </div>
  {#snippet footer()}
    <button class="btn-ghost" onclick={closeWindow}><X size={14} /> Close</button>
    {#if running}
      <button class="btn-danger border border-danger/40" onclick={cancel}><CircleStop size={14} /> Cancel</button>
    {:else}
      <button class="btn-primary" disabled={!command.trim() || picked.size === 0} onclick={run}>
        {#if Object.keys(results).length}<Play size={14} /> Run again{:else}<Play size={14} /> Run on {picked.size || ""} host{picked.size === 1 ? "" : "s"}{/if}
      </button>
    {/if}
  {/snippet}
</Modal>
