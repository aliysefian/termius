<script lang="ts">
  import { onDestroy } from "svelte";
  import { Loader2, RefreshCw, ScrollText, Info } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { DISRUPTIVE, LIST_SCRIPT, VERBS, actionScript, parseUnits, statusScript, toneOf, type Unit, type Verb } from "$lib/ops/services";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  let { onlogs }: { onlogs: (hostId: Uuid, unit: string) => void } = $props();

  let hostId = $state<Uuid | "">("");
  let session: Uuid | null = null;
  let loading = $state(false);
  let error = $state<string | null>(null);
  let systemd = $state(true);
  let units = $state<Unit[]>([]);
  let query = $state("");
  let onlyFailed = $state(false);
  let onlyRunning = $state(false);
  let useSudo = $state(false);
  let busy = $state<string | null>(null);
  let status = $state<{ unit: string; text: string } | null>(null);

  const hosts = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.canMonitor(h.id)).sort((a, b) => a.data!.label.localeCompare(b.data!.label)));
  const host = $derived(hostId ? vaultStore.hostById.get(hostId)?.data : undefined);
  const production = $derived(envInfo(vaultStore.effectiveEnv(host)).value === "production");

  const shown = $derived(
    units.filter((u) => (!query || u.name.toLowerCase().includes(query.toLowerCase()) || u.description.toLowerCase().includes(query.toLowerCase())) && (!onlyFailed || u.active === "failed") && (!onlyRunning || u.sub === "running")),
  );
  const failed = $derived(units.filter((u) => u.active === "failed").length);

  async function run(script: string, timeout = 20) {
    if (!session) throw new Error("Not connected.");
    return api.monitor.exec(session, script, timeout);
  }

  async function connectAndLoad(id: Uuid) {
    loading = true;
    error = null;
    status = null;
    try {
      if (session) await api.monitor.close(session).catch(() => {});
      session = await api.monitor.open(id);
      await load();
    } catch (e) {
      error = errorMessage(e);
      units = [];
    } finally {
      loading = false;
    }
  }

  async function load() {
    const r = await run(LIST_SCRIPT);
    const parsed = parseUnits(r.stdout);
    systemd = parsed.systemd;
    units = parsed.units;
  }

  async function act(u: Unit, verb: Verb) {
    if (!host) return;
    // Taking something down on production means typing the host's name; elsewhere it just happens.
    if (production && DISRUPTIVE.includes(verb)) {
      const ok = await ask(`${verb[0].toUpperCase()}${verb.slice(1)} ${u.name} on ${host.label}? This is a production host.`, { title: "Production host", confirm: verb[0].toUpperCase() + verb.slice(1), danger: true, requireText: host.label });
      if (!ok) return;
    }
    busy = `${u.name}:${verb}`;
    try {
      const r = await run(actionScript(verb, u.name, useSudo), 60);
      if (r.code !== 0) ui.notify("error", `${verb} ${u.name}: ${(r.stdout || r.stderr).trim().split("\n")[0] || `exit ${r.code}`}${useSudo ? "" : ". If it needs root, turn on “Use sudo”."}`);
      else ui.notify("info", `${u.name}: ${verb} done.`);
      await load();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = null;
    }
  }

  async function showStatus(u: Unit) {
    try {
      const r = await run(statusScript(u.name));
      status = { unit: u.name, text: (r.stdout + r.stderr).trim() };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  onDestroy(() => {
    if (session) void api.monitor.close(session).catch(() => {});
  });

  const dot = { good: "bg-success", bad: "bg-danger", muted: "bg-fg-muted/50" } as const;
  /** What makes sense for a unit in its state: a running one can be stopped, restarted or reloaded; a stopped one started. Boot enablement is separate. */
  const verbsFor = (u: Unit): Verb[] => {
    const now: Verb[] = u.active === "active" ? ["restart", "reload", "stop"] : ["start"];
    return [...now, u.enabled === "enabled" ? "disable" : "enable"].filter((v): v is Verb => VERBS.includes(v as Verb));
  };
</script>

<div class="space-y-4">
  <div class="flex flex-wrap items-end gap-3 rounded-xl border border-line bg-panel p-4">
    <div class="min-w-56 flex-1">
      <label class="label" for="sv-host">Host</label>
      <select id="sv-host" class="input" bind:value={hostId} onchange={() => hostId && void connectAndLoad(hostId)}>
        <option value="">Choose a host…</option>
        {#each hosts as h (h.id)}<option value={h.id}>{h.data?.label}</option>{/each}
      </select>
    </div>
    <label class="flex items-center gap-1.5 pb-2 text-xs" title="Runs the action with sudo -n: it works only where sudo needs no password, and fails at once where it does.">
      <input type="checkbox" class="accent-input" bind:checked={useSudo} /> Use sudo (no password prompt)
    </label>
    <button class="btn-secondary" disabled={!hostId || loading} onclick={() => hostId && void connectAndLoad(hostId)}>
      {#if loading}<Loader2 size={14} class="animate-spin" />{:else}<RefreshCw size={14} />{/if} Refresh
    </button>
  </div>

  {#if error}
    <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
  {/if}

  {#if !hostId}
    <div class="rounded-xl border border-line bg-panel p-8"><EmptyState art="hosts" text={hosts.length ? "Choose a host to see its services." : "No host has saved credentials yet; services are read over a second connection."} /></div>
  {:else if !systemd && !loading}
    <div class="rounded-xl border border-line bg-panel p-8"><EmptyState art="security" text="This host doesn't use systemd, so there is nothing to list here." /></div>
  {:else if units.length}
    <div class="overflow-hidden rounded-xl border border-line bg-panel">
      <div class="flex flex-wrap items-center gap-3 border-b border-line px-3 py-2">
        <input class="input w-64 py-1 text-xs" bind:value={query} placeholder="Search services…" aria-label="Search services" />
        <label class="flex items-center gap-1 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={onlyRunning} /> running</label>
        <label class="flex items-center gap-1 text-xs {failed ? 'text-danger' : 'text-fg-muted'}"><input type="checkbox" class="accent-input" bind:checked={onlyFailed} /> failed ({failed})</label>
        <span class="ml-auto text-[11px] text-fg-muted">{shown.length} of {units.length}{production ? " · production: stopping or restarting asks first" : ""}</span>
      </div>
      <div class="max-h-[30rem] overflow-y-auto">
        <table class="w-full text-left text-xs">
          <thead class="sticky top-0 bg-panel text-fg-muted"><tr><th class="px-3 py-1.5 font-medium">Service</th><th class="font-medium">State</th><th class="font-medium">At boot</th><th class="pr-3 text-right font-medium">Actions</th></tr></thead>
          <tbody>
            {#each shown as u (u.name)}
              <tr class="border-t border-line/60 hover:bg-panel-hover/50">
                <td class="max-w-72 px-3 py-1.5">
                  <div class="flex items-center gap-2"><span class="h-2 w-2 shrink-0 rounded-full {dot[toneOf(u)]}"></span><span class="truncate font-medium" title={u.name}>{u.name}</span></div>
                  <div class="truncate pl-4 text-[11px] text-fg-muted" title={u.description}>{u.description}</div>
                </td>
                <td class="whitespace-nowrap {toneOf(u) === 'bad' ? 'text-danger' : ''}">{u.active} ({u.sub})</td>
                <td class="text-fg-muted">{u.enabled || "—"}</td>
                <td class="pr-3 text-right whitespace-nowrap">
                  {#each verbsFor(u) as v (v)}
                    <button class="btn-ghost px-1.5 py-0.5 text-[11px] {DISRUPTIVE.includes(v) ? 'hover:text-danger' : ''}" disabled={busy !== null} onclick={() => void act(u, v)}>
                      {#if busy === `${u.name}:${v}`}<Loader2 size={11} class="animate-spin" />{:else}{v}{/if}
                    </button>
                  {/each}
                  <button class="icon-btn h-6 w-6" title="Status" aria-label="Status of {u.name}" onclick={() => void showStatus(u)}><Info size={13} /></button>
                  <button class="icon-btn h-6 w-6" title="Follow its log" aria-label="Logs of {u.name}" onclick={() => hostId && onlogs(hostId, u.name)}><ScrollText size={13} /></button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
    {#if status}
      <div class="rounded-xl border border-line bg-panel p-3">
        <div class="mb-1 flex items-center justify-between text-xs"><span class="font-medium">{status.unit}</span><button class="btn-ghost py-0.5 text-xs" onclick={() => (status = null)}>Close</button></div>
        <pre class="max-h-64 overflow-auto rounded-md bg-base p-3 font-mono text-[11px] leading-5 whitespace-pre-wrap">{status.text}</pre>
      </div>
    {/if}
  {:else if loading}
    <div class="flex items-center gap-2 py-8 text-sm text-fg-muted"><Loader2 size={15} class="animate-spin" /> Reading the services…</div>
  {/if}
</div>
