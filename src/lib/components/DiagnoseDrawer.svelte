<script lang="ts">
  import { Activity, CheckCircle2, CircleAlert, Loader2, RefreshCw, TriangleAlert, X } from "lucide-svelte";
  import * as api from "$lib/api";
  import { applyEvent, barWidths, emptyReport, formatAvg, formatLoss, lossClass, progress, verdictHeading, verdictTone, type DiagReport } from "$lib/diagnose";
  import { shortFingerprint } from "$lib/nethops";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  // A slide-over that checks the way to one host without opening a shell: the name, the TCP connection against the
  // SSH key exchange, the route with loss per hop, and the login. A verdict on top says where it most likely fails.
  let { hostId }: { hostId: Uuid } = $props();

  const host = $derived(vaultStore.hostById.get(hostId)?.data);
  let report = $state<DiagReport>(emptyReport());
  let running = $state(false);
  let failure = $state<string | null>(null);
  /** The run whose results count: starting another run makes an earlier, slower one go quiet. */
  let run = 0;

  async function start() {
    const mine = ++run;
    report = emptyReport();
    failure = null;
    running = true;
    try {
      const verdict = await api.diagnoseHost(hostId, (e) => {
        if (mine === run) report = applyEvent(report, e);
      });
      if (mine === run && !report.verdict) report = applyEvent(report, { stage: "verdict", verdict });
    } catch (e) {
      if (mine === run) failure = errorMessage(e);
    } finally {
      if (mine === run) running = false;
    }
  }

  // Runs when it opens, and again for another host (the drawer is reused).
  $effect(() => {
    void hostId;
    void start();
    return () => {
      run++;
    };
  });

  const tone = $derived(report.verdict ? verdictTone(report.verdict.kind) : null);
  const toneClass = $derived(tone === "ok" ? "border-success/30 bg-success/10 text-success" : tone === "warn" ? "border-warning/30 bg-warning/10 text-warning" : "border-danger/30 bg-danger/10 text-danger");
  const [tcpBar, kexBar] = $derived(barWidths(report.tcp?.connect_ms, report.ssh?.kex_ms));
  const prog = $derived(progress(report));

  const trace = $derived(report.route?.hops ?? []);
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") ui.diagnoseHost = null; }} />

