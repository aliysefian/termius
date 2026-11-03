<script lang="ts">
  import { BellRing, ScrollText, ShieldCheck, Wrench } from "lucide-svelte";
  import AlertsPanel from "./AlertsPanel.svelte";
  import LogsWorkspace from "./ops/LogsWorkspace.svelte";
  import ServicesPanel from "./ServicesPanel.svelte";
  import SecurityCockpitView from "./SecurityCockpitView.svelte";
  import { alerts } from "$lib/stores/alerts.svelte";
  import { ops, type OpsTab } from "$lib/stores/ops.svelte";
  import { opsLogs } from "$lib/stores/opslogs.svelte";
  import { t as tr } from "$lib/i18n/index.svelte";

  const TABS: { id: OpsTab; label: "ops.logs" | "ops.services" | "ops.security" | "ops.alerts"; icon: typeof Wrench }[] = [
    { id: "logs", label: "ops.logs", icon: ScrollText },
    { id: "services", label: "ops.services", icon: Wrench },
    { id: "security", label: "ops.security", icon: ShieldCheck },
    { id: "alerts", label: "ops.alerts", icon: BellRing },
  ];

  const tabKey = (e: KeyboardEvent) => {
    const i = TABS.findIndex((t) => t.id === ops.tab);
    const to = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = TABS[(to + TABS.length) % TABS.length];
    ops.tab = next.id;
    queueMicrotask(() => document.getElementById(`ops-tab-${next.id}`)?.focus());
  };
</script>

<div class="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-base" data-testid="operations">
  <header class="shrink-0 border-b border-line bg-panel px-4 pt-2.5">
    <h1 class="flex items-center gap-2 text-[15px] font-semibold"><Wrench size={18} class="text-accent" /> {tr("ops.title")}</h1>
    <p class="mt-0.5 text-xs text-fg-muted">{tr("ops.subtitle")}</p>
    <div class="mt-2 flex gap-1" role="tablist" aria-label="Operations sections">
      {#each TABS as t (t.id)}
        <button
          id="ops-tab-{t.id}"
          role="tab"
          aria-selected={ops.tab === t.id}
          aria-controls="ops-panel-{t.id}"
          tabindex={ops.tab === t.id ? 0 : -1}
          class="-mb-px flex items-center gap-1.5 border-b-2 px-3 py-1.5 text-sm transition-colors focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent {ops.tab === t.id ? 'border-accent font-medium text-accent' : 'border-transparent text-fg-muted hover:text-fg'}"
          onclick={() => (ops.tab = t.id)}
          onkeydown={tabKey}
        >
          <t.icon size={14} />
          {tr(t.label)}
          {#if t.id === "logs" && opsLogs.running}
            <span class="h-1.5 w-1.5 rounded-full {opsLogs.phase === 'streaming' && !opsLogs.paused ? 'animate-pulse bg-success' : 'bg-warning'}" title="Following {opsLogs.hosts.length} host{opsLogs.hosts.length === 1 ? '' : 's'}"></span>
          {/if}
          {#if t.id === "alerts" && alerts.unread > 0}<span class="rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{alerts.unread}</span>{/if}
        </button>
      {/each}
    </div>
  </header>

  <!-- Logs stay mounted (and running) while another tab is looked at. -->
  <div id="ops-panel-logs" role="tabpanel" aria-labelledby="ops-tab-logs" class="min-h-0 flex-1 {ops.tab === 'logs' ? 'flex' : 'hidden'}"><LogsWorkspace /></div>
  {#if ops.tab !== "logs"}
    <div class="min-h-0 flex-1 overflow-y-auto" role="tabpanel" id="ops-panel-{ops.tab}" aria-labelledby="ops-tab-{ops.tab}">
      <div class="mx-auto max-w-6xl p-5">
        {#if ops.tab === "services"}<ServicesPanel />{:else if ops.tab === "security"}<SecurityCockpitView />{:else}<AlertsPanel />{/if}
      </div>
    </div>
  {/if}
</div>
