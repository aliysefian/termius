<script lang="ts">
  import { BellRing, ScrollText, Server, Wrench } from "lucide-svelte";
  import AlertsPanel from "./AlertsPanel.svelte";
  import LogsPanel from "./LogsPanel.svelte";
  import ServicesPanel from "./ServicesPanel.svelte";
  import { alerts } from "$lib/stores/alerts.svelte";
  import type { Uuid } from "$lib/types";

  type Tab = "logs" | "services" | "alerts";
  let tab = $state<Tab>("logs");
  let preset = $state<{ hostId: Uuid; unit: string } | null>(null);

  const TABS: { id: Tab; label: string; icon: typeof Server }[] = [
    { id: "logs", label: "Logs", icon: ScrollText },
    { id: "services", label: "Services", icon: Wrench },
    { id: "alerts", label: "Alerts", icon: BellRing },
  ];

  /** From a service's "Follow its log" button. */
  function followLog(hostId: Uuid, unit: string) {
    preset = { hostId, unit };
    tab = "logs";
  }
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-6xl space-y-5">
    <div>
      <h1 class="flex items-center gap-2 text-lg font-semibold"><Wrench size={20} class="text-accent" /> Operations</h1>
      <p class="mt-1 text-sm text-fg-muted">Follow logs on several hosts at once, manage systemd services, and get told when a host goes down or runs hot. Everything is read over a second connection; nothing is installed on the host.</p>
    </div>

    <div class="flex w-fit gap-1 rounded-lg border border-line bg-panel p-1 text-sm" role="tablist">
      {#each TABS as t (t.id)}
        <button
          role="tab"
          aria-selected={tab === t.id}
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium transition-colors {tab === t.id ? 'bg-accent text-white shadow-sm' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
          onclick={() => (tab = t.id)}
        >
          <t.icon size={14} />
          {t.label}
          {#if t.id === "alerts" && alerts.unread > 0}<span class="rounded-full bg-danger px-1.5 text-[10px] font-semibold text-white">{alerts.unread}</span>{/if}
        </button>
      {/each}
    </div>

    <!-- Logs keep running while another tab is looked at. -->
    <div hidden={tab !== "logs"}><LogsPanel {preset} /></div>
    {#if tab === "services"}<ServicesPanel onlogs={followLog} />{/if}
    {#if tab === "alerts"}<AlertsPanel />{/if}
  </div>
</div>