<aside class="anim-slide fixed inset-y-0 right-0 z-40 flex w-96 max-w-full flex-col border-l border-line bg-base shadow-lg" aria-label="Connection diagnostics" data-testid="diagnose-drawer">
  <header class="flex items-center gap-2 border-b border-line px-4 py-3">
    <Activity size={16} class="shrink-0 text-accent" aria-hidden="true" />
    <div class="min-w-0 flex-1">
      <h2 class="truncate text-sm font-semibold">Diagnose · {host?.label ?? "host"}</h2>
      <p class="truncate font-mono text-[11px] tabular-nums text-fg-muted">{host ? `${host.hostname}:${host.port}` : ""}</p>
    </div>
    <button class="icon-btn h-7 w-7" title="Run again" aria-label="Run again" disabled={running} onclick={() => void start()}>
      <RefreshCw size={14} class={running ? "animate-spin" : ""} />
    </button>
    <button class="icon-btn h-7 w-7" title="Close" aria-label="Close" onclick={() => (ui.diagnoseHost = null)}><X size={14} /></button>
  </header>

  <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-4 text-xs">
    <!-- The verdict -->
    {#if failure}
      <div class="flex items-start gap-2 rounded-md border border-danger/30 bg-danger/10 p-3 text-danger" role="alert">
        <CircleAlert size={14} class="mt-0.5 shrink-0" />
        <span>{failure}</span>
      </div>
    {:else if report.verdict}
      <div class="rounded-md border p-3 {toneClass}" role="status" data-testid="diagnose-verdict" data-kind={report.verdict.kind}>
        <div class="flex items-center gap-2 text-sm font-semibold">
          {#if tone === "ok"}<CheckCircle2 size={15} />{:else}<TriangleAlert size={15} />{/if}
          {verdictHeading(report.verdict.kind)}
        </div>
        <p class="mt-1 leading-relaxed text-fg">{report.verdict.detail}</p>
      </div>
    {:else}
      <div class="flex items-center gap-2 rounded-md border border-line bg-panel p-3 text-fg-muted" role="status">
        <Loader2 size={14} class="shrink-0 animate-spin" />
        Running checks… <span class="font-mono tabular-nums">{prog.done}/{prog.total}</span>
      </div>
    {/if}

    <!-- TCP and SSH -->
    <section class="rounded-md border border-line bg-panel p-3" data-testid="diagnose-handshake">
      <h3 class="mb-2 font-semibold">TCP &amp; SSH handshake</h3>
      {#if !report.tcp}
        <p class="text-fg-muted">Connecting…</p>
      {:else}
        <div class="space-y-1.5 font-mono tabular-nums">
          <div class="grid grid-cols-[5.5rem_1fr_4rem] items-center gap-2">
            <span class="font-sans text-fg-muted">TCP connect</span>
            <span class="h-1.5 rounded bg-line"><span class="block h-full rounded bg-accent" style:width="{tcpBar}%"></span></span>
            <span class="text-right">{report.tcp.connect_ms === null ? "failed" : `${report.tcp.connect_ms} ms`}</span>
          </div>
          <div class="grid grid-cols-[5.5rem_1fr_4rem] items-center gap-2">
            <span class="font-sans text-fg-muted">Key exchange</span>
            <span class="h-1.5 rounded bg-line"><span class="block h-full rounded bg-success" style:width="{kexBar}%"></span></span>
            <span class="text-right">{!report.ssh ? "…" : report.ssh.kex_ms === null ? "failed" : `${report.ssh.kex_ms} ms`}</span>
          </div>
        </div>
        {#if report.tcp.error}<p class="mt-2 text-danger">{report.tcp.error}</p>{/if}
        {#if report.ssh}
          <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 font-mono">
            {#if report.ssh.banner}<dt class="text-fg-muted">Server</dt><dd class="truncate" title={report.ssh.banner}>{report.ssh.banner}</dd>{/if}
            {#if report.ssh.algorithm}<dt class="text-fg-muted">Host key</dt><dd>{report.ssh.algorithm}</dd>{/if}
            {#if report.ssh.fingerprint}<dt class="text-fg-muted">Fingerprint</dt><dd class="truncate" title={report.ssh.fingerprint}>{shortFingerprint(report.ssh.fingerprint)}</dd>{/if}
          </dl>
          {#if report.ssh.error}<p class="mt-2 text-danger">{report.ssh.error}</p>{/if}
        {/if}
      {/if}
    </section>

    <!-- Route -->
    <section class="rounded-md border border-line bg-panel p-3" data-testid="diagnose-route">
      <h3 class="mb-2 flex items-center gap-2 font-semibold">
        Route
        {#if report.routeRunning}<Loader2 size={12} class="animate-spin text-fg-muted" />{/if}
        {#if report.route?.tool}<span class="ml-auto font-mono text-[11px] font-normal text-fg-muted">{report.route.tool}</span>{/if}
      </h3>
      {#if report.route && trace.length}
        <table class="w-full table-fixed font-mono text-xs tabular-nums">
          <thead class="text-left text-[11px] text-fg-muted">
            <tr><th class="w-9 pb-1 font-normal">Hop</th><th class="pb-1 font-normal">IP / Hostname</th><th class="w-12 pb-1 text-right font-normal">Loss</th><th class="w-14 pb-1 text-right font-normal">Avg ms</th></tr>
          </thead>
          <tbody>
            {#each trace as h (h.hop)}
              <tr class="border-t border-line/60">
                <td class="py-0.5 text-fg-muted">{h.hop}</td>
                <td class="truncate py-0.5 {h.ip ? '' : 'text-fg-muted'}" title={h.ip ?? "no answer"}>{h.ip ?? "* * *"}</td>
                <td class="py-0.5 text-right {lossClass(h.loss_pct)}">{formatLoss(h.loss_pct)}</td>
                <td class="py-0.5 text-right">{formatAvg(h.avg_ms)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if report.route.tool === "tracepath"}<p class="mt-2 text-fg-muted">tracepath does not report packet loss.</p>{/if}
      {:else if report.route?.error}
        <p class="text-fg-muted">{report.route.error}</p>
      {:else if report.routeRunning}
        <p class="text-fg-muted">Tracing the route… this takes several seconds.</p>
      {:else if report.verdict || report.tcp}
        <p class="text-fg-muted">The route was not traced.</p>
      {:else}
        <p class="text-fg-muted">Waiting…</p>
      {/if}
    </section>

    <!-- DNS -->
    <section class="rounded-md border border-line bg-panel p-3" data-testid="diagnose-dns">
      <h3 class="mb-2 flex items-center gap-2 font-semibold">
        DNS &amp; reverse lookup
        {#if report.dns}<span class="ml-auto font-mono text-[11px] font-normal tabular-nums text-fg-muted">{report.dns.resolve_ms} ms</span>{/if}
      </h3>
      {#if !report.dns}
        <p class="text-fg-muted">Resolving…</p>
      {:else if report.dns.error}
        <p class="text-danger">{report.dns.error}</p>
      {:else}
        <table class="w-full table-fixed font-mono text-xs tabular-nums">
          <tbody>
            {#each report.dns.addresses as a (a.ip)}
              <tr class="border-t border-line/60 first:border-t-0">
                <td class="w-12 py-0.5 text-fg-muted">{a.record}</td>
                <td class="truncate py-0.5" title={a.ip}>{a.ip}</td>
                <td class="w-40 truncate py-0.5 text-right text-fg-muted" title={a.ptr ?? "no PTR record"}>{a.ptr ?? "no PTR"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if report.dns.literal}<p class="mt-1 text-fg-muted">The host is an address; nothing was looked up.</p>{/if}
      {/if}
      <p class="mt-2 text-fg-muted">The network's owner (ASN, cloud provider) is not looked up: that means sending the address to a third party.</p>
    </section>

    <!-- Login -->
    {#if report.auth || report.verdict}
      <section class="rounded-md border border-line bg-panel p-3" data-testid="diagnose-login">
        <h3 class="mb-1 font-semibold">Login</h3>
        {#if !report.auth}
          <p class="text-fg-muted">Not tried: no stored credentials, or the host could not be reached.</p>
        {:else if report.auth.ok}
          <p class="text-success">Signed in and out in <span class="font-mono tabular-nums">{report.auth.total_ms} ms</span>.</p>
        {:else}
          <p class="text-danger">{report.auth.error}</p>
        {/if}
      </section>
    {/if}

    <p class="text-fg-muted">Nothing is run on the host and no shell is opened. A login, if tried, signs in and straight out again.</p>
  </div>
</aside>
