<script lang="ts">
  import { AlertTriangle, CheckCircle2, HelpCircle, Info, Loader2, RefreshCw, ScrollText } from "lucide-svelte";
  import type { SecuritySession } from "$lib/security/securitySession.svelte";
  import { OVERVIEW_SCRIPT, findings, parseOverview, type Finding, type Overview } from "$lib/security/overview";
  import { SESSIONS_SCRIPT, parseSessions, type LoginSession, type RecentLogin } from "$lib/security/intrusion";
  import { ops } from "$lib/stores/ops.svelte";
  import { errorMessage } from "$lib/types";

  let { session }: { session: SecuritySession } = $props();

  let loading = $state(false);
  let error = $state<string | null>(null);
  let overview = $state<Overview | null>(null);
  let current = $state<LoginSession[]>([]);
  let recent = $state<RecentLogin[]>([]);
  let loadedFor = $state<string | null>(null);

  const list = $derived<Finding[]>(overview ? findings(overview) : []);
  const tcp = $derived((overview?.ports ?? []).filter((p) => p.proto === "tcp"));
  const icon = { ok: CheckCircle2, warn: AlertTriangle, info: Info, unknown: HelpCircle } as const;
  const tone = { ok: "text-success", warn: "text-warning", info: "text-fg-muted", unknown: "text-fg-muted" } as const;
  const exposure = { all: "all addresses", local: "this host only", address: "one address" } as const;

  async function load() {
    if (!session.connected) return;
    loading = true;
    error = null;
    try {
      // Only reads, as the ordinary user: nothing here asks for a password.
      const [a, b] = await Promise.all([session.read(OVERVIEW_SCRIPT, 25), session.read(SESSIONS_SCRIPT, 25)]);
      overview = parseOverview(a.stdout);
      const s = parseSessions(b.stdout);
      current = s.current;
      recent = s.recent;
      loadedFor = session.hostId;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (session.connected && loadedFor !== session.hostId && !loading) void load();
  });

  const hostId = $derived(session.hostId);
</script>

<div class="space-y-4" data-testid="security-overview">
  <div class="flex flex-wrap items-center gap-2">
    <h2 class="text-sm font-semibold">Overview</h2>
    <span class="rounded bg-success/10 px-2 py-0.5 text-[11px] text-success">Read-only · no password asked</span>
    <div class="flex-1"></div>
    {#if hostId}
      <button class="btn-secondary h-7 px-2 text-xs" onclick={() => void ops.viewLogs(hostId, undefined, { units: ["sshd"] })} title="Follow this host's logs filtered to the SSH server"><ScrollText size={12} /> SSH events in Logs</button>
      <button class="btn-secondary h-7 px-2 text-xs" onclick={() => void ops.viewLogs(hostId, undefined, { text: "Failed password|Invalid user|authentication failure|Accepted", regex: true })} title="Follow this host's logs and search for logins"><ScrollText size={12} /> Login events in Logs</button>
    {/if}
    <button class="btn-secondary h-7 px-2 text-xs" disabled={loading} onclick={() => void load()}>{#if loading}<Loader2 size={12} class="animate-spin" />{:else}<RefreshCw size={12} />{/if} Refresh</button>
  </div>
  {#if error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{error}</p>{/if}

  <ul class="divide-y divide-line/60 overflow-hidden rounded-lg border border-line bg-panel">
    {#each list as f (f.id)}
      {@const Icon = icon[f.status]}
      <li class="flex items-start gap-3 px-4 py-2.5 text-xs" data-testid="finding" data-status={f.status}>
        <Icon size={15} class="mt-0.5 shrink-0 {tone[f.status]}" />
        <div class="min-w-0"><div class="font-medium">{f.title}</div><div class="text-fg-muted">{f.detail}</div></div>
      </li>
    {:else}
      <li class="px-4 py-6 text-center text-xs text-fg-muted">{loading ? "Reading…" : "Nothing read yet."}</li>
    {/each}
  </ul>

  <div class="grid gap-4 lg:grid-cols-2">
    <div class="overflow-hidden rounded-lg border border-line">
      <div class="border-b border-line bg-panel/60 px-3 py-1.5 text-xs font-medium text-fg-muted">Listening ports ({tcp.length} TCP, {(overview?.ports.length ?? 0) - tcp.length} UDP)</div>
      <div class="max-h-64 overflow-auto">
        <table class="w-full text-left font-mono text-xs tabular-nums">
          <tbody class="divide-y divide-line/80">
            {#each overview?.ports ?? [] as p (`${p.proto}${p.address}${p.port}`)}
              <tr class="[&>td]:px-3 [&>td]:py-1"><td>{p.proto}/{p.port}</td><td class="text-fg-muted">{p.address}</td><td class="font-sans {p.exposure === 'all' ? 'text-warning' : 'text-fg-muted'}">{exposure[p.exposure]}</td></tr>
            {:else}
              <tr><td class="px-3 py-4 text-center font-sans text-fg-muted">{loading ? "Reading…" : "None read."}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
    <div class="space-y-4">
      <div class="overflow-hidden rounded-lg border border-line">
        <div class="border-b border-line bg-panel/60 px-3 py-1.5 text-xs font-medium text-fg-muted">Logged in now ({current.length})</div>
        <table class="w-full text-left font-mono text-xs tabular-nums"><tbody class="divide-y divide-line/80">
          {#each current as s (s.tty)}<tr class="[&>td]:px-3 [&>td]:py-1"><td>{s.user}</td><td>{s.tty}</td><td>{s.source || "local"}</td><td class="text-fg-muted">{s.since}</td></tr>{:else}<tr><td class="px-3 py-3 text-center font-sans text-fg-muted">Nobody.</td></tr>{/each}
        </tbody></table>
      </div>
      <div class="overflow-hidden rounded-lg border border-line">
        <div class="border-b border-line bg-panel/60 px-3 py-1.5 text-xs font-medium text-fg-muted">Recent logins</div>
        <table class="w-full text-left font-mono text-xs tabular-nums"><tbody class="divide-y divide-line/80">
          {#each recent.slice(0, 6) as l, i (i)}<tr class="[&>td]:px-3 [&>td]:py-1"><td>{l.user}</td><td>{l.source || "local"}</td><td class="text-fg-muted">{l.when}</td></tr>{:else}<tr><td class="px-3 py-3 text-center font-sans text-fg-muted">None read.</td></tr>{/each}
        </tbody></table>
      </div>
    </div>
  </div>
  <p class="text-[11px] text-fg-muted">Not checked here because they need root: the firewall's rules, fail2ban, and the SSH server's running settings. Open the Firewall and Intrusion radar tabs for those; they ask for sudo only when you open them.</p>
</div>
