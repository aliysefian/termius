<script lang="ts">
  import { AlertTriangle, CornerDownRight, Eye, EyeOff, Info, Loader2, RefreshCw, ScrollText, ShieldQuestion, X } from "lucide-svelte";
  import * as api from "$lib/api";
  import { confidenceText, emptyState, endText, isCurrent, isStaleResult, listenText, parseTargetInput, startedText, stateText, targetText, udpNote, unitKind, type Investigation, type InvestigateTarget, type Protocol } from "$lib/investigate";
  import { ops } from "$lib/stores/ops.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { hostId, target, onclose, onretarget }: { hostId: Uuid; target: InvestigateTarget; onclose: () => void; onretarget?: (t: InvestigateTarget) => void } = $props();
  const label = $derived(vaultStore.hostById.get(hostId)?.data?.label ?? "host");

  let result = $state<Investigation | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let showCommands = $state(false);
  let now = $state(Date.now());
  let seq = 0;

  // Another input to look at, without leaving the drawer.
  let kind = $state<"pid" | "port">("pid");
  let text = $state("");
  let proto = $state<Protocol>("tcp");
  let inputError = $state<string | null>(null);

  /** One read, only when the target changes or the person presses Refresh. Nothing is polled. */
  async function load(t: InvestigateTarget) {
    const mine = ++seq;
    loading = true;
    error = null;
    try {
      const r = t.type === "pid" ? await api.hostSamples.investigate.pid(hostId, t.pid) : await api.hostSamples.investigate.port(hostId, t.port, t.protocol);
      if (!isCurrent(mine, seq)) return;
      result = r;
      now = Date.now();
    } catch (e) {
      if (!isCurrent(mine, seq)) return;
      error = errorMessage(e);
    } finally {
      if (isCurrent(mine, seq)) loading = false;
    }
  }
  $effect(() => {
    const t = target;
    result = null;
    void load(t);
  });

  function cancel() {
    seq++;
    loading = false;
    error = "Cancelled.";
    void api.hostSamples.cancel(hostId).catch(() => {});
  }
  function go() {
    const r = parseTargetInput(kind, text, proto);
    inputError = "error" in r ? r.error : null;
    if ("target" in r) onretarget?.(r.target);
  }
  function logs(unit: string) {
    ui.modal = null;
    void ops.viewLogs(hostId, unit);
  }

  const empty = $derived(result ? emptyState(result) : null);
  const stale = $derived(result ? isStaleResult(result, now) : false);
  const warnings = $derived((result?.warnings ?? []).filter((w) => w.code !== "process_denied" && w.code !== "ss_missing"));
</script>

