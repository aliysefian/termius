<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { ArrowDown, ArrowUp, Loader2, OctagonX, Pause, Play, RefreshCw, Skull } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import HostChart from "./HostChart.svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { DETAIL_SCRIPT, formatBytes, formatRate, killScript, parseDetailOutput, parseKillOutput, sortProcesses, type HostDetail, type ProcessInfo, type ProcessSort } from "$lib/hostdetail";
  import { GAP_MS, HISTORY_MS, clock, type Sample } from "$lib/hosthistory";
  import { formatUptime } from "$lib/hostmetrics";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, isApiError, type Uuid } from "$lib/types";

  let { id }: { id: Uuid } = $props();

  type Tab = "overview" | "processes" | "ports" | "interfaces";
  const INTERVALS = [3, 5, 10, 30] as const;
  const MAX_ROWS = 150;

  const host = $derived(vaultStore.hostById.get(id)?.data);
  const production = $derived(vaultStore.effectiveEnv(host) === "production");
  const label = $derived(host?.label ?? "host");

  let sessionId = $state<Uuid | null>(null);
  let detail = $state<HostDetail | null>(null);
  let error = $state<string | null>(null);
  let connecting = $state(true);
  let sampling = $state(false);
  let updatedAt = $state(0);
  let now = $state(Date.now());
  let paused = $state(false);
  let intervalSecs = $state<number>(5);
  let tab = $state<Tab>("overview");
  let showTable = $state(false);
  let query = $state("");
  let sort = $state<{ key: ProcessSort; dir: "asc" | "desc" }>({ key: "cpu", dir: "desc" });
  let signalling = $state<Record<number, boolean>>({});
  let closed = false;

  // How far back the charts reach: the live 15 minutes, or what was kept (Settings → Alerts and monitoring history).
  const RANGES = [
    { id: "live", label: "15 min", ms: HISTORY_MS },
    { id: "day", label: "24 h", ms: 24 * 3600_000 },
    { id: "week", label: "7 days", ms: 7 * 24 * 3600_000 },
  ] as const;
  let range = $state<(typeof RANGES)[number]["id"]>("live");
  const keep = $derived(settings.prefs.metricsKeep);
  const rangeOk = (r: (typeof RANGES)[number]["id"]) => r === "live" || (keep === "week") || (keep === "day" && r === "day");
  const windowMs = $derived(RANGES.find((r) => r.id === range)!.ms);
  let kept = $state<Sample[]>([]);
  $effect(() => {
    if (range === "live" || !rangeOk(range)) {
      kept = [];
      return;
    }
    // Reload as the live readings come in; the archive takes a few seconds to fold a new one in.
    hostMetrics.history[id]?.length;
    void hostMetrics.archived(id, windowMs).then((s) => (kept = s));
  });
  $effect(() => {
    if (!rangeOk(range)) range = "live";
  });
  const samples = $derived(range === "live" ? (hostMetrics.history[id] ?? []) : kept);
  const gapMs = $derived(range === "live" ? GAP_MS : 5 * 60_000);
  const monitored = $derived(hostMetrics.isMonitored(id));

  async function connect() {
    connecting = true;
    error = null;
    try {
      const sid = await api.monitor.open(id);
      if (closed) {
        void api.monitor.close(sid);
        return;
      }
      sessionId = sid;
      await sample();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      connecting = false;
    }
  }

  /** One reading. Skipped while another is running or the window is hidden; never overlaps itself. */
  async function sample(manual = false) {
    if (!sessionId || sampling || (!manual && document.hidden)) return;
    sampling = true;
    try {
      const r = await api.monitor.exec(sessionId, DETAIL_SCRIPT, 30);
      if (r.code !== 0 && !r.stdout.includes("@@OS")) throw new Error(r.stderr.trim() || `the sampling command ended with status ${r.code ?? "?"}`);
      detail = parseDetailOutput(r.stdout);
      hostMetrics.recordDetail(id, detail);
      updatedAt = now = Date.now();
      error = null;
    } catch (e) {
      error = errorMessage(e);
      // The app side lost the session (for example after a lock): open a fresh one next time.
      if (isApiError(e) && e.code === "no_session") {
        sessionId = null;
        void connect();
      }
    } finally {
      sampling = false;
    }
  }

  onMount(() => void connect());
  onDestroy(() => {
    closed = true;
    if (sessionId) void api.monitor.close(sessionId).catch(() => {});
  });

  // The refresh timer, only while there's a connection and it isn't paused.
  $effect(() => {
    if (!sessionId || paused) return;
    const t = setInterval(() => void sample(), intervalSecs * 1000);
    return () => clearInterval(t);
  });
  // The charts' time axis moves on even between readings.
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(t);
  });

  // -- processes ----------------------------------------------------------------

  const shownProcesses = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = (detail?.processes ?? []).filter((p) => !q || `${p.pid} ${p.name} ${p.user}`.toLowerCase().includes(q));
    return sortProcesses(list, sort.key, sort.dir);
  });

  function sortBy(key: ProcessSort) {
    sort = sort.key === key ? { key, dir: sort.dir === "asc" ? "desc" : "asc" } : { key, dir: key === "name" || key === "user" || key === "pid" ? "asc" : "desc" };
  }

  /** Signal a process after asking. On a production host the host's name has to be typed. */
  async function signal(p: ProcessInfo, kind: "TERM" | "KILL") {
    if (!sessionId || !detail || signalling[p.pid]) return;
    const sshd = /^sshd/.test(p.name) ? "\n\nThis looks like an SSH server process. Ending it can close a login session, possibly the one used for monitoring." : "";
    const ok =
      kind === "TERM"
        ? await ask(`Ask ${p.name} (process ${p.pid}, user ${p.user}) on ${label} to stop?\n\nThis sends SIGTERM. The program can save its work and exit, and some ignore it.${sshd}`, {
            title: "Terminate this process?",
            confirm: "Terminate",
            danger: true,
            requireText: production ? label : undefined,
          })
        : await ask(`Force ${p.name} (process ${p.pid}, user ${p.user}) on ${label} to stop right now?\n\nThis sends SIGKILL. The program gets no chance to save or clean up: unsaved data is lost and its child processes can be left running. Try Terminate first.${sshd}`, {
            title: "Force kill this process?",
            confirm: "Force kill",
            danger: true,
            requireText: production ? label : undefined,
          });
    if (!ok) return;
    signalling[p.pid] = true;
    try {
      const r = await api.monitor.exec(sessionId, killScript(detail.os, p.pid, p.nameRaw, kind), 15);
      const out = parseKillOutput(r.stdout, r.stderr, r.code);
      const what = `${p.name} (${p.pid})`;
      if (out.kind === "ok") ui.notify("info", `${kind === "TERM" ? "Asked" : "Forced"} ${what} to stop (SIG${kind}).`);
      else if (out.kind === "gone") ui.notify("info", `${what} had already ended.`);
      else if (out.kind === "changed") ui.notify("error", `Process ${p.pid} is no longer ${p.name}: the number was reused. Nothing was sent.`);
      else ui.notify("error", `Couldn't signal ${what}: ${out.reason}`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      delete signalling[p.pid];
      void sample(true);
    }
  }

  // -- ports and interfaces ---------------------------------------------------------

  const shownPorts = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return (detail?.ports.items ?? []).filter((p) => !q || `${p.proto} ${p.address} ${p.port} ${p.process ?? ""} ${p.pid ?? ""}`.toLowerCase().includes(q));
  });
  const rateOf = (name: string) => detail?.rates.find((r) => r.name === name);
  const note = (panel: HostDetail["unavailable"][number]["panel"]) => detail?.unavailable.find((u) => u.panel === panel)?.reason ?? null;
  const stat = (v: number | null | undefined) => (v === null || v === undefined ? "—" : `${v}%`);
