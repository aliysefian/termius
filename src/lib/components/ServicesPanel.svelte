<script lang="ts">
  import { onDestroy } from "svelte";
  import { Info, Loader2, Play, RefreshCw, RotateCw, ScrollText, Search, Square, SquareTerminal, X } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { DISRUPTIVE, LIST_SCRIPT, VERBS, actionScript, detailsScript, parseDetails, parseUnits, sizeText, toneOf, type Unit, type UnitDetails, type Verb } from "$lib/ops/services";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { ops } from "$lib/stores/ops.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  let hostId = $state<Uuid | "">("");
  let session: Uuid | null = null;
  let loading = $state(false);
  let error = $state<string | null>(null);
  let systemd = $state(true);
  let units = $state<Unit[]>([]);
  let query = $state("");
  let filter = $state<"all" | "failed" | "running" | "inactive">("all");
  let useSudo = $state(false);
  let busy = $state<string | null>(null);
  let loadedAt = $state<number | null>(null);
  let selected = $state<string | null>(null);
  let details = $state<{ unit: string; loading: boolean; data: UnitDetails | null; error: string | null } | null>(null);

  const hosts = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.canMonitor(h.id)).sort((a, b) => a.data!.label.localeCompare(b.data!.label)));
  const host = $derived(hostId ? vaultStore.hostById.get(hostId)?.data : undefined);
  const production = $derived(envInfo(vaultStore.effectiveEnv(host)).value === "production");

  const counts = $derived({
    all: units.length,
    failed: units.filter((u) => u.active === "failed").length,
    running: units.filter((u) => u.active === "active" && u.sub === "running").length,
    inactive: units.filter((u) => u.active === "inactive").length,
  });
  const shown = $derived(
    units.filter((u) => {
      const q = query.trim().toLowerCase();
      if (q && !u.name.toLowerCase().includes(q) && !u.description.toLowerCase().includes(q)) return false;
      if (filter === "failed") return u.active === "failed";
      if (filter === "running") return u.active === "active" && u.sub === "running";
      if (filter === "inactive") return u.active === "inactive";
      return true;
    }),
  );

  async function run(script: string, timeout = 20) {
    if (!session) throw new Error("Not connected.");
    return api.monitor.exec(session, script, timeout);
  }

  async function connectAndLoad(id: Uuid) {
    loading = true;
    error = null;
    details = null;
    selected = null;
    try {
      if (session) await api.monitor.close(session).catch(() => {});
      session = null;
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
    const r = await run(LIST_SCRIPT, 30);
    const parsed = parseUnits(r.stdout);
    systemd = parsed.systemd;
    units = parsed.units;
    loadedAt = Date.now();
  }

  async function refresh() {
    if (!hostId) return;
    loading = true;
    error = null;
    try {
      if (!session) session = await api.monitor.open(hostId);
      await load();
      if (selected) void showDetails(selected, true);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  async function act(u: Unit, verb: Verb) {
    if (!host) return;
    const word = verb[0].toUpperCase() + verb.slice(1);
    if (DISRUPTIVE.includes(verb)) {
      // Taking something down on production means typing the host's name; elsewhere it still asks once.
      const ok = production
        ? await ask(`${word} ${u.name} on ${host.label}? This is a production host.`, { title: "Production host", confirm: word, danger: true, requireText: host.label })
        : await ask(`${word} ${u.name} on ${host.label}? People or programs using it may notice.`, { title: `${word} service`, confirm: word, danger: true });
      if (!ok) return;
    }
    busy = `${u.name}:${verb}`;
    try {
      const r = await run(actionScript(verb, u.name, useSudo), 60);
      if (r.code !== 0) ui.notify("error", `${verb} ${u.name}: ${(r.stdout || r.stderr).trim().split("\n")[0] || `exit ${r.code}`}${useSudo ? "" : ". If it needs root, turn on “Use sudo”."}`);
      else ui.notify("info", `${u.name}: ${verb} done.`);
      await load();
      if (selected === u.name) void showDetails(u.name, true);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = null;
    }
  }

  async function showDetails(name: string, quiet = false) {
    selected = name;
    if (!quiet) details = { unit: name, loading: true, data: null, error: null };
    try {
      const r = await run(detailsScript(name));
      details = { unit: name, loading: false, data: parseDetails(r.stdout + (r.stderr ? `\n${r.stderr}` : "")), error: null };
    } catch (e) {
      details = { unit: name, loading: false, data: null, error: errorMessage(e) };
    }
  }

  // "View service" from a log entry lands here with a host and a unit chosen.
  $effect(() => {
    const focus = ops.serviceFocus;
    if (!focus) return;
    ops.serviceFocus = null;
    hostId = focus.hostId;
    void connectAndLoad(focus.hostId).then(() => {
      if (units.some((u) => u.name === focus.unit)) void showDetails(focus.unit);
      else query = focus.unit;
    });
  });

  onDestroy(() => {
    if (session) void api.monitor.close(session).catch(() => {});
  });

  const dot = { good: "bg-success", bad: "bg-danger", warn: "bg-warning", muted: "bg-fg-muted/50" } as const;
  const text = { good: "text-success", bad: "text-danger", warn: "text-warning", muted: "text-fg-muted" } as const;
  /** What makes sense for a unit in its state: a running one can be restarted, reloaded or stopped; a stopped one started. Boot enablement is separate. */
  const runtimeVerbs = (u: Unit): Verb[] => (u.active === "active" || u.active === "activating" || u.active === "reloading" ? ["restart", "reload", "stop"] : ["start"]);
  const bootVerb = (u: Unit): Verb | null => (u.enabled === "enabled" ? "disable" : u.enabled === "disabled" ? "enable" : null);
  const icons: Partial<Record<Verb, typeof Play>> = { start: Play, stop: Square, restart: RotateCw, reload: RefreshCw };
  const FILTERS = [
    { id: "all", label: "All" },
    { id: "failed", label: "Failed" },
    { id: "running", label: "Running" },
    { id: "inactive", label: "Inactive" },
  ] as const;
  const d = $derived(details?.data?.props ?? {});
  void VERBS;
</script>

<div class="space-y-3" data-testid="services">
  <div class="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-line bg-panel px-3 py-2">
    <label class="flex items-center gap-1.5 text-xs text-fg-muted">
      Host
      <select class="input h-8 w-52 py-0 text-xs" aria-label="Host" bind:value={hostId} onchange={() => hostId && void connectAndLoad(hostId)}>
        <option value="">Choose a host…</option>
        {#each hosts as h (h.id)}<option value={h.id}>{h.data?.label}</option>{/each}
      </select>
    </label>
    <div class="relative min-w-48 max-w-xs flex-1">
      <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
      <input class="input h-8 w-full pl-8 text-xs" bind:value={query} placeholder="Search services" aria-label="Search services" disabled={!units.length} />
    </div>
    <div class="flex gap-1" role="group" aria-label="Show">
      {#each FILTERS as f (f.id)}
        <button class="h-7 rounded-md border px-2 text-[11px] {filter === f.id ? 'border-accent/60 bg-accent/10 text-accent' : 'border-line text-fg-muted hover:text-fg'} {f.id === 'failed' && counts.failed ? 'text-danger' : ''}" aria-pressed={filter === f.id} disabled={!units.length} onclick={() => (filter = f.id)}>{f.label} <span class="opacity-70">{counts[f.id]}</span></button>
      {/each}
    </div>
    <label class="flex items-center gap-1.5 text-xs text-fg-muted" title="Runs the action with sudo -n: it works only where sudo needs no password, and fails at once where it does. Off unless you turn it on.">
      <input type="checkbox" class="accent-input" bind:checked={useSudo} /> Use sudo (no password prompt)
    </label>
    <div class="ml-auto flex items-center gap-2">
      {#if loadedAt}<span class="text-[11px] text-fg-muted">Read {new Date(loadedAt).toLocaleTimeString()}</span>{/if}
      <button class="btn-secondary h-8 text-xs" disabled={!hostId || loading} onclick={() => void refresh()}>
        {#if loading}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if} Refresh
      </button>
    </div>
  </div>

  {#if error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error}</p>{/if}

  {#if !hostId}
    <div class="rounded-lg border border-line bg-panel p-8"><EmptyState art="hosts" text={hosts.length ? "Choose a host to see its services." : "No host has saved credentials yet; services are read over a second connection."} /></div>
  {:else if !systemd && !loading}
    <div class="rounded-lg border border-line bg-panel p-8"><EmptyState art="security" text="This host doesn't use systemd, so there is nothing to list here." /></div>
  {:else if units.length}
    <div class="flex min-h-0 gap-3">
      <div class="min-w-0 flex-1 overflow-hidden rounded-lg border border-line bg-panel">
        <div class="max-h-[calc(100vh-17rem)] overflow-auto">
          <table class="w-full text-left text-xs">
            <thead class="sticky top-0 z-10 bg-panel text-fg-muted shadow-[0_1px_0_var(--color-line,#2a2a3a)]">
              <tr><th class="px-3 py-1.5 font-medium">Service</th><th class="font-medium">Runtime state</th><th class="font-medium" title="Whether it starts when the host boots. Separate from whether it is running now.">At boot</th><th class="font-medium">Since</th><th class="pr-3 text-right font-medium">Actions</th></tr>
            </thead>
            <tbody>
              {#each shown as u (u.name)}
                {@const tone = toneOf(u)}
                {@const boot = bootVerb(u)}
                <tr class="cursor-pointer border-t border-line/60 hover:bg-panel-hover/50 {selected === u.name ? 'bg-accent/10' : ''}" onclick={() => void showDetails(u.name)} data-testid="service-row">
                  <td class="max-w-72 px-3 py-1.5">
                    <div class="flex items-center gap-2"><span class="h-2 w-2 shrink-0 rounded-full {dot[tone]}"></span><span class="truncate font-medium" title={u.name}>{u.name}</span></div>
                    <div class="truncate pl-4 text-[11px] text-fg-muted" title={u.description}>{u.description}</div>
                  </td>
                  <td class="whitespace-nowrap {text[tone]}">{u.active} <span class="text-fg-muted">({u.sub})</span></td>
                  <td class="text-fg-muted">{u.enabled || "—"}</td>
                  <td class="whitespace-nowrap text-fg-muted">{u.since || "—"}</td>
                  <td class="whitespace-nowrap pr-3 text-right" onclick={(e) => e.stopPropagation()}>
                    {#each runtimeVerbs(u) as v (v)}
                      {@const Icon = icons[v]}
                      <button class="btn-ghost px-1.5 py-0.5 text-[11px] {DISRUPTIVE.includes(v) ? 'hover:text-danger' : ''}" disabled={busy !== null} title="{v} {u.name}" aria-label="{v} {u.name}" onclick={() => void act(u, v)}>
                        {#if busy === `${u.name}:${v}`}<Loader2 size={11} class="animate-spin" />{:else if Icon}<Icon size={11} /> {v}{:else}{v}{/if}
                      </button>
                    {/each}
                    {#if boot}
                      <button class="btn-ghost px-1.5 py-0.5 text-[11px] {DISRUPTIVE.includes(boot) ? 'hover:text-danger' : ''}" disabled={busy !== null} title="{boot === 'enable' ? 'Start it at boot' : 'Stop it starting at boot'} (does not start or stop it now)" aria-label="{boot} {u.name} at boot" onclick={() => void act(u, boot)}>
                        {#if busy === `${u.name}:${boot}`}<Loader2 size={11} class="animate-spin" />{:else}{boot}{/if}
                      </button>
                    {/if}
                    <button class="icon-btn h-6 w-6" title="Details" aria-label="Details of {u.name}" onclick={() => void showDetails(u.name)}><Info size={13} /></button>
                    <button class="icon-btn h-6 w-6" title="View its logs" aria-label="Logs of {u.name}" onclick={() => hostId && void ops.viewLogs(hostId, u.name)}><ScrollText size={13} /></button>
                  </td>
                </tr>
              {:else}
                <tr><td colspan="5" class="px-3 py-8 text-center text-fg-muted">No service matches.</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
        <div class="border-t border-line px-3 py-1.5 text-[11px] text-fg-muted">{shown.length} of {units.length}{production ? " · production: stopping, restarting or disabling asks you to type the host's name" : " · stopping, restarting or disabling asks first"}</div>
      </div>

      {#if details}
        <aside class="w-[22rem] shrink-0 overflow-hidden rounded-lg border border-line bg-panel" aria-label="Service details" data-testid="service-details">
          <div class="flex items-center gap-2 border-b border-line px-3 py-2">
            <h3 class="min-w-0 flex-1 truncate text-sm font-semibold" title={details.unit}>{details.unit}</h3>
            <button class="icon-btn h-6 w-6" aria-label="Close the details" onclick={() => ((details = null), (selected = null))}><X size={13} /></button>
          </div>
          <div class="max-h-[calc(100vh-20rem)] overflow-auto p-3 text-xs">
            {#if details.loading}
              <p class="flex items-center gap-2 text-fg-muted"><Loader2 size={13} class="animate-spin" /> Reading…</p>
            {:else if details.error}
              <p class="text-danger">{details.error}</p>
            {:else if details.data}
              <p class="mb-2 text-fg-muted">{d.Description ?? ""}</p>
              <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
                <dt class="text-fg-muted">Runtime</dt><dd>{d.ActiveState ?? "—"} ({d.SubState ?? "—"})</dd>
                <dt class="text-fg-muted">Loaded</dt><dd>{d.LoadState ?? "—"}</dd>
                <dt class="text-fg-muted">At boot</dt><dd>{d.UnitFileState || "—"}</dd>
                <dt class="text-fg-muted">Result</dt><dd>{d.Result || "—"}</dd>
                <dt class="text-fg-muted">Main PID</dt><dd>{d.MainPID && d.MainPID !== "0" ? d.MainPID : "—"}</dd>
                <dt class="text-fg-muted">Restarts</dt><dd>{d.NRestarts ?? "—"}</dd>
                <dt class="text-fg-muted">Restart policy</dt><dd>{d.Restart || "—"}</dd>
                <dt class="text-fg-muted">User</dt><dd>{d.User || "root or default"}</dd>
                <dt class="text-fg-muted">Memory</dt><dd>{sizeText(d.MemoryCurrent) || "—"}</dd>
                <dt class="text-fg-muted">State changed</dt><dd>{d.StateChangeTimestamp || "—"}</dd>
                <dt class="text-fg-muted">Unit file</dt><dd class="break-all font-mono text-[11px]">{d.FragmentPath || "—"}</dd>
              </dl>
              <div class="mt-3 flex flex-wrap gap-1.5">
                <button class="btn-secondary h-7 text-[11px]" onclick={() => hostId && void ops.viewLogs(hostId, details!.unit)}><ScrollText size={12} /> View logs</button>
                <button class="btn-secondary h-7 text-[11px]" onclick={() => hostId && host && ui.openTerminal(hostId, host.label)}><SquareTerminal size={12} /> Terminal</button>
              </div>
              <h4 class="mb-1 mt-3 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">systemctl status</h4>
              <pre class="max-h-64 overflow-auto whitespace-pre-wrap rounded-md bg-base p-2 font-mono text-[11px] leading-5">{details.data.status}</pre>
            {/if}
          </div>
        </aside>
      {/if}
    </div>
  {:else if loading}
    <div class="flex items-center gap-2 py-8 text-sm text-fg-muted"><Loader2 size={15} class="animate-spin" /> Reading the services…</div>
  {/if}
</div>
