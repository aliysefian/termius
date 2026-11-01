<script lang="ts">
  import { HeartPulse, Pencil, Plus, Radar, Trash2 } from "lucide-svelte";
  import EmptyState from "../EmptyState.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import type { MonitorTarget } from "$lib/monitoring";
  import { errorMessage, type VaultRecord } from "$lib/types";

  let busy = $state(false);
  const targets = $derived(monitoring.targets.filter((t) => t.data));

  async function run(f: () => Promise<unknown>) {
    busy = true;
    try {
      await f();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = false;
    }
  }
  async function remove(rec: VaultRecord<MonitorTarget>) {
    if (!(await ask(`Stop watching "${rec.data?.label}"?`, { title: "Remove host", confirm: "Remove", danger: true }))) return;
    await run(() => monitoring.deleteTarget(rec.id, rec.rev));
  }
  const toggle = (rec: VaultRecord<MonitorTarget>) => rec.data && run(() => monitoring.saveTarget(rec.id, rec.rev, { ...$state.snapshot(rec.data!), enabled: !rec.data!.enabled }));
  const checkSummary = (t: MonitorTarget) =>
    t.checks.map((c) => (c.kind === "ping" ? "ping" : c.kind === "tcp" ? `port ${c.port}` : c.kind === "http" ? `site ${c.url.replace(/^https?:\/\//, "").split(/[?#]/)[0]}` : "CPU, memory, disk, network")).join(", ");
  const line = (t: MonitorTarget) => [`Every ${t.interval_secs} s`, checkSummary(t), t.tags.join(", ")].filter(Boolean).join(" · ");
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between gap-3">
    <p class="text-sm text-fg-muted">The hosts to watch. They don't have to be saved SSH hosts; a router or a printer will do.</p>
    <button class="btn-primary shrink-0" data-testid="monitor-new" onclick={() => (ui.modal = { kind: "monitor-target", id: null })}><Plus size={14} /> Add a host</button>
  </div>
  {#if targets.length === 0}
    <EmptyState icon={HeartPulse} text="Nothing to watch yet." />
  {:else}
    <div class="divide-y divide-line rounded-xl border border-line bg-panel">
      {#each targets as rec (rec.id)}
        {@const t = rec.data!}
        <div class="flex items-center gap-3 px-4 py-2.5 {t.enabled ? '' : 'opacity-60'}">
          <input type="checkbox" class="accent-input" checked={t.enabled} disabled={busy} aria-label="Watch {t.label}" onchange={() => void toggle(rec)} />
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-medium">{t.label} <span class="font-mono text-xs font-normal text-fg-muted">{t.address}</span></div>
            <div class="truncate text-xs text-fg-muted">{line(t)}</div>
          </div>
          <button class="icon-btn" title="Scan ports" aria-label="Scan the ports of {t.label}" onclick={() => (ui.modal = { kind: "port-scan", id: rec.id })}><Radar size={14} /></button>
          <button class="icon-btn" title="Edit" aria-label="Edit {t.label}" onclick={() => (ui.modal = { kind: "monitor-target", id: rec.id })}><Pencil size={14} /></button>
          <button class="icon-btn" title="Remove" aria-label="Remove {t.label}" onclick={() => void remove(rec)}><Trash2 size={14} /></button>
        </div>
      {/each}
    </div>
  {/if}
</div>
