<script lang="ts">
  import { BellRing, CheckCircle2, Loader2, RefreshCw, Siren } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import { alerts } from "$lib/stores/alerts.svelte";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const watched = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.isMonitored(h.id)));
  const at = (t: number) => new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

  $effect(() => {
    // Looking at the list reads it.
    alerts.unread = 0;
  });
</script>

<div class="space-y-4">
  <div class="flex flex-wrap items-center gap-3 rounded-xl border border-line bg-panel p-4">
    <BellRing size={18} class={settings.prefs.alerts ? "text-accent" : "text-fg-muted"} />
    <div class="min-w-0 flex-1 text-sm">
      {#if settings.prefs.alerts}
        Alerts are <strong>on</strong> for {alerts.watched().length} monitored {alerts.watched().length === 1 ? "host" : "hosts"}: {#if settings.prefs.alertDown}down{:else}no down check{/if}{#each [["CPU", settings.prefs.alertCpu], ["memory", settings.prefs.alertMem], ["disk", settings.prefs.alertDisk]] as [n, v] (n)}{#if v}, {n} over {v}%{/if}{/each}{settings.prefs.alertQuiet ? `, quiet ${settings.prefs.alertQuietFrom}–${settings.prefs.alertQuietTo}` : ""}.
      {:else}
        Alerts are <strong>off</strong>. They tell you when a monitored host stops answering or runs hot, only while this app is open.
      {/if}
    </div>
    <button class="btn-secondary py-1.5 text-xs" onclick={() => (ui.view = "settings")}>Settings</button>
    {#if settings.prefs.alerts}
      <button class="btn-secondary py-1.5 text-xs" disabled={alerts.checking || !alerts.watched().length} onclick={() => void alerts.check()}>
        {#if alerts.checking}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if} Check now
      </button>
    {/if}
  </div>

  <div class="grid gap-4 lg:grid-cols-[1.5fr_1fr]">
    <div class="rounded-xl border border-line bg-panel">
      <div class="flex items-center justify-between border-b border-line px-4 py-2.5"><h2 class="text-sm font-semibold">{settings.prefs.alertKeepHistory ? "Kept on this computer" : "Since this app was opened"}{#if alerts.openCount} <span class="ml-1 text-xs font-normal text-danger">· {alerts.openCount} to acknowledge</span>{/if}</h2>
        <div class="flex gap-1">
          <button class="btn-ghost py-0.5 text-xs" disabled={!alerts.openCount} onclick={() => alerts.acknowledge("all")}>Acknowledge all</button>
          <button class="btn-ghost py-0.5 text-xs" disabled={!alerts.log.length} onclick={() => alerts.clear()}>Clear</button>
        </div></div>
      {#if alerts.log.length === 0}
        <div class="p-6"><EmptyState art="security" text={settings.prefs.alerts ? "Nothing to report. All quiet." : "Nothing yet."} /></div>
      {:else}
        <ul class="divide-y divide-line/60">
          {#each alerts.log as a (a.id)}
            <li class="flex items-start gap-3 px-4 py-2.5 text-sm">
              {#if a.severity === "info"}<CheckCircle2 size={16} class="mt-0.5 shrink-0 text-success" />{:else}<Siren size={16} class="mt-0.5 shrink-0 {a.severity === 'critical' ? 'text-danger' : 'text-warning'}" />{/if}
              <div class="min-w-0 flex-1">
                <div class={a.ack !== null ? "text-fg-muted" : ""}>{a.message}</div>
                <div class="text-[11px] text-fg-muted">
                  {a.severity}{#if a.quiet} · during quiet hours: not announced{/if}{#if a.ack !== null} · acknowledged {at(a.ack)}{/if}
                </div>
              </div>
              {#if a.severity !== "info" && a.ack === null}<button class="btn-ghost shrink-0 py-0.5 text-xs" onclick={() => alerts.acknowledge(a.id)}>Acknowledge</button>{/if}
              <span class="shrink-0 text-xs text-fg-muted">{at(a.at)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="rounded-xl border border-line bg-panel">
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