<aside class="absolute inset-y-0 right-0 z-30 flex w-[28rem] max-w-full flex-col border-l border-line bg-bg shadow-2xl" aria-label="Why is this running?" data-testid="why-running">
  <header class="flex items-start gap-2 border-b border-line px-3 py-2">
    <div class="min-w-0 flex-1">
      <h3 class="text-sm font-semibold">Why is this running?</h3>
      <p class="truncate text-xs text-fg-muted">Host: {label} · Target: {targetText(target)}</p>
    </div>
    <button class="icon-btn h-7 w-7" title="Read it again" aria-label="Refresh" onclick={() => load(target)} disabled={loading}><RefreshCw size={14} class={loading ? "animate-spin" : ""} /></button>
    <button class="icon-btn h-7 w-7" title="Close" aria-label="Close" onclick={onclose}><X size={14} /></button>
  </header>

  <div class="min-h-0 flex-1 space-y-3 overflow-auto p-3 text-xs">
    <form class="flex items-center gap-1" onsubmit={(e) => { e.preventDefault(); go(); }}>
      <select class="input h-7 w-16 py-0 text-xs" aria-label="Look up by" bind:value={kind}><option value="pid">PID</option><option value="port">Port</option></select>
      <input class="input h-7 w-24 text-xs" inputmode="numeric" placeholder={kind === "pid" ? "4231" : "8080"} aria-label={kind === "pid" ? "Process id" : "Port"} bind:value={text} />
      {#if kind === "port"}<select class="input h-7 w-16 py-0 text-xs" aria-label="Protocol" bind:value={proto}><option value="tcp">TCP</option><option value="udp">UDP</option></select>{/if}
      <button class="btn-secondary py-0.5 text-xs" type="submit" disabled={loading || !text.trim()}>Investigate</button>
    </form>
    {#if inputError}<p class="text-danger" role="alert">{inputError}</p>{/if}

    {#if loading && !result}
      <div class="space-y-2" aria-busy="true" aria-label="Reading the host">
        <div class="flex items-center gap-2 text-fg-muted"><Loader2 size={14} class="animate-spin" /> Reading {label}…
          <button class="btn-secondary ml-auto py-0.5 text-xs" onclick={cancel}>Cancel</button></div>
        {#each [0, 1, 2] as i (i)}<div class="h-12 animate-pulse rounded-md bg-panel"></div>{/each}
      </div>
    {:else if error && !result}
      <p class="flex items-start gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-danger" role="alert"><AlertTriangle size={14} class="mt-0.5 shrink-0" /><span>{error}</span></p>
      <button class="btn-secondary text-xs" onclick={() => load(target)}>Try again</button>
    {:else if result}
      {#if loading}<p class="flex items-center gap-2 text-fg-muted"><Loader2 size={12} class="animate-spin" /> Reading again… <button class="btn-secondary ml-auto py-0.5 text-xs" onclick={cancel}>Cancel</button></p>{/if}
      {#if error}<p class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-warning" role="alert"><AlertTriangle size={14} class="mt-0.5 shrink-0" /><span>The last read failed ({error}); this is the one before it.</span></p>{/if}
      {#if stale}<p class="text-warning">This answer is over a minute old. Refresh before relying on it.</p>{/if}

      {#if empty}
        <div class="rounded-lg border border-line bg-panel p-4 text-center">
          <ShieldQuestion size={20} class="mx-auto mb-2 text-fg-muted" />
          <p class="font-medium">{empty.title}</p>
          <p class="mt-1 text-fg-muted">{empty.text}</p>
        </div>
      {:else}
        {#if result.sockets.length}
          <section>
            <h4 class="mb-1 font-medium text-fg-muted">Listening on</h4>
            <ul class="space-y-1">
              {#each result.sockets as s, i (i)}
                <li class="rounded-md border border-line bg-panel px-2 py-1.5">
                  <span class="font-mono">{listenText(s)}</span>
                  <span class="ml-1 text-fg-muted">{s.owners.length ? "→ " + s.owners.map((o) => `${o.name} (${o.pid})`).join(", ") : s.uid !== null ? `→ owner not shown (user id ${s.uid})` : "→ owner not shown"}</span>
                  {#if udpNote(s)}<p class="mt-1 flex gap-1 text-fg-muted"><Info size={12} class="mt-0.5 shrink-0" />{udpNote(s)}</p>{/if}
                </li>
              {/each}
            </ul>
          </section>
        {/if}
        {#each warnings as w (w.code)}
          <p class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-warning"><AlertTriangle size={13} class="mt-0.5 shrink-0" /><span>{w.message}</span></p>
        {/each}

        {#each result.subjects as s (s.process.pid)}
          <article class="space-y-2 rounded-lg border border-line bg-panel p-2.5" data-testid="subject">
            <section>
              <h4 class="mb-1 font-medium text-fg-muted">Process</h4>
              <dl class="grid grid-cols-[5rem_1fr] gap-x-2 gap-y-0.5">
                <dt class="text-fg-muted" title="Process id: the number the system gives a running program">Name</dt><dd class="font-mono">{s.process.name}</dd>
                <dt class="text-fg-muted">PID</dt><dd class="font-mono">{s.process.pid}{#if s.process.ppid !== null}<span class="text-fg-muted" title="Parent process id: the process that started this one"> · parent {s.process.ppid}</span>{/if}</dd>
                <dt class="text-fg-muted">User</dt><dd>{s.process.owner ?? (s.process.uid !== null ? `user id ${s.process.uid}` : "not shown")}</dd>
                <dt class="text-fg-muted">State</dt><dd>{stateText(s.process.state) ?? "not shown"}</dd>
                <dt class="text-fg-muted">Started</dt><dd>{startedText(s.process, now) ?? "not shown"}</dd>
                <dt class="text-fg-muted">Command</dt>
                <dd class="min-w-0">
                  {#if s.process.command === null}<span class="text-fg-muted">none (a kernel thread, or not readable)</span>
                  {:else if showCommands}<span class="break-all font-mono">{s.process.command}</span>
                  {:else}<span class="text-fg-muted">hidden: command lines can hold passwords</span>{/if}
                </dd>
              </dl>
              {#if s.process.command !== null}
                <button class="mt-1 inline-flex items-center gap-1 text-accent" onclick={() => (showCommands = !showCommands)}>{#if showCommands}<EyeOff size={12} /> Hide command{:else}<Eye size={12} /> Show command{/if}</button>
              {/if}
            </section>

            <section>
              <h4 class="mb-1 font-medium text-fg-muted" title="Each process is started by another; this is that chain, read from the host, top first">Started by</h4>
              <ol class="font-mono" aria-label="Parent chain">
                {#each s.ancestry as a, depth (a.pid)}
                  <li style="padding-left: {depth * 0.9}rem" class="flex items-center gap-1 {a.pid === s.process.pid ? 'font-semibold' : ''}">
                    {#if depth > 0}<CornerDownRight size={11} class="shrink-0 text-fg-muted" />{/if}{a.name} <span class="text-fg-muted">({a.pid})</span>{#if a.unit}<span class="ml-1 rounded bg-panel-hover px-1 font-sans text-[10px] text-fg-muted" title="The unit this process's own cgroup is inside">{a.unit}</span>{/if}
                  </li>
                {/each}
              </ol>
              {#if endText(s.ancestry_end)}<p class="mt-1 text-fg-muted">{endText(s.ancestry_end)}</p>{/if}
              <p class="mt-1 text-[11px] text-fg-muted">Read from the host. Having PID 1 at the top does not mean a service started it.</p>
            </section>

            <section>
              <h4 class="mb-1 font-medium text-fg-muted" title="The systemd unit whose control group (cgroup) this process is in">Managed by</h4>
              {#if s.service}
                {@const v = s.service}
                <p><span class="font-medium">{v.unit_name}</span> <span class="text-fg-muted">· {unitKind(v)}{v.state ? ` · ${v.state}` : ""}</span></p>
                <p class="text-fg-muted">{confidenceText(v)}</p>
                <details class="mt-1"><summary class="cursor-pointer text-fg-muted">Evidence</summary><ul class="ml-4 list-disc">{#each v.evidence as e (e)}<li>{e}</li>{/each}</ul></details>
                <button class="mt-1 inline-flex items-center gap-1 text-accent" onclick={() => logs(v.unit_name)}><ScrollText size={12} /> View logs</button>
              {:else}
                <p class="text-fg-muted">Service: Not identified</p>
              {/if}
            </section>

            {#if target.type === "port" && onretarget}
              <button class="inline-flex items-center gap-1 text-accent" onclick={() => onretarget({ type: "pid", pid: s.process.pid })}>Investigate PID {s.process.pid} on its own</button>
            {/if}
          </article>
        {/each}
      {/if}
      <p class="text-[11px] text-fg-muted">Read-only: one set of reads over the connection the app already keeps, nothing installed or changed on the host, and nothing run with extra rights. Read {new Date(result.taken_at_ms).toLocaleTimeString()}.</p>
    {/if}
  </div>
</aside>
