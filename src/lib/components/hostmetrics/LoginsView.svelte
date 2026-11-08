<script lang="ts">
  import { onMount } from "svelte";
  import { AlertTriangle, Info, Loader2, RefreshCw, ScrollText } from "lucide-svelte";
  import Badge from "../Badge.svelte";
  import { agoText, failuresBySource, newestFirst } from "$lib/hostlists";
  import type { AuthEvent, LoginEntry } from "$lib/hostsample";
  import { hostDetails } from "$lib/stores/hostdetails.svelte";
  import { ops } from "$lib/stores/ops.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import type { Uuid } from "$lib/types";

  let { hostId }: { hostId: Uuid } = $props();
  const r = $derived(hostDetails.logins[hostId] ?? null);
  const error = $derived(hostDetails.errors[`logins:${hostId}`] ?? null);
  const loading = $derived(!!hostDetails.loading[`logins:${hostId}`] && !r);
  let refreshing = $state(false);

  onMount(() => hostDetails.watch("logins", hostId));

  // "Ago" uses the time the report was taken, so a figure does not drift while the list is on screen.
  const at = $derived(r?.taken_at_ms ?? 0);
  const failures = $derived(r ? failuresBySource(r.auth) : []);
  const KIND: Record<AuthEvent["kind"], { text: string; tone: "success" | "danger" | "warning" | "neutral" }> = {
    accepted: { text: "Accepted", tone: "success" },
    failed: { text: "Failed", tone: "danger" },
    invalid_user: { text: "Unknown user", tone: "warning" },
    other: { text: "Other", tone: "neutral" },
  };

  /** Operations is behind this window, so the window closes as the logs open. */
  function openLogs() {
    ui.modal = null;
    void ops.viewLogs(hostId, undefined, { units: ["ssh.service", "sshd.service"] });
  }

  async function refresh() {
    refreshing = true;
    try {
      await hostDetails.refresh("logins", hostId, true);
    } finally {
      refreshing = false;
    }
  }
</script>

{#snippet logins(title: string, items: LoginEntry[], empty: string)}
  <section class="rounded-lg border border-line bg-panel p-3">
    <h3 class="text-sm font-semibold">{title}</h3>
    {#if items.length === 0}
      <p class="py-3 text-xs text-fg-muted">{empty}</p>
    {:else}
      <div class="overflow-x-auto">
        <table class="mt-2 w-full min-w-[36rem] text-left text-xs">
          <thead class="text-fg-muted"><tr><th class="py-1 pr-3 font-medium">User</th><th class="pr-3 font-medium">Terminal</th><th class="pr-3 font-medium">From</th><th class="pr-3 font-medium">Started</th><th class="font-medium">Ended</th></tr></thead>
          <tbody>
            {#each newestFirst(items) as e, i (i)}
              <tr class="border-t border-line/60">
                <td class="py-1.5 pr-3 font-medium">{e.user}</td>
                <td class="pr-3 font-mono">{e.tty}</td>
                <td class="pr-3 font-mono">{e.from ?? "—"}</td>
                <td class="pr-3" title={e.started}>{agoText(e.started_ms, at) ?? e.started}</td>
                <td class="text-fg-muted">{e.ended || "—"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
{/snippet}

<div class="space-y-3" data-testid="logins-view">
  <div class="flex flex-wrap items-center gap-2">
    <p class="text-xs text-fg-muted">Read-only: who is logged in, who was, and what the SSH server logged. Nothing here is marked suspicious; a failed attempt is a failed attempt.</p>
    <button class="btn-secondary ml-auto py-1 text-xs" onclick={() => openLogs()} title="Open the SSH server's journal in Operations → Logs"><ScrollText size={13} /> SSH logs</button>
    <button class="icon-btn h-7 w-7" title="Refresh now" aria-label="Refresh logins" onclick={refresh} disabled={refreshing}><RefreshCw size={14} class={refreshing ? "animate-spin" : ""} /></button>
  </div>
  {#if error}
    <p class="flex items-start gap-2 rounded-md border {r ? 'border-warning/40 bg-warning/10 text-warning' : 'border-danger/30 bg-danger/10 text-danger'} px-3 py-2 text-xs" role="alert"><AlertTriangle size={14} class="mt-0.5 shrink-0" /><span>{r ? `The last reading failed (${error}); showing the one before it.` : error}</span></p>
  {/if}

  {#if loading}
    <div class="flex items-center justify-center gap-2 py-10 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Reading the logins…</div>
  {:else if r}
    {#if r.limits.length}
      <ul class="space-y-1.5" aria-label="What could not be read">
        {#each r.limits as l, i (i)}
          <li class="flex items-start gap-2 rounded-md border border-line bg-panel px-3 py-2 text-xs"><Info size={14} class="mt-0.5 shrink-0 text-fg-muted" /><span><span class="font-medium">{l.what}:</span> <span class="text-fg-muted">{l.reason}</span></span></li>
        {/each}
      </ul>
    {/if}
    {@render logins("Current sessions", r.sessions, "Nobody is logged in, or this host does not say.")}
    {@render logins("Recent logins", r.recent, "No logins to show.")}
    {#if !r.limits.some((l) => l.what === "Failed login attempts")}{@render logins("Failed login attempts (btmp)", r.failed_logins, "No failed attempts are recorded.")}{/if}

    {#if r.auth_source}
      <section class="rounded-lg border border-line bg-panel p-3">
        <h3 class="text-sm font-semibold">SSH authentication events <span class="font-normal text-fg-muted">· from {r.auth_source}, last 24 hours at most</span></h3>
        {#if failures.length}
          <p class="mt-2 text-xs text-fg-muted">Failed attempts by source:
            {#each failures.slice(0, 5) as f, i (f.from)}{i ? "; " : " "}<span class="font-mono text-fg">{f.from}</span> × {f.count}{f.users.length ? ` (${f.users.join(", ")})` : ""}{/each}
          </p>
        {/if}
        {#if r.auth.length === 0}
          <p class="py-3 text-xs text-fg-muted">The log has no accepted or failed logins in that time.</p>
        {:else}
          <div class="overflow-x-auto">
            <table class="mt-2 w-full min-w-[36rem] text-left text-xs">
              <thead class="text-fg-muted"><tr><th class="py-1 pr-3 font-medium">When</th><th class="pr-3 font-medium">Result</th><th class="pr-3 font-medium">User</th><th class="pr-3 font-medium">From</th><th class="font-medium">Method</th></tr></thead>
              <tbody>
                {#each r.auth as e, i (i)}
                  <tr class="border-t border-line/60">
                    <td class="py-1.5 pr-3 tabular-nums">{e.time}</td>
                    <td class="pr-3"><Badge tone={KIND[e.kind].tone}>{KIND[e.kind].text}</Badge></td>
                    <td class="pr-3">{e.user ?? "—"}</td>
                    <td class="pr-3 font-mono">{e.from ?? "—"}</td>
                    <td class="text-fg-muted">{e.method ?? "—"}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>
    {/if}
  {/if}
</div>
