<script lang="ts">
  import { Radar } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type Uuid } from "$lib/types";
  import type { ScanReport } from "$lib/monitoring";

  let { id }: { id: Uuid } = $props();

  // svelte-ignore state_referenced_locally
  const rec = monitoring.targets.find((t) => t.id === id);
  const target = rec?.data;
  let ports = $state("common");
  let busy = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let report = $state<ScanReport | null>(null);
  let picked = $state<number[]>([]);

  /** Ports this host already has a port check for. */
  const watched = $derived(new Set((monitoring.targets.find((t) => t.id === id)?.data?.checks ?? []).flatMap((c) => (c.kind === "tcp" ? [c.port] : []))));

  async function scan(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = null;
    report = null;
    try {
      report = await api.monitoring.scanPorts(id, ports);
      picked = report.open.filter((p) => !watched.has(p.port)).map((p) => p.port);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  function toggle(port: number) {
    picked = picked.includes(port) ? picked.filter((p) => p !== port) : [...picked, port];
  }

  /** Watch the chosen open ports: each becomes a port check on this host. */
  async function addChecks() {
    const current = monitoring.targets.find((t) => t.id === id);
    if (!current?.data) return;
    saving = true;
    error = null;
    try {
      const next = $state.snapshot(current.data);
      for (const port of picked) if (!watched.has(port)) next.checks.push({ kind: "tcp", port });
      await monitoring.saveTarget(id, current.rev, next);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={`Scan ports: ${target?.label ?? "host"}`} onclose={() => (ui.modal = null)} width="max-w-xl">
  <form id="scan-form" onsubmit={scan} class="space-y-3">
    <p class="text-sm text-fg-muted">
      Tries to connect to each port on <span class="font-mono">{target?.address}</span>, like <span class="font-mono">nc -z</span>, and lists the ones that answer. Nothing is sent
      on a connection that opens. Only scan hosts you are allowed to.
    </p>
    <div>
      <label class="label" for="scan-ports">Ports</label>
      <input id="scan-ports" class="input font-mono text-xs" bind:value={ports} placeholder="common, or 22, 80, 8000-8100" spellcheck="false" />
      <p class="mt-1 text-xs text-fg-muted"><span class="font-mono">common</span> is about 30 well-known ports. At most 1024 ports at a time.</p>
    </div>
    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
    {#if busy}
      <p class="text-sm text-fg-muted">Scanning…</p>
    {/if}
    {#if report}
      <div class="rounded-lg border border-line">
        <div class="border-b border-line px-3 py-2 text-xs text-fg-muted">
          {report.open.length} of {report.scanned} ports open, in {(report.took_ms / 1000).toFixed(1)} s
        </div>
        {#if report.open.length === 0}
          <p class="px-3 py-3 text-sm text-fg-muted">Nothing answered.</p>
        {:else}
          <ul class="divide-y divide-line">
            {#each report.open as p (p.port)}
              <li class="flex items-center gap-3 px-3 py-1.5 text-sm">
                {#if watched.has(p.port)}
                  <span class="w-4 text-center text-success" title="Already checked">✓</span>
                {:else}
                  <input type="checkbox" class="accent-input" checked={picked.includes(p.port)} onchange={() => toggle(p.port)} aria-label="Watch port {p.port}" />
                {/if}
                <span class="w-14 font-mono">{p.port}</span>
                <span class="flex-1 text-fg-muted">{p.service ?? ""}</span>
                <span class="font-mono text-xs text-fg-muted">{p.latency_ms} ms</span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Close</button>
    {#if report && picked.length}
      <button class="btn-secondary" type="button" disabled={saving || busy} onclick={() => void addChecks()}>Watch {picked.length} open port{picked.length === 1 ? "" : "s"}</button>
    {/if}
    <button class="btn-primary" type="submit" form="scan-form" disabled={busy}><Radar size={14} /> {busy ? "Scanning…" : "Scan"}</button>
  {/snippet}
</Modal>
