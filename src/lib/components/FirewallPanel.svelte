<script lang="ts">
  import { Loader2, RefreshCw, ShieldAlert, ShieldCheck, Trash2 } from "lucide-svelte";
  import { ask } from "$lib/dialogs.svelte";
  import type { FwRule } from "$lib/security/firewall";
  import type { FirewallManager } from "$lib/security/useFirewallManager.svelte";
  import type { SecuritySession } from "$lib/security/securitySession.svelte";

  let { fw, session }: { fw: FirewallManager; session: SecuritySession } = $props();

  const BACKEND = { ufw: "UFW", firewalld: "firewalld", nftables: "nftables", iptables: "iptables", none: "Firewall" } as const;

  let port = $state("");
  let proto = $state<"tcp" | "udp">("tcp");
  let expiry = $state("0");
  let blockTarget = $state("");

  // The port box starts on the port this connection came in on.
  $effect(() => {
    if (port === "" && session.me) port = String(session.me.sshPort);
  });

  const EXPIRIES = [
    { v: "0", label: "Permanent" },
    { v: "3600", label: "Temporary: 1 hour" },
    { v: "7200", label: "Temporary: 2 hours" },
    { v: "86400", label: "Temporary: 24 hours" },
  ];

  const tone = {
    allow: "bg-success/10 text-success",
    limit: "bg-warning/10 text-warning",
    deny: "bg-danger/10 text-danger",
    drop: "bg-danger/10 text-danger",
    reject: "bg-danger/10 text-danger",
    other: "bg-fg-muted/10 text-fg-muted",
  } as const;

  const count = (n: number | null) => (n === null ? "—" : n.toLocaleString());
  const bytes = (n: number | null) => {
    if (n === null) return "—";
    const u = ["B", "KB", "MB", "GB", "TB"];
    let i = 0;
    let v = n;
    while (v >= 1024 && i < u.length - 1) (v /= 1024), i++;
    return `${i === 0 ? v : v.toFixed(1)} ${u[i]}`;
  };
  const policy = (p: string | undefined) => (p ? p.toUpperCase() : "—");
  const deletable = (r: FwRule) => (fw.state.backend === "ufw" && r.num !== null) || (fw.state.backend === "iptables" && r.num !== null) || (fw.state.backend === "nftables" && r.handle !== undefined) || fw.state.backend === "firewalld";
  const hasCounters = $derived(fw.state.rules.some((r) => r.packets !== null));

  async function block() {
    const t = blockTarget.trim();
    if (!t) return;
    if (await fw.block(t, "sshvault")) blockTarget = "";
  }
</script>

