<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { Pause, Play, Square, Trash2, ClipboardCopy, Download, Loader2 } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import EmptyState from "./EmptyState.svelte";
  import * as api from "$lib/api";
  import { LineAssembler, LogBuffer, NO_FILTER, PRIORITIES, compileText, highlights, logScript, matches, sourceError, type Level, type LogLine, type LogSource, type Priority } from "$lib/ops/logs";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  let { preset = null }: { preset?: { hostId: Uuid; unit: string } | null } = $props();

  // -- what to follow ---------------------------------------------------------------------------
  let picked = $state<Uuid[]>([]);
  let kind = $state<"journal" | "file">("journal");
  let unit = $state("");
  let priority = $state<Priority | "">("");
  let kernel = $state(false);
  let path = $state("/var/log/syslog");
  let tail = $state(100);

  // A "Logs" button in the service manager lands here with a host and a unit already chosen.
  $effect(() => {
    if (preset) {
      picked = [preset.hostId];
      kind = "journal";
      unit = preset.unit;
    }
  });

  const hosts = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.canMonitor(h.id)).sort((a, b) => a.data!.label.localeCompare(b.data!.label)));
  const source = $derived<LogSource>(
    kind === "file" ? { kind: "file", path } : { kind: "journal", unit: unit.trim() || undefined, priority: priority || undefined, kernel: kernel || undefined },
  );
  const sourceProblem = $derived(sourceError(source));

  // -- running -----------------------------------------------------------------------------------
  type Run = { hostId: Uuid; label: string; session: Uuid | null; stream: Uuid | null; state: "connecting" | "following" | "ended" | "error"; message: string; assembler: LineAssembler };
  let runs = $state<Run[]>([]);
  const buffer = new LogBuffer();
  let hideSecrets = $state(true);
  $effect(() => {
    buffer.mask = hideSecrets;
  });
  let lines = $state<LogLine[]>([]);
  let pendingFlush = false;

  function flush() {
    if (pendingFlush) return;
    pendingFlush = true;
    // At most once a frame, however fast the logs arrive.
    requestAnimationFrame(() => {
      pendingFlush = false;
      lines = buffer.lines;
    });
  }

  function addLines(run: Run, texts: string[]) {
    if (!texts.length) return;
    buffer.add(run.hostId, run.label, texts);
    flush();
  }

  const running = $derived(runs.some((r) => r.state === "connecting" || r.state === "following"));

  async function start() {
    if (sourceProblem || !picked.length) return;
    await stop();
    const script = logScript(source, tail);
    runs = picked.map((id) => ({ hostId: id, label: vaultStore.hostById.get(id)?.data?.label ?? "host", session: null, stream: null, state: "connecting", message: "", assembler: new LineAssembler() }));
    await Promise.all(runs.map((_, i) => begin(i, script)));
  }

  async function begin(i: number, script: string) {
    const run = runs[i];
    try {
      const session = await api.monitor.open(run.hostId);
      run.session = session;
      run.stream = await api.monitor.streamStart(session, script, (e) => {
        if (e.event === "chunk") addLines(run, run.assembler.push(e.text));
        else {
          addLines(run, run.assembler.finish());
          run.state = e.error ? "error" : "ended";
          run.message = e.error ?? (e.code ? `ended (exit ${e.code})` : "ended");
        }
      });
      if (run.state === "connecting") run.state = "following";
    } catch (e) {
      run.state = "error";
      run.message = errorMessage(e);
    }
  }

  async function stop() {
    const old = runs;
    runs = [];
    await Promise.all(
      old.map(async (r) => {
        if (r.stream) await api.monitor.streamStop(r.stream).catch(() => {});
        if (r.session) await api.monitor.close(r.session).catch(() => {});
      }),
    );
  }

  onDestroy(() => void stop());

  // -- looking at it ---------------------------------------------------------------------------------
  let text = $state("");
  let regex = $state(false);
  let levels = $state<Level[]>([]);
  let hostFilter = $state<Uuid[]>([]);
  let paused = $state(false);
  let wrap = $state(true);
  let frozen = $state<LogLine[]>([]);

  const filter = $derived({ ...NO_FILTER, text, regex, levels, hosts: hostFilter });
  const compiled = $derived(compileText(filter));
  const bad = $derived(compiled instanceof Error ? compiled.message : null);
  const re = $derived(compiled instanceof Error ? null : compiled);
  const source_lines = $derived(paused ? frozen : lines);
  const shown = $derived(source_lines.filter((l) => matches(filter, l, re)));
  const VISIBLE = 2000;
  const visible = $derived(shown.length > VISIBLE ? shown.slice(shown.length - VISIBLE) : shown);

  function togglePause() {
    paused = !paused;
    if (paused) frozen = lines;
  }

  const colourOf = (hostId: string) => {
    const i = Math.max(0, picked.indexOf(hostId));
    return `hsl(${(i * 67 + 250) % 360} 70% 68%)`;
  };

  let scroller = $state<HTMLDivElement>();
  let stick = $state(true);
  function onScroll() {
    if (!scroller) return;
    stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
  }
  $effect(() => {
    visible.length;
    if (stick && !paused && scroller) void tick().then(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  function parts(line: string): { text: string; hit: boolean }[] {
    const spans = highlights(line, re);
    if (!spans.length) return [{ text: line, hit: false }];
    const out: { text: string; hit: boolean }[] = [];
    let at = 0;
    for (const [a, b] of spans) {
      if (a > at) out.push({ text: line.slice(at, a), hit: false });
      out.push({ text: line.slice(a, b), hit: true });
      at = b;
    }
    if (at < line.length) out.push({ text: line.slice(at), hit: false });
    return out;
  }

  const levelClass: Record<Level, string> = { error: "text-danger", warn: "text-warning", info: "text-fg", debug: "text-fg-muted" };

  async function copyVisible() {
    try {
      await writeText(visible.map((l) => `${picked.length > 1 ? `[${l.host}] ` : ""}${l.text}`).join("\n"));
      ui.notify("info", `Copied ${visible.length} lines.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function saveVisible() {
    if (!visible.length) return;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ title: "Save the lines shown", defaultPath: "sshvault-log.log", filters: [{ name: "Log", extensions: ["log"] }] });
      if (!path) return;
      await api.exportTextFile(path, visible.map((l) => `${picked.length > 1 ? `[${l.host}] ` : ""}${l.text}`).join("\n") + "\n");
      ui.notify("info", `Saved ${visible.length} lines${hideSecrets ? "" : " (secrets not hidden)"}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function clear() {
    buffer.clear();
    lines = [];
    frozen = [];
  }

  const toggle = <T,>(list: T[], v: T): T[] => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
</script>

<div class="space-y-4">
  <div class="rounded-xl border border-line bg-panel p-4">
    <div class="grid gap-4 lg:grid-cols-[1fr_1.4fr]">
      <div>
        <span class="label">Hosts <span class="font-normal normal-case text-fg-muted">({picked.length} chosen)</span></span>
        {#if hosts.length === 0}
          <p class="text-xs text-fg-muted">No host has saved credentials yet. Logs are read over a second connection that never asks for a password.</p>
        {:else}
          <div class="max-h-36 space-y-0.5 overflow-y-auto rounded-md border border-line bg-base p-1.5" role="group" aria-label="Hosts">
            {#each hosts as h (h.id)}
              {@const env = envInfo(vaultStore.effectiveEnv(h.data))}
              <label class="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-panel-hover">
                <input type="checkbox" class="accent-input" checked={picked.includes(h.id)} onchange={() => (picked = toggle(picked, h.id))} />
                <span class="min-w-0 flex-1 truncate">{h.data?.label}</span>
                {#if env.value === "production"}<span class="rounded bg-danger/20 px-1 text-[10px] font-semibold text-danger">PROD</span>{/if}
              </label>
            {/each}
          </div>
        {/if}
      </div>
      <div class="space-y-3">
        <div class="grid grid-cols-3 gap-3">
          <div>
            <label class="label" for="lg-kind">Follow</label>
            <select id="lg-kind" class="input" bind:value={kind}>
              <option value="journal">System journal</option>
              <option value="file">A log file</option>
            </select>
          </div>
          {#if kind === "journal"}
            <div>
              <label class="label" for="lg-unit">Unit <span class="font-normal normal-case text-fg-muted">(optional)</span></label>
              <input id="lg-unit" class="input font-mono" bind:value={unit} placeholder="nginx.service" spellcheck="false" />
            </div>
            <div>
              <label class="label" for="lg-prio">Priority and above</label>
              <select id="lg-prio" class="input" bind:value={priority}>
                <option value="">Everything</option>
                {#each PRIORITIES as p (p)}<option value={p}>{p}</option>{/each}
              </select>
            </div>
          {:else}
            <div class="col-span-2">
              <label class="label" for="lg-path">Path on the host</label>
              <input id="lg-path" class="input font-mono" bind:value={path} placeholder="/var/log/nginx/error.log" spellcheck="false" />
            </div>
          {/if}
        </div>
        <div class="flex flex-wrap items-end gap-3">
          <div class="w-28">
            <label class="label" for="lg-tail">Start with</label>
            <select id="lg-tail" class="input" bind:value={tail}>
              {#each [0, 50, 100, 500, 1000] as n (n)}<option value={n}>{n === 0 ? "new lines only" : `last ${n}`}</option>{/each}
            </select>
          </div>
          {#if kind === "journal"}
            <label class="flex items-center gap-1.5 pb-2 text-xs"><input type="checkbox" class="accent-input" bind:checked={kernel} /> Kernel only</label>
          {/if}
          <div class="ml-auto flex items-center gap-2 pb-0.5">
            {#if running}
              <button class="btn-secondary" onclick={() => void stop()}><Square size={14} /> Stop</button>
            {:else}
              <button class="btn-primary" disabled={!!sourceProblem || !picked.length} onclick={() => void start()}><Play size={14} /> Follow</button>
            {/if}
          </div>
        </div>
        {#if sourceProblem && (unit || kind === "file")}<p class="text-xs text-danger">{sourceProblem}</p>{/if}
      </div>
    </div>
    {#if runs.length}
      <div class="mt-3 flex flex-wrap gap-1.5 border-t border-line pt-3">
        {#each runs as r (r.hostId)}
          <span class="flex items-center gap-1.5 rounded-full border border-line bg-base px-2 py-0.5 text-[11px]" title={r.message}>
            <span class="h-2 w-2 rounded-full" style:background={colourOf(r.hostId)}></span>
            {r.label}
            {#if r.state === "connecting"}<Loader2 size={10} class="animate-spin text-fg-muted" />
            {:else if r.state === "following"}<span class="text-success">following</span>
            {:else if r.state === "error"}<span class="max-w-48 truncate text-danger">{r.message}</span>
            {:else}<span class="text-fg-muted">{r.message || "ended"}</span>{/if}
          </span>
        {/each}
      </div>
    {/if}
  </div>

  <div class="overflow-hidden rounded-xl border border-line bg-panel">
    <div class="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
      <input class="input w-56 py-1 text-xs" bind:value={text} placeholder="Filter lines…" aria-label="Filter lines" spellcheck="false" />
      <label class="flex items-center gap-1 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={regex} /> regex</label>
      {#if bad}<span class="text-xs text-danger">{bad}</span>{/if}
      <div class="flex gap-1" role="group" aria-label="Levels">
        {#each ["error", "warn", "info", "debug"] as l (l)}
          <button
            class="rounded-md border px-2 py-0.5 text-[11px] capitalize {levels.includes(l as Level) ? 'border-accent bg-accent/15 text-accent' : 'border-line text-fg-muted hover:text-fg'}"
            aria-pressed={levels.includes(l as Level)}
            onclick={() => (levels = toggle(levels, l as Level))}>{l}</button
          >
        {/each}
      </div>
      {#if runs.length > 1}
        <select class="input w-36 py-1 text-xs" aria-label="Show one host" onchange={(e) => (hostFilter = e.currentTarget.value ? [e.currentTarget.value] : [])}>
          <option value="">All hosts</option>
          {#each runs as r (r.hostId)}<option value={r.hostId}>{r.label}</option>{/each}
        </select>
      {/if}
      <div class="ml-auto flex items-center gap-1">
        <span class="mr-1 text-[11px] text-fg-muted">{shown.length}{shown.length !== source_lines.length ? ` of ${source_lines.length}` : ""} lines</span>
        <label class="mr-1 flex items-center gap-1 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={wrap} /> wrap</label>
        <label class="mr-1 flex items-center gap-1 text-xs text-fg-muted" title="Hides passwords, tokens, keys and credentials in lines that arrive from now on. It recognises common shapes only; it is not a guarantee."><input type="checkbox" class="accent-input" bind:checked={hideSecrets} /> hide secrets</label>
        <button class="icon-btn" title={paused ? "Resume scrolling" : "Pause the view (lines keep arriving)"} aria-pressed={paused} onclick={togglePause}>
          {#if paused}<Play size={15} />{:else}<Pause size={15} />{/if}
        </button>
        <button class="icon-btn" title="Copy the lines shown" disabled={!visible.length} onclick={() => void copyVisible()}><ClipboardCopy size={15} /></button>
        <button class="icon-btn" title="Save the lines shown to a .log file" disabled={!visible.length} onclick={() => void saveVisible()}><Download size={15} /></button>
        <button class="icon-btn" title="Clear" disabled={!lines.length} onclick={clear}><Trash2 size={15} /></button>
      </div>
    </div>
    <div bind:this={scroller} onscroll={onScroll} class="h-[26rem] overflow-auto bg-base px-3 py-2 font-mono text-xs leading-5" role="log" aria-live="off" aria-label="Log lines">
      {#if lines.length === 0}
        <div class="flex h-full items-center justify-center">
          <EmptyState art="terminal" text={running ? "Waiting for lines…" : "Choose hosts and a source, then Follow."} />
        </div>
      {:else if visible.length === 0}
        <p class="py-8 text-center text-fg-muted">No line matches the filter.</p>
      {:else}
        {#each visible as l (l.id)}
          <div class="flex gap-2 {wrap ? 'whitespace-pre-wrap break-all' : 'whitespace-pre'} {levelClass[l.level]}">
            {#if picked.length > 1}<span class="w-24 shrink-0 truncate" style:color={colourOf(l.hostId)} title={l.host}>{l.host}</span>{/if}
            <span class="min-w-0">{#each parts(l.text) as p, i (i)}{#if p.hit}<mark class="rounded-sm bg-accent/30 text-inherit">{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>
