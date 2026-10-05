<script lang="ts">
  import { onDestroy } from "svelte";
  import { Download, Square } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { LogBuffer, isPullable } from "$lib/containerdata";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type ContainerLogEvent, type Uuid } from "$lib/types";

  let { sourceKey }: { sourceKey: string } = $props();

  const buffer = new LogBuffer(5000);
  let reference = $state("");
  let running = $state(false);
  let lines = $state<string[]>([]);
  let note = $state<{ ok: boolean; text: string } | null>(null);
  let streamId: Uuid | null = null;
  let generation = 0;
  let pane = $state<HTMLPreElement>();

  const valid = $derived(isPullable(reference));

  function onEvent(gen: number, e: ContainerLogEvent, ref: string) {
    if (gen !== generation) return;
    if (e.event === "chunk") {
      buffer.push(e.text);
      // The last lines are what matters; the full text of a big pull is hundreds of layer lines.
      lines = buffer.all().slice(-300);
      queueMicrotask(() => pane && (pane.scrollTop = pane.scrollHeight));
      return;
    }
    running = false;
    streamId = null;
    if (e.error) note = { ok: false, text: e.error };
    else if (e.code === 0) {
      note = { ok: true, text: `Pulled ${ref}.` };
      ui.notify("info", `Pulled ${ref}.`);
      void containers.refresh(sourceKey);
    } else note = { ok: false, text: e.code === null ? "Stopped." : `The pull failed (status ${e.code}). The reason is above.` };
  }

  async function start(e?: SubmitEvent) {
    e?.preventDefault();
    const src = containers.sources[sourceKey];
    const ref = reference.trim();
    if (!src || !valid || running) return;
    const gen = ++generation;
    buffer.clear();
    lines = [];
    note = null;
    running = true;
    try {
      streamId = await api.containers.pull(src.sessionId, src.runtime, ref, (ev) => onEvent(gen, ev, ref));
      if (gen !== generation) await api.containers.logsStop(streamId).catch(() => {});
    } catch (err) {
      running = false;
      note = { ok: false, text: errorMessage(err) };
    }
  }

  async function stop() {
    generation++;
    const s = streamId;
    streamId = null;
    running = false;
    note = { ok: false, text: "Stopped." };
    if (s) await api.containers.logsStop(s).catch(() => {});
  }

  onDestroy(() => {
    generation++;
    if (streamId) void api.containers.logsStop(streamId).catch(() => {});
  });
</script>

<Modal title="Pull an image" onclose={() => (ui.modal = null)} width="max-w-2xl">
  <form onsubmit={start} class="space-y-3">
    <div>
      <label class="label" for="pull-ref">Image</label>
      <div class="flex gap-2">
        <!-- svelte-ignore a11y_autofocus -->
        <input id="pull-ref" class="input flex-1 font-mono" bind:value={reference} placeholder="nginx:1.27, ghcr.io/org/app:v2, postgres@sha256:…" autocomplete="off" spellcheck="false" autofocus disabled={running} />
        {#if running}
          <button class="btn-danger border border-danger/40" type="button" onclick={stop}><Square size={13} /> Stop</button>
        {:else}
          <button class="btn-primary" type="submit" disabled={!valid}><Download size={13} /> Pull</button>
        {/if}
      </div>
      <p class="mt-1 text-xs text-fg-muted">
        Pulls onto {containers.sources[sourceKey]?.label ?? "this source"}. Without a tag, the runtime pulls <code>latest</code>.
        {#if reference.trim() && !valid}<span class="text-danger">That isn't a plain image reference.</span>{/if}
      </p>
    </div>
    {#if lines.length || running}
      <pre bind:this={pane} class="h-64 overflow-auto rounded-md border border-line bg-base p-3 font-mono text-xs leading-5" aria-label="Pull progress" aria-live="off">{lines.join("\n")}{#if running && !lines.length}Starting…{/if}</pre>
    {/if}
    {#if note}
      <p class="rounded-md border px-3 py-2 text-sm {note.ok ? 'border-success/30 bg-success/10 text-success' : 'border-danger/30 bg-danger/10 text-danger'}" role={note.ok ? "status" : "alert"}>{note.text}</p>
    {/if}
  </form>
</Modal>