<div class="space-y-3">
  <!-- Backend, defaults, whether the way in is open -->
  <div class="flex flex-wrap items-center gap-2 rounded-lg border border-line bg-panel/60 px-3 py-2 text-xs">
    {#if fw.state.active}
      <span class="inline-flex items-center gap-1.5 rounded-sm bg-success/10 px-2 py-0.5 font-mono font-medium text-success"><ShieldCheck size={12} /> {BACKEND[fw.state.backend]}: Active</span>
    {:else if fw.loaded}
      <span class="inline-flex items-center gap-1.5 rounded-sm bg-danger/10 px-2 py-0.5 font-mono font-medium text-danger"><ShieldAlert size={12} /> Firewall: INACTIVE / Exposed</span>
    {/if}
    {#if fw.state.active}
      <span class="rounded-sm border border-line px-2 py-0.5 font-mono tabular-nums">{policy(fw.state.policies.incoming)} INCOMING</span>
      <span class="rounded-sm border border-line px-2 py-0.5 font-mono tabular-nums">{policy(fw.state.policies.outgoing)} OUTGOING</span>
    {/if}
    {#if fw.loaded && session.me?.clientIp}
      <span class="rounded-sm px-2 py-0.5 font-mono tabular-nums {fw.sshOpen === 'allow' ? 'text-success' : fw.sshOpen === 'deny' ? 'bg-danger/10 text-danger' : 'text-fg-muted'}" title="Whether the rules let this connection's address reach SSH">
        SSH :{session.me.sshPort} from {session.me.clientIp}: {fw.sshOpen === "allow" ? "open" : fw.sshOpen === "deny" ? "closed" : "unclear"}
      </span>
    {/if}
    <div class="flex-1"></div>
    {#if fw.loaded && !fw.state.active && fw.state.backend === "ufw"}
      <button class="btn-secondary px-2 py-1 text-xs" disabled={!!fw.busy} onclick={() => void fw.enable()}>Enable ufw safely</button>
    {/if}
    <button class="btn-secondary px-2 py-1 text-xs" disabled={fw.loading} onclick={() => void fw.refresh()}>
      {#if fw.loading}<Loader2 size={12} class="animate-spin" />{:else}<RefreshCw size={12} />{/if} Refresh Rules
    </button>
  </div>

  {#each fw.state.notes as n (n)}<p class="px-1 text-xs text-fg-muted">{n}</p>{/each}
  {#if fw.error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{fw.error}</p>{/if}

  <!-- Quick rule builder -->
  <div class="flex flex-wrap items-end gap-3 rounded-lg border border-line bg-panel/60 p-3 text-xs">
    <div>
      <label class="label" for="fw-port">Port</label>
      <input id="fw-port" class="input w-24 py-1 font-mono tabular-nums text-xs" bind:value={port} placeholder="any" inputmode="numeric" />
    </div>
    <div>
      <label class="label" for="fw-proto">Protocol</label>
      <select id="fw-proto" class="input py-1 text-xs" bind:value={proto}><option value="tcp">tcp</option><option value="udp">udp</option></select>
    </div>
    <div>
      <label class="label" for="fw-exp">Lasts</label>
      <select id="fw-exp" class="input py-1 text-xs" bind:value={expiry}>{#each EXPIRIES as e (e.v)}<option value={e.v}>{e.label}</option>{/each}</select>
    </div>
    <button
      class="btn-primary px-3 py-1.5 text-xs"
      disabled={!session.me?.clientIp || !!fw.busy || !fw.loaded || fw.state.backend === "none"}
      title={session.me?.clientIp ? "" : "The host did not say which address you are connecting from"}
      onclick={() => void fw.allowMyIp(port.trim(), proto, Number(expiry) || undefined)}
    >
      Allow My IP (<span class="font-mono tabular-nums">{session.me?.clientIp ?? "unknown"}</span>){port ? ` on ${port}` : ""}
    </button>
    <div class="ml-auto flex items-end gap-2">
      <div>
        <label class="label" for="fw-block">Block an address or subnet</label>
        <input id="fw-block" class="input w-52 py-1 font-mono tabular-nums text-xs" bind:value={blockTarget} placeholder="185.211.57.0/24" onkeydown={(e) => e.key === "Enter" && void block()} />
      </div>
      <button class="btn-secondary px-3 py-1.5 text-xs" disabled={!blockTarget.trim() || !!fw.busy || !fw.loaded || fw.state.backend === "none"} onclick={block}>Block</button>
    </div>
  </div>

  <!-- Rules -->
  <div class="overflow-x-auto rounded-lg border border-line">
    <table class="w-full text-left font-mono text-xs tabular-nums">
      <thead class="bg-panel/60 text-fg-muted">
        <tr class="[&>th]:px-3 [&>th]:py-2 [&>th]:font-medium">
          <th>Rule #</th><th>Action</th><th>Port / Protocol</th><th>Source IP / Subnet</th>
          {#if hasCounters}<th class="text-right">Packets</th><th class="text-right">Bytes</th>{/if}
          <th>Comment</th><th class="w-10"></th>
        </tr>
      </thead>
      <tbody class="divide-y divide-line/80">
        {#each fw.state.rules as r (r.id)}
          <tr class="group transition-colors hover:bg-panel-hover/50 [&>td]:px-3 [&>td]:py-1.5" title={r.raw}>
            <td class="text-fg-muted">{r.num ?? r.handle ?? "·"}</td>
            <td><span class="rounded-sm px-1.5 py-0.5 text-[11px] font-semibold uppercase {tone[r.action]}">{r.action === "other" ? "jump" : r.action}</span></td>
            <td>{r.ports || "any"}{r.proto ? `/${r.proto}` : ""}{r.direction !== "in" ? ` (${r.direction})` : ""}{r.v6 ? " v6" : ""}</td>
            <td>{r.source}</td>
            {#if hasCounters}<td class="text-right">{count(r.packets)}</td><td class="text-right">{bytes(r.bytes)}</td>{/if}
            <td class="max-w-64 truncate font-sans text-fg-muted">{r.comment}</td>
            <td class="text-right">
              {#if deletable(r)}
                <button class="icon-btn h-6 w-6 opacity-0 transition-opacity focus:opacity-100 group-hover:opacity-100" title="Delete rule" aria-label="Delete rule {r.num ?? ''}" disabled={!!fw.busy} onclick={() => void fw.remove(r)}><Trash2 size={13} /></button>
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td class="px-3 py-6 text-center font-sans text-fg-muted" colspan={hasCounters ? 8 : 6}>{fw.loading ? "Reading the firewall…" : fw.loaded ? "No rules." : "Not loaded yet."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
