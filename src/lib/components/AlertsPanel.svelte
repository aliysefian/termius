<script lang="ts">
  import { BellRing, CheckCircle2, Loader2, RefreshCw, ScrollText, Siren } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import type { Alert, AlertKind } from "$lib/ops/alerts";
  import { alerts } from "$lib/stores/alerts.svelte";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { ops } from "$lib/stores/ops.svelte";
  import { opsLogs } from "$lib/stores/opslogs.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const watched = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.isMonitored(h.id)));
  const at = (t: number) => new Date(t).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

  $effect(() => {
    // Looking at the list reads it.
    alerts.unread = 0;
  });

  const TITLE: Record<AlertKind, string> = {
    down: "Host not answering",
    up: "Host reachable again",
    cpu: "High CPU",
    mem: "High memory",
    disk: "High disk usage",
    service: "Service failed",
    cleared: "Back to normal",
    log: "Critical log entry",
    auth: "Repeated SSH login failures",
  };
  const status = (a: Alert) => (a.severity === "info" ? "Recovered" : a.ack !== null ? `Acknowledged ${at(a.ack)}` : "Open");
  const p = settings.prefs;
</script>

<div class="space-y-4" data-testid="alerts">
  <div class="flex flex-wrap items-center gap-3 rounded-lg border border-line bg-panel px-4 py-3">
    <BellRing size={18} class={p.alerts ? "text-accent" : "text-fg-muted"} />
    <div class="min-w-0 flex-1 text-sm">
      {#if p.alerts}
        Alerts are <strong>on</strong> for {alerts.watched().length} monitored {alerts.watched().length === 1 ? "host" : "hosts"}: {#if p.alertDown}down{:else}no down check{/if}{#each [["CPU", p.alertCpu], ["memory", p.alertMem], ["disk", p.alertDisk]] as [n, v] (n)}{#if v}, {n} over {v}%{/if}{/each}{p.alertQuiet ? `, quiet ${p.alertQuietFrom}–${p.alertQuietTo}` : ""}.
      {:else}
        Alerts are <strong>off</strong>. They tell you when a monitored host stops answering or runs hot, only while this app is open.
      {/if}
    </div>
    {#if !p.alerts}<button class="btn-primary h-8 text-xs" onclick={() => (p.alerts = true)}>Turn alerts on</button>{/if}
    <button class="btn-secondary h-8 text-xs" onclick={() => (ui.view = "settings")}>Settings</button>
    {#if p.alerts}
      <button class="btn-secondary h-8 text-xs" disabled={alerts.checking || !alerts.watched().length} onclick={() => void alerts.check()}>
        {#if alerts.checking}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if} Check now
      </button>
    {/if}
  </div>

  <div class="rounded-lg border border-line bg-panel px-4 py-3" data-testid="log-alert-rules">
    <h2 class="text-sm font-semibold">From followed logs</h2>
    <p class="mt-0.5 text-xs text-fg-muted">
      These look at the lines of Operations → Logs, so they only work while a stream is being followed ({opsLogs.running ? `following ${opsLogs.hosts.length} host${opsLogs.hosts.length === 1 ? "" : "s"} now` : "none is now"}). They use the same list, quiet hours, muting and notifications as the alerts above. The same problem on a host is announced at most once every ten minutes.
    </p>
    <div class="mt-2 grid gap-2 sm:grid-cols-3">
      <label class="flex items-start gap-2 text-xs"><input type="checkbox" class="mt-0.5 accent-input" disabled={!p.alerts} bind:checked={p.alertLogCritical} /><span><span class="font-medium">Critical entries</span><br /><span class="text-fg-muted">Emergency, alert or critical priority in the journal.</span></span></label>
      <label class="flex items-start gap-2 text-xs"><input type="checkbox" class="mt-0.5 accent-input" disabled={!p.alerts} bind:checked={p.alertLogService} /><span><span class="font-medium">Failed services</span><br /><span class="text-fg-muted">A unit entering the failed state.</span></span></label>
      <div class="text-xs">
        <label class="flex items-start gap-2"><input type="checkbox" class="mt-0.5 accent-input" disabled={!p.alerts} bind:checked={p.alertLogAuth} /><span><span class="font-medium">Repeated SSH login failures</span></span></label>
        <div class="mt-1 flex items-center gap-1 pl-6 text-fg-muted">
          at least <input type="number" min="2" max="100" class="input h-6 w-14 px-1 py-0 text-xs" aria-label="Failed logins" disabled={!p.alerts || !p.alertLogAuth} bind:value={p.alertLogAuthCount} />
          in <input type="number" min="1" max="120" class="input h-6 w-14 px-1 py-0 text-xs" aria-label="Minutes" disabled={!p.alerts || !p.alertLogAuth} bind:value={p.alertLogAuthMins} /> min
        </div>
      </div>
    </div>
    {#if !p.alerts}<p class="mt-2 text-[11px] text-warning">Alerts are off. Turn them on in Settings first.</p>{/if}
  </div>

  <div class="grid gap-4 lg:grid-cols-[1.7fr_1fr]">
    <div class="rounded-lg border border-line bg-panel">
      <div class="flex items-center justify-between border-b border-line px-4 py-2.5">
        <h2 class="text-sm font-semibold">{p.alertKeepHistory ? "Kept on this computer" : "Since this app was opened"}{#if alerts.openCount} <span class="ml-1 text-xs font-normal text-danger">· {alerts.openCount} to acknowledge</span>{/if}</h2>
        <div class="flex gap-1">
          <button class="btn-ghost py-0.5 text-xs" disabled={!alerts.openCount} onclick={() => alerts.acknowledge("all")}>Acknowledge all</button>
          <button class="btn-ghost py-0.5 text-xs" disabled={!alerts.log.length} onclick={() => alerts.clear()}>Clear</button>
        </div>
      </div>
      {#if alerts.log.length === 0}
        <div class="p-6"><EmptyState art="security" text={p.alerts ? "Nothing to report. All quiet." : "Nothing yet."} /></div>
      {:else}
        <ul class="divide-y divide-line/60">
          {#each alerts.log as a (a.id)}
            <li class="flex items-start gap-3 px-4 py-2.5 text-sm" data-testid="alert-row">
              {#if a.severity === "info"}<CheckCircle2 size={16} class="mt-0.5 shrink-0 text-success" />{:else}<Siren size={16} class="mt-0.5 shrink-0 {a.severity === 'critical' ? 'text-danger' : 'text-warning'}" />{/if}
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-x-2">
                  <span class="font-medium {a.ack !== null ? 'text-fg-muted' : ''}">{TITLE[a.kind]}</span>
                  <span class="rounded px-1.5 text-[10px] font-semibold uppercase {a.severity === 'critical' ? 'bg-danger/15 text-danger' : a.severity === 'warning' ? 'bg-warning/15 text-warning' : 'bg-success/15 text-success'}">{a.severity}</span>
                  <span class="text-xs text-fg-muted">{a.host}</span>
                </div>
                <div class="break-words text-xs {a.ack !== null ? 'text-fg-muted' : ''}">{a.message}</div>
                <div class="mt-0.5 text-[11px] text-fg-muted">
                  First detected {at(a.at)}{a.last && a.last !== a.at ? ` · last ${at(a.last)}` : ""}{a.count && a.count > 1 ? ` · seen ${a.count} times` : ""} · {status(a)}{a.quiet ? " · quiet hours: not announced" : ""}
                </div>
              </div>
              <div class="flex shrink-0 flex-col items-end gap-1">
                {#if a.severity !== "info" && a.ack === null}<button class="btn-ghost py-0.5 text-xs" onclick={() => alerts.acknowledge(a.id)}>Acknowledge</button>{/if}
                {#if vaultStore.hostById.get(a.hostId)}<button class="btn-ghost inline-flex items-center gap-1 py-0.5 text-xs" onclick={() => void ops.viewLogs(a.hostId)} title="Follow this host's logs"><ScrollText size={12} /> Inspect logs</button>{/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="rounded-lg border border-line bg-panel">
      <div class="border-b border-line px-4 py-2.5"><h2 class="text-sm font-semibold">Hosts watched</h2></div>
      {#if watched.length === 0}
        <p class="p-4 text-xs text-fg-muted">Alerts watch the hosts you turned monitoring on for (the pulse button on a host in Fleet).</p>
        <div class="px-4 pb-4"><button class="btn-secondary py-1 text-xs" onclick={() => (ui.view = "fleet")}>Open Fleet</button></div>
      {:else}
        <ul class="divide-y divide-line/60">
          {#each watched as h (h.id)}
            <li class="flex items-center gap-2 px-4 py-2 text-sm">
              <span class="min-w-0 flex-1 truncate">{h.data?.label}</span>
              <label class="flex items-center gap-1.5 text-xs text-fg-muted" title="A muted host never raises an alert">
                <input type="checkbox" class="accent-input" checked={alerts.isMuted(h.id)} onchange={(e) => alerts.setMuted(h.id, e.currentTarget.checked)} /> mute
              </label>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</div>
