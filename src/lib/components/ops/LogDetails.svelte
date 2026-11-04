<script lang="ts">
  import { ClipboardCopy, Fingerprint, Filter, Search, SquareTerminal, Wrench, X } from "lucide-svelte";
  import { PRIORITY_NAMES, type Row } from "$lib/ops/logrows";
  import { ops } from "$lib/stores/ops.svelte";
  import { opsLogs } from "$lib/stores/opslogs.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let { row, height }: { row: Row; height: number } = $props();

  const around = $derived(opsLogs.contexts[row.id]);
  const stamp = $derived(new Date(row.ts));
  const unit = $derived(row.unit || row.ident);
  const isService = $derived(row.unit.endsWith(".service"));
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour12: false }) + "." + String(ms % 1000).padStart(3, "0");
  const when = (r: Row) => time(r.ts);

  const sevText = $derived(row.priority === null ? `${row.sev} (guessed from the words; a log file has no priority)` : `${PRIORITY_NAMES[row.priority]} (${row.priority})`);
  const facts = $derived<[string, string, boolean?][]>([
    ["Host", row.host],
    ["Journal's name for the host", row.journalHost ?? "—"],
    ["Unit", row.unit || "—", true],
    ["Program", row.ident || "—", true],
    ["Process", row.pid === null ? "—" : String(row.pid)],
    ["Time", `${stamp.toLocaleString()} (${stamp.toISOString()})`],
    ["Severity", sevText],
    ["Position", row.cursor ?? "— (no journal position)", true],
  ]);
</script>

<div class="grid min-h-0 gap-3 overflow-hidden border-t border-line/60 bg-base/60 px-3 py-2 text-xs lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]" style="height: {height}px" data-testid="log-details">
  <div class="flex min-h-0 flex-col gap-2">
    <div class="flex flex-wrap items-center gap-1">
      <button class="btn-secondary h-6 px-2 text-[11px]" onclick={() => void opsLogs.copyMessage(row)}><ClipboardCopy size={11} /> Copy message</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" onclick={() => void opsLogs.copyEvent(row)}><ClipboardCopy size={11} /> Copy event</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" onclick={() => void opsLogs.copyId(row)} title="Copies the journal position when there is one"><Fingerprint size={11} /> Copy id</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" onclick={() => opsLogs.filterHost(row)}><Filter size={11} /> This host</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" disabled={!unit} onclick={() => opsLogs.filterUnit(row)}><Filter size={11} /> This unit</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" onclick={() => opsLogs.searchSimilar(row)}><Search size={11} /> Similar</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" onclick={() => ui.openTerminal(row.hostId, row.host)}><SquareTerminal size={11} /> Terminal</button>
      <button class="btn-secondary h-6 px-2 text-[11px]" disabled={!isService} title={isService ? "Open this service in Services" : "Only entries of a service can be opened there"} onclick={() => ops.viewService(row.hostId, row.unit)}><Wrench size={11} /> View service</button>
      <button class="icon-btn ml-auto h-6 w-6" aria-label="Close the details" onclick={() => (opsLogs.expanded = null)}><X size={12} /></button>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre class="max-h-24 min-h-8 shrink-0 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-base p-2 font-mono text-[12px]" tabindex="0" aria-label="Full message">{row.message}{#if row.truncated}<span class="text-warning"> (cut: the message was longer than what is kept)</span>{/if}</pre>
    <div class="min-h-0 flex-1 overflow-auto">
      <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
        {#each facts as [k, v, mono] (k)}
          <dt class="text-fg-muted">{k}</dt>
          <dd class="min-w-0 break-all {mono ? 'font-mono' : ''}">{v}</dd>
        {/each}
      </dl>
      {#if row.fields.length}
        <details class="mt-2">
          <summary class="cursor-pointer text-fg-muted hover:text-fg">Journal fields ({row.fields.length})</summary>
          <dl class="mt-1 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 font-mono text-[11px]">
            {#each row.fields as [k, v] (k)}
              <dt class="text-fg-muted">{k}</dt>
              <dd class="min-w-0 break-all">{v}</dd>
            {/each}
          </dl>
        </details>
      {/if}
    </div>
  </div>

  <div class="flex min-h-0 flex-col" aria-label="Nearby events">
    <h4 class="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">Nearby in the host's journal</h4>
    <div class="min-h-0 flex-1 overflow-auto rounded border border-line bg-base font-mono text-[11px]">
      {#if !around || around.state === "loading"}
        <p class="p-3 text-center font-sans text-fg-muted">Reading around this event…</p>
      {:else if around.state === "unavailable" || around.state === "error"}
        <p class="p-3 font-sans text-fg-muted">{around.error ?? "Not available."}</p>
      {:else}
        {#each around.before as r, i (i)}
          <div class="flex gap-2 px-2 py-0.5 text-fg-muted"><span class="shrink-0">{when(r)}</span><span class="shrink-0 text-fg-muted/70">{r.unit || r.ident}</span><span class="min-w-0 break-all">{r.message}</span></div>
        {/each}
        <div class="flex gap-2 border-y border-accent/40 bg-accent/10 px-2 py-0.5 text-fg"><span class="shrink-0">{when(row)}</span><span class="shrink-0">{row.unit || row.ident}</span><span class="min-w-0 break-all">{row.message}</span></div>
        {#each around.after as r, i (i)}
          <div class="flex gap-2 px-2 py-0.5 text-fg-muted"><span class="shrink-0">{when(r)}</span><span class="shrink-0 text-fg-muted/70">{r.unit || r.ident}</span><span class="min-w-0 break-all">{r.message}</span></div>
        {/each}
        {#if around.before.length + around.after.length === 0}<p class="p-2 font-sans text-fg-muted">The journal has nothing else near this entry.</p>{/if}
      {/if}
    </div>
    <p class="mt-1 text-[10px] text-fg-muted">Read from the host just now, from all units. Only entries the journal still has.</p>
  </div>
</div>
