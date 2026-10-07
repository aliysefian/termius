<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import { ClipboardCopy, Pause, Play, Trash2, X } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import * as api from "$lib/api";
  import { LineAssembler, LogBuffer, NO_FILTER, compileText, highlights, matches, type LogLine } from "$lib/ops/logs";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { session, context, namespace, pod, containers, onclose }: { session: Uuid; context: string; namespace: string; pod: string; containers: string[]; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let container = $state(containers[0] ?? "");
  let tail = $state(200);
  let timestamps = $state(false);
  let previous = $state(false);
  let text = $state("");
  let paused = $state(false);
  let frozen = $state<LogLine[]>([]);
  let note = $state("");
  let lines = $state<LogLine[]>([]);

  const buffer = new LogBuffer();
  let assembler = new LineAssembler();
  let stream: Uuid | null = null;
  let generation = 0;
  let queued = false;

  const re = $derived(compileText({ text, regex: false }));
  const shown = $derived((paused ? frozen : lines).filter((l) => matches(NO_FILTER, l, re instanceof Error ? null : re)));
  const visible = $derived(shown.length > 2000 ? shown.slice(shown.length - 2000) : shown);

  function flush() {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      lines = buffer.lines;
    });
  }

  async function start() {
    const mine = ++generation;
    await stop();
    buffer.clear();
    lines = [];
    assembler = new LineAssembler();
    note = "";
    try {
      const id = await api.kube.logs(session, context, namespace, pod, container || null, { tail, follow: true, timestamps, previous }, (e) => {
        if (mine !== generation) return;
        if (e.event === "chunk") {
          const got = assembler.push(e.text);
          if (got.length) {
            buffer.add(pod, pod, got);
            flush();
          }
        } else {
          const rest = assembler.finish();
          if (rest.length) buffer.add(pod, pod, rest);
          flush();
          note = e.error ?? (previous ? "end of the previous run's log" : e.code ? `ended (exit ${e.code})` : "ended");
        }
      });
      if (mine === generation) stream = id;
      else void api.kube.stop(id);
    } catch (e) {
      note = errorMessage(e);
    }
  }

  async function stop() {
    const id = stream;
    stream = null;
    if (id) await api.kube.stop(id).catch(() => {});
  }

  $effect(() => {
    // Restart when what is followed changes.
    container;
    tail;
    timestamps;
    previous;
    void start();
  });
  onDestroy(() => {
    generation++;
    void stop();
  });

  let scroller = $state<HTMLDivElement>();
  let stick = $state(true);
  $effect(() => {
    visible.length;
    if (stick && !paused && scroller) void tick().then(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  function parts(line: string): { text: string; hit: boolean }[] {
    const spans = highlights(line, re instanceof Error ? null : re);
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

  async function copy() {
    try {
      await writeText(visible.map((l) => l.text).join("\n"));
      ui.notify("info", `Copied ${visible.length} lines.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  const levelClass = { error: "text-danger", warn: "text-warning", info: "text-fg", debug: "text-fg-muted" } as const;
</script>

<div class="overflow-hidden rounded-xl border border-line bg-panel">
  <div class="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
    <span class="mr-1 text-sm font-semibold">Logs · {pod}</span>
    {#if containers.length > 1}
      <select class="input w-36 py-1 text-xs" bind:value={container} aria-label="Container">
        {#each containers as c (c)}<option value={c}>{c}</option>{/each}
      </select>
    {/if}
    <select class="input w-28 py-1 text-xs" bind:value={tail} aria-label="Lines to start with">
      {#each [50, 200, 1000, 5000] as n (n)}<option value={n}>last {n}</option>{/each}
    </select>
    <label class="flex items-center gap-1 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={timestamps} /> times</label>
    <label class="flex items-center gap-1 text-xs text-fg-muted" title="The log of the container before it last restarted"><input type="checkbox" class="accent-input" bind:checked={previous} /> previous run</label>
    <input class="input w-44 py-1 text-xs" bind:value={text} placeholder="Filter lines…" aria-label="Filter lines" />
    <span class="ml-auto flex items-center gap-1">
      {#if note}<span class="mr-1 max-w-72 truncate text-[11px] text-fg-muted" title={note}>{note}</span>{/if}
      <button class="icon-btn" title={paused ? "Resume" : "Pause the view"} aria-pressed={paused} onclick={() => { paused = !paused; if (paused) frozen = lines; }}>{#if paused}<Play size={15} />{:else}<Pause size={15} />{/if}</button>
      <button class="icon-btn" title="Copy the lines shown" disabled={!visible.length} onclick={() => void copy()}><ClipboardCopy size={15} /></button>
      <button class="icon-btn" title="Clear" onclick={() => { buffer.clear(); lines = []; }}><Trash2 size={15} /></button>
      <button class="icon-btn" title="Close" aria-label="Close the log" onclick={onclose}><X size={15} /></button>
    </span>
  </div>
  <div bind:this={scroller} onscroll={() => scroller && (stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40)} class="h-72 overflow-auto bg-base px-3 py-2 font-mono text-xs leading-5" role="log" aria-label="Pod log">
    {#each visible as l (l.id)}
      <div class="whitespace-pre-wrap break-all {levelClass[l.level]}">{#each parts(l.text) as p, i (i)}{#if p.hit}<mark class="rounded-sm bg-accent/30 text-inherit">{p.text}</mark>{:else}{p.text}{/if}{/each}</div>
    {:else}
      <p class="py-8 text-center text-fg-muted">{note || "Waiting for lines…"}</p>
    {/each}
  </div>
</div>
