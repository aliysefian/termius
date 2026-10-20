<script lang="ts">
  import { Ban, Loader2, RefreshCw, Skull } from "lucide-svelte";
  import type { IntrusionRadar } from "$lib/security/useIntrusionRadar.svelte";
  import { isIP } from "$lib/security/net";

  let { radar }: { radar: IntrusionRadar } = $props();

  let manual = $state("");
  const f2b = $derived(radar.fail2ban);
  const activeJails = $derived(f2b?.jails.length ?? 0);

  const when = (d: Date | null) => (d ? d.toLocaleString() : "—");

  async function ban() {
    const ip = manual.trim();
    if (!isIP(ip)) return;
    await radar.ban(ip);
    manual = "";
  }

  const metrics = $derived([
    { label: "Currently Banned IPs", value: f2b?.running ? String(radar.totalBanned) : "—" },
    { label: "Total Banned (24h)", value: f2b?.bans24h === null || f2b?.bans24h === undefined ? "—" : String(f2b.bans24h) },
    { label: "Failed SSH Attempts (1h)", value: radar.failedLastHour === null ? "—" : String(radar.failedLastHour) },
    { label: "Active Jails", value: f2b?.running ? String(activeJails) : "—" },
  ]);
</script>

<div class="space-y-4">
  <div class="flex items-center justify-between">
    <h2 class="text-sm font-semibold">Sessions and intrusion attempts</h2>
    <button class="btn-secondary px-2 py-1 text-xs" disabled={radar.loading} onclick={() => void radar.refresh()}>
      {#if radar.loading}<Loader2 size={12} class="animate-spin" />{:else}<RefreshCw size={12} />{/if} Refresh
    </button>
  </div>
  {#if radar.error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{radar.error}</p>{/if}

  <!-- Who is on the host -->
  <div class="overflow-hidden rounded-lg border border-line">
    <div class="border-b border-line bg-panel/60 px-3 py-1.5 text-xs font-medium text-fg-muted">Logged in now</div>
    <table class="w-full text-left font-mono text-xs tabular-nums">
      <thead class="text-fg-muted"><tr class="[&>th]:px-3 [&>th]:py-1.5 [&>th]:font-medium"><th>User</th><th>TTY / PTS</th><th>Source IP</th><th>Login</th><th>Active command</th><th class="w-10"></th></tr></thead>
      <tbody class="divide-y divide-line/80">
        {#each radar.current as s (s.tty)}
          <tr class="group transition-colors hover:bg-panel-hover/50 [&>td]:px-3 [&>td]:py-1.5">
            <td>{s.user}</td><td>{s.tty}</td>
            <td>{s.source || "local"}{#if radar.isMine(s)}<span class="ml-1.5 rounded-sm bg-accent/10 px-1 font-sans text-[10px] text-accent">you</span>{/if}</td>
            <td>{s.since}</td><td class="max-w-72 truncate" title={s.command}>{s.command || "—"}</td>
            <td class="text-right">
              {#if s.tty.startsWith("pts/") && !radar.isMine(s)}
                <button class="icon-btn h-6 w-6 opacity-0 transition-opacity hover:text-danger focus:opacity-100 group-hover:opacity-100" title="Terminate session (kill -9)" aria-label="Terminate session {s.tty}" onclick={() => void radar.terminate(s)}><Skull size={13} /></button>
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td class="px-3 py-4 text-center font-sans text-fg-muted" colspan="6">{radar.loaded ? "Nobody." : "Not loaded yet."}</td></tr>
        {/each}
      </tbody>
    </table>
    <div class="border-y border-line bg-panel/60 px-3 py-1.5 text-xs font-medium text-fg-muted">Last successful logins</div>
    <table class="w-full text-left font-mono text-xs tabular-nums">
      <tbody class="divide-y divide-line/80">
        {#each radar.recent as l, i (i)}
          <tr class="transition-colors hover:bg-panel-hover/50 [&>td]:px-3 [&>td]:py-1.5"><td>{l.user}</td><td>{l.tty}</td><td>{l.source || "local"}</td><td>{l.when}</td><td class="text-fg-muted">{l.duration}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>

  <!-- fail2ban -->
  <div class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-line bg-line sm:grid-cols-4">
    {#each metrics as m (m.label)}
      <div class="bg-base px-3 py-2.5"><div class="text-[11px] text-fg-muted">{m.label}</div><div class="mt-0.5 font-mono text-lg tabular-nums">{m.value}</div></div>
    {/each}
  </div>
  {#if radar.loaded && f2b && !f2b.installed}
    <p class="px-1 text-xs text-fg-muted">fail2ban is not installed on this host, so there are no bans to show. Attempts below come from the SSH log.</p>
  {:else if radar.loaded && f2b && !f2b.running}
    <p class="px-1 text-xs text-fg-muted">fail2ban is installed but not answering (not running, or this user may not talk to it).</p>
  {/if}

  {#if f2b?.running}
    <div class="overflow-hidden rounded-lg border border-line">
      <div class="flex items-center gap-2 border-b border-line bg-panel/60 px-3 py-1.5 text-xs">
        <span class="font-medium text-fg-muted">Banned addresses</span>
        <div class="flex-1"></div>
        <input class="input w-44 py-1 font-mono tabular-nums text-xs" placeholder="Ban IP manually" bind:value={manual} onkeydown={(e) => e.key === "Enter" && void ban()} aria-label="Address to ban" />
        <button class="btn-secondary px-2 py-1 text-xs" disabled={!isIP(manual.trim())} onclick={ban}>Ban</button>
      </div>
      <table class="w-full text-left font-mono text-xs tabular-nums">
        <tbody class="divide-y divide-line/80">
          {#each radar.banned as b (b.jail + b.ip)}
            <tr class="transition-colors hover:bg-panel-hover/50 [&>td]:px-3 [&>td]:py-1.5">
              <td>{b.ip}</td><td class="text-fg-muted">{b.jail}</td>
              <td class="text-right"><button class="btn-ghost px-2 py-0.5 text-xs" onclick={() => void radar.unban(b.ip, b.jail)}>Unban IP</button></td>
            </tr>
          {:else}
            <tr><td class="px-3 py-4 text-center font-sans text-fg-muted">No one is banned.</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <!-- Brute-force attackers -->
  <div class="overflow-x-auto rounded-lg border border-line">
    <div class="border-b border-line bg-panel/60 px-3 py-1.5 text-xs font-medium text-fg-muted">Top brute-force sources (from the SSH log)</div>
    <table class="w-full text-left font-mono text-xs tabular-nums">
      <thead class="text-fg-muted"><tr class="[&>th]:px-3 [&>th]:py-1.5 [&>th]:font-medium"><th>Attacker IP</th><th>Country</th><th class="text-right">Attempts</th><th>Targeted usernames</th><th>Last seen</th><th class="w-28"></th></tr></thead>
      <tbody class="divide-y divide-line/80">
        {#each radar.attackers as a (a.ip)}
          <tr class="group transition-colors hover:bg-panel-hover/50 [&>td]:px-3 [&>td]:py-1.5">
            <td>{a.ip}</td><td class="font-sans text-fg-muted">{a.country || "—"}</td><td class="text-right">{a.attempts}</td>
            <td class="max-w-56 truncate">{a.users.join(", ") || "—"}</td><td>{when(a.lastSeen)}</td>
            <td class="text-right"><button class="btn-ghost px-2 py-0.5 text-xs text-danger" onclick={() => void radar.block(a.ip)}><Ban size={12} /> Block IP</button></td>
          </tr>
        {:else}
          <tr><td class="px-3 py-4 text-center font-sans text-fg-muted" colspan="6">{radar.loaded ? "No failed logins in the log." : "Not loaded yet."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