</script>

<Modal title="Monitor · {label}" onclose={() => (ui.modal = null)} width="max-w-6xl">
  <div class="flex h-[72vh] min-h-96 flex-col gap-3">
    {#if connecting}
      <div class="flex flex-1 items-center justify-center gap-2 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Connecting to {label}…</div>
    {:else if !detail}
      <p class="whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error ?? "No reading yet."}</p>
      <div><button class="btn-secondary" onclick={connect}>Try again</button></div>
    {:else}
      <!-- Summary and controls -->
      <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
        <Badge tone="neutral">{detail.os}</Badge>
        {#each [["CPU", stat(detail.cpuPct)], ["Memory", stat(detail.memPct)], ["Disk /", stat(detail.diskPct)], ["Load", detail.load ?? "—"], ["Uptime", detail.uptimeSecs !== null ? formatUptime(detail.uptimeSecs) : "—"]] as [name, value] (name)}
          <div class="text-xs"><span class="text-fg-muted">{name}</span> <span class="font-semibold tabular-nums">{value}</span></div>
        {/each}
        <div class="ml-auto flex items-center gap-2 text-xs text-fg-muted">
          <button class="btn-secondary py-1 text-xs" onclick={() => (paused = !paused)} aria-pressed={paused} title={paused ? "Resume refreshing" : "Stop refreshing"}>
            {#if paused}<Play size={12} /> Resume{:else}<Pause size={12} /> Pause{/if}
          </button>
          <select class="input h-7 w-20 py-0 text-xs" bind:value={intervalSecs} aria-label="Refresh every" disabled={paused}>
            {#each INTERVALS as n (n)}<option value={n}>{n} s</option>{/each}
          </select>
          <button class="icon-btn h-7 w-7" title="Refresh now" aria-label="Refresh now" onclick={() => sample(true)} disabled={sampling}>
            <RefreshCw size={14} class={sampling ? "animate-spin" : ""} />
          </button>
          <span class="w-40 whitespace-nowrap text-right">{paused ? "paused" : updatedAt ? `updated ${clock(updatedAt)}` : ""}</span>
        </div>
      </div>

      {#if error}
        <p class="whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning" role="alert">The last reading failed ({error}); showing the one before it.</p>
      {/if}

      <div class="flex items-center gap-2">
        <div class="flex overflow-hidden rounded-md border border-line text-xs" role="tablist" aria-label="Monitor sections">
          {#each [["overview", "Overview"], ["processes", `Processes (${detail.processes.length})`], ["ports", `Ports (${detail.ports.items.length})`], ["interfaces", `Interfaces (${detail.interfaces.length})`]] as [v, text] (v)}
            <button role="tab" aria-selected={tab === v} class="px-3 py-1 {tab === v ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}" onclick={() => (tab = v as Tab)}>{text}</button>
          {/each}
        </div>
        {#if tab === "processes" || tab === "ports"}
          <input class="input h-8 w-60 text-xs" placeholder={tab === "processes" ? "Search name, user or pid…" : "Search port, address or process…"} aria-label="Search" bind:value={query} />
        {/if}
        {#if tab === "overview"}
          <label class="ml-auto flex items-center gap-1.5 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={showTable} /> Show as a table</label>
        {/if}
      </div>

      <div class="min-h-0 flex-1 overflow-auto">
        {#if tab === "overview"}
          {#if showTable}
            <table class="w-full text-left text-xs">
              <caption class="mb-1 text-left text-fg-muted">Every reading from the last 15 minutes, newest first. Kept in memory only.</caption>
              <thead class="sticky top-0 bg-panel text-fg-muted"><tr><th class="py-1.5 pr-3 font-medium">Time</th><th class="pr-3 text-right font-medium">CPU</th><th class="pr-3 text-right font-medium">Memory</th><th class="pr-3 text-right font-medium">Received</th><th class="text-right font-medium">Sent</th></tr></thead>
              <tbody>
                {#each [...samples].reverse() as s (s.t)}
                  <tr class="border-t border-line/60 tabular-nums">
                    <td class="py-1 pr-3">{clock(s.t)}</td>
                    <td class="pr-3 text-right">{stat(s.cpu)}</td>
                    <td class="pr-3 text-right">{stat(s.mem)}</td>
                    <td class="pr-3 text-right">{s.rx === null ? "—" : formatRate(s.rx)}</td>
                    <td class="text-right">{s.tx === null ? "—" : formatRate(s.tx)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <div class="mb-2 flex items-center gap-2 text-xs">
              <div class="flex gap-0.5 rounded-md border border-line bg-base p-0.5" role="group" aria-label="Time range">
                {#each RANGES as r (r.id)}
                  <button class="rounded px-2 py-0.5 {range === r.id ? 'bg-accent text-white' : rangeOk(r.id) ? 'text-fg-muted hover:text-fg' : 'cursor-not-allowed text-fg-muted/40'}" disabled={!rangeOk(r.id)} aria-pressed={range === r.id} onclick={() => (range = r.id)}>{r.label}</button>
                {/each}
              </div>
              {#if keep === "off"}<span class="text-fg-muted">Longer ranges: Settings → Alerts and monitoring history.</span>{/if}
            </div>
            <div class="grid gap-3 lg:grid-cols-2">
              <HostChart title="CPU" {samples} {now} {windowMs} {gapMs} unit="percent" series={[{ key: "cpu", label: "CPU", slot: 1 }]} empty={detail.cpuPct === null && !monitored ? "Turn on monitoring for this host to chart CPU" : "Collecting readings…"} />
              <HostChart title="Memory" {samples} {now} {windowMs} {gapMs} unit="percent" series={[{ key: "mem", label: "Memory", slot: 1 }]} empty={detail.memPct === null && !monitored ? "Turn on monitoring for this host to chart memory" : "Collecting readings…"} />
              <div class="lg:col-span-2">
                <HostChart title="Network, all interfaces except loopback" {samples} {now} {windowMs} {gapMs} unit="rate" series={[{ key: "rx", label: "Received", slot: 1 }, { key: "tx", label: "Sent", slot: 2 }]} />
              </div>
            </div>
            {#if (detail.cpuPct === null || detail.memPct === null) && !monitored}
              <p class="mt-3 flex flex-wrap items-center gap-2 text-xs text-fg-muted">
                {note("cpu") ?? "CPU and memory history comes from the 30-second summary."}
                <button class="btn-secondary py-0.5 text-xs" onclick={() => hostMetrics.setMonitored(id, true)} disabled={!hostMetrics.canMonitor(id)}>Turn on monitoring</button>
              </p>
            {/if}
            <p class="mt-3 text-[11px] text-fg-muted">The last 15 minutes, kept in memory only: nothing is written to disk, and locking the vault clears it.</p>
          {/if}
        {:else if tab === "processes"}
          {#if note("processes")}<p class="mb-2 text-xs text-warning">{note("processes")}</p>{/if}
          <table class="w-full min-w-[40rem] text-left text-xs">
            <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
              <tr>
                {#each [["pid", "PID", "text-right"], ["name", "Name", ""], ["user", "User", ""], ["cpu", "CPU %", "text-right"], ["mem", "Memory", "text-right"]] as [k, name, cls] (k)}
                  <th class="px-2 py-1.5 font-medium {cls}" aria-sort={sort.key === k ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
                    <button class="inline-flex items-center gap-1 hover:text-fg" onclick={() => sortBy(k as ProcessSort)}>
                      {name}{#if sort.key === k}{#if sort.dir === "asc"}<ArrowUp size={11} />{:else}<ArrowDown size={11} />{/if}{/if}
                    </button>
                  </th>
                {/each}
                <th class="px-2 py-1.5 text-right font-medium">Stop</th>
              </tr>
            </thead>
            <tbody>
              {#each shownProcesses.slice(0, MAX_ROWS) as p (p.pid)}
                <tr class="border-t border-line/60 hover:bg-panel-hover/50">
                  <td class="px-2 py-1 text-right font-mono text-fg-muted">{p.pid}</td>
                  <td class="max-w-72 truncate px-2 py-1" title={p.nameRaw}>{p.name}</td>
                  <td class="px-2 py-1">{p.user}</td>
                  <td class="px-2 py-1 text-right tabular-nums">{p.cpuPct.toFixed(1)}</td>
                  <td class="px-2 py-1 text-right tabular-nums" title={p.memPct !== null ? `${p.memPct}% of memory` : ""}>{formatBytes(p.memKb * 1000)}</td>
                  <td class="px-2 py-1">
                    <div class="flex justify-end gap-0.5">
                      <button class="icon-btn h-6 w-6" title="Terminate (SIGTERM): ask it to stop" aria-label="Terminate {p.name} {p.pid}" disabled={p.pid <= 1 || !!signalling[p.pid]} onclick={() => signal(p, "TERM")}><OctagonX size={13} /></button>
                      <button class="icon-btn h-6 w-6 hover:text-danger" title="Force kill (SIGKILL): stop it at once" aria-label="Force kill {p.name} {p.pid}" disabled={p.pid <= 1 || !!signalling[p.pid]} onclick={() => signal(p, "KILL")}><Skull size={13} /></button>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if shownProcesses.length === 0}<p class="py-6 text-center text-xs text-fg-muted">{detail.processes.length ? "No process matches." : "No processes."}</p>{/if}
          {#if shownProcesses.length > MAX_ROWS}<p class="py-2 text-center text-xs text-fg-muted">Showing the first {MAX_ROWS} of {shownProcesses.length}. Search or sort to find the rest.</p>{/if}
          <p class="mt-2 text-[11px] text-fg-muted">CPU is a share of one core over the last second, so a busy multi-threaded program can pass 100.{detail.os === "Linux" ? "" : ` On ${detail.os} it is the system's own figure.`} Stopping a process of another user may need that user's rights; the host's answer is shown.</p>
        {:else if tab === "ports"}
          {#if note("ports")}<p class="mb-2 text-xs text-warning">{note("ports")}</p>{/if}
          <table class="w-full min-w-[34rem] text-left text-xs">
            <thead class="sticky top-0 bg-panel text-fg-muted"><tr><th class="px-2 py-1.5 font-medium">Port</th><th class="px-2 font-medium">Protocol</th><th class="px-2 font-medium">Listening on</th><th class="px-2 font-medium">Process</th></tr></thead>
            <tbody>
              {#each shownPorts as p (`${p.proto}|${p.address}|${p.port}|${p.pid}`)}
                <tr class="border-t border-line/60 hover:bg-panel-hover/50">
                  <td class="px-2 py-1 font-mono text-sm">{p.port}</td>
                  <td class="px-2 py-1 uppercase">{p.proto}{#if p.ipv6}<span class="ml-1 normal-case text-fg-muted">v6</span>{/if}</td>
                  <td class="px-2 py-1 font-mono">{p.address === "0.0.0.0" || p.address === "::" || p.address === "*" ? `all addresses (${p.address})` : p.address}</td>
                  <td class="px-2 py-1">{#if p.process}{p.process} <span class="text-fg-muted">({p.pid})</span>{:else}<span class="text-fg-muted">owner not shown</span>{/if}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if shownPorts.length === 0 && detail.ports.source !== "none"}<p class="py-6 text-center text-xs text-fg-muted">{detail.ports.items.length ? "No listener matches." : "Nothing is listening."}</p>{/if}
          {#if detail.ports.ownersMissing && detail.ports.source !== "proc"}<p class="mt-2 text-[11px] text-fg-muted">Some owners aren't shown: the host only tells a user about their own processes, so the rest needs root.</p>{/if}
        {:else}
          {#if note("interfaces")}<p class="mb-2 text-xs text-warning">{note("interfaces")}</p>{/if}
          <table class="w-full min-w-[40rem] text-left text-xs">
            <thead class="sticky top-0 bg-panel text-fg-muted"><tr><th class="px-2 py-1.5 font-medium">Name</th><th class="px-2 font-medium">State</th><th class="px-2 font-medium">Addresses</th><th class="px-2 font-medium">MAC</th><th class="px-2 text-right font-medium">MTU</th><th class="px-2 text-right font-medium">Received</th><th class="px-2 text-right font-medium">Sent</th></tr></thead>
            <tbody>
              {#each detail.interfaces as i (i.name)}
                {@const r = rateOf(i.name)}
                <tr class="border-t border-line/60 align-top hover:bg-panel-hover/50">
                  <td class="px-2 py-1 font-mono text-sm">{i.name}</td>
                  <td class="px-2 py-1"><Badge tone={i.state === "up" ? "success" : i.state === "down" ? "danger" : "neutral"}>{i.state}</Badge></td>
                  <td class="px-2 py-1 font-mono">{#each i.addresses as a (a.address)}<div>{a.address}{a.prefix !== null ? `/${a.prefix}` : ""}</div>{:else}<span class="text-fg-muted">—</span>{/each}</td>
                  <td class="px-2 py-1 font-mono">{i.mac ?? "—"}</td>
                  <td class="px-2 py-1 text-right tabular-nums">{i.mtu ?? "—"}</td>
                  <td class="px-2 py-1 text-right tabular-nums" title={r ? `${formatBytes(r.rxBytes)} in total` : ""}>{r ? formatRate(r.rxBps) : "—"}</td>
                  <td class="px-2 py-1 text-right tabular-nums" title={r ? `${formatBytes(r.txBytes)} in total` : ""}>{r ? formatRate(r.txBps) : "—"}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {/if}
  </div>
</Modal>
