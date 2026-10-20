<script lang="ts">
  import { onDestroy } from "svelte";
  import { Flame, KeyRound, Loader2, Radar, RefreshCw } from "lucide-svelte";
  import FirewallPanel from "./FirewallPanel.svelte";
  import IntrusionPanel from "./IntrusionPanel.svelte";
  import AuthorizedKeysPanel from "./AuthorizedKeysPanel.svelte";
  import { SecuritySession } from "$lib/security/securitySession.svelte";
  import { FirewallManager } from "$lib/security/useFirewallManager.svelte";
  import { IntrusionRadar } from "$lib/security/useIntrusionRadar.svelte";
  import { AuthorizedKeysManager } from "$lib/security/useAuthorizedKeysManager.svelte";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Uuid } from "$lib/types";

  const session = new SecuritySession();
  const firewall = new FirewallManager(session);
  const radar = new IntrusionRadar(session, firewall);
  const keys = new AuthorizedKeysManager(session);

  type Tab = "firewall" | "intrusion" | "keys";
  let tab = $state<Tab>("firewall");
  let hostId = $state<Uuid | "">("");
  let sudoPassword = $state("");

  const TABS: { id: Tab; label: string; icon: typeof Flame }[] = [
    { id: "firewall", label: "Firewall", icon: Flame },
    { id: "intrusion", label: "Intrusion radar", icon: Radar },
    { id: "keys", label: "SSH keys", icon: KeyRound },
  ];

  const hosts = $derived(vaultStore.hosts.filter((h) => h.data && hostMetrics.canMonitor(h.id)).sort((a, b) => a.data!.label.localeCompare(b.data!.label)));

  /** Each part loads when it is first looked at; a different host starts them all over. */
  function load(t: Tab) {
    if (!session.connected) return;
    if (t === "firewall" && !firewall.loaded && !firewall.loading) void firewall.refresh();
    if (t === "intrusion" && !radar.loaded && !radar.loading) void radar.refresh();
    if (t === "keys" && !keys.loaded && !keys.loading) void keys.refresh();
  }

  async function connect(id: Uuid) {
    firewall.loaded = radar.loaded = keys.loaded = false;
    firewall.state = { ...firewall.state, rules: [], active: false, backend: "none" };
    radar.attackers = radar.current = radar.recent = [];
    radar.fail2ban = null;
    keys.keys = [];
    await session.open(id);
    load(tab);
  }

  function submitPassword(e: SubmitEvent) {
    e.preventDefault();
    const p = sudoPassword;
    sudoPassword = "";
    session.answerPassword(p);
  }

  onDestroy(() => void session.close());
</script>

<div class="space-y-4">
  <div class="flex flex-wrap items-end gap-3 rounded-xl border border-line bg-panel p-4">
    <div class="min-w-56 flex-1">
      <label class="label" for="sec-host">Host</label>
      <select id="sec-host" class="input" bind:value={hostId} onchange={() => hostId && void connect(hostId)}>
        <option value="">Choose a host…</option>
        {#each hosts as h (h.id)}<option value={h.id}>{h.data?.label}</option>{/each}
      </select>
    </div>
    {#if session.me}
      <div class="pb-2 font-mono text-xs tabular-nums text-fg-muted" title="Who the cockpit is on the host, and the address the host sees you at">
        {session.me.user}{session.me.uid === 0 ? " (root)" : ""} · you are {session.me.clientIp ?? "unknown"} · sshd :{session.me.sshPort}
      </div>
    {/if}
    {#if session.sudoKnown}
      <button class="btn-ghost px-2 py-1 text-xs" onclick={() => session.forgetSudo()} title="The sudo password is kept in memory only, until the app closes">Forget sudo password</button>
    {/if}
    <button class="btn-secondary" disabled={!hostId || session.connecting} onclick={() => hostId && void connect(hostId)}>
      {#if session.connecting}<Loader2 size={14} class="animate-spin" />{:else}<RefreshCw size={14} />{/if} Reconnect
    </button>
  </div>
  <p class="px-1 text-xs text-fg-muted">Everything here runs over its own connection to the host, not in your terminal, so nothing lands in your shell history. Changes that need root use <span class="font-mono">sudo</span>; the password is kept in memory only.</p>
  {#if session.error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{session.error}</p>{/if}

  {#if session.passwordPrompt}
    <form class="flex flex-wrap items-center gap-3 rounded-lg border border-warning/30 bg-warning/10 px-4 py-2 text-xs text-warning" role="alert" data-testid="sudo-password-bar" onsubmit={submitPassword}>
      <label class="shrink-0" for="sec-sudo">{session.passwordPrompt.reason ?? `sudo needs ${session.me?.user ?? "your"} password on the host`}</label>
      <input id="sec-sudo" class="input max-w-56 py-1 text-xs" type="password" autocomplete="off" placeholder="Password for sudo" bind:value={sudoPassword} required />
      <button class="btn-secondary px-2 py-1 text-xs" type="button" onclick={() => { sudoPassword = ""; session.answerPassword(null); }}>Cancel</button>
      <button class="btn-primary px-2 py-1 text-xs" type="submit" disabled={!sudoPassword}>Continue</button>
    </form>
  {/if}

  {#if session.connected}
    <div class="flex w-fit gap-1 rounded-lg border border-line bg-panel p-1 text-sm" role="tablist">
      {#each TABS as t (t.id)}
        <button role="tab" aria-selected={tab === t.id} class="flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium transition-colors {tab === t.id ? 'bg-accent text-white shadow-sm' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}" onclick={() => { tab = t.id; load(t.id); }}>
          <t.icon size={14} /> {t.label}
        </button>
      {/each}
    </div>
    {#if tab === "firewall"}<FirewallPanel fw={firewall} {session} />{/if}
    {#if tab === "intrusion"}<IntrusionPanel {radar} />{/if}
    {#if tab === "keys"}<AuthorizedKeysPanel mgr={keys} />{/if}
  {:else if !session.connecting && !session.error}
    <p class="rounded-xl border border-dashed border-line px-6 py-10 text-center text-sm text-fg-muted">Choose a host to see its firewall, who is logging in, and which keys can.</p>
  {/if}
</div>
