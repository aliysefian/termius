<script lang="ts">
  import { Loader2, Play, RotateCcw, Square } from "lucide-svelte";
  import Combobox, { type ComboOption } from "../Combobox.svelte";
  import HostSelector from "./HostSelector.svelte";
  import { HISTORY_CHOICES, PRIORITY_CHOICES, opsLogs } from "$lib/stores/opslogs.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { UNIT } from "$lib/ops/logs";

  const p = settings.prefs;
  const unitOptions = $derived<ComboOption[]>(opsLogs.unitNames().map((n) => ({ value: n, label: n })));
  const unitBad = $derived(p.opsLogUnit.trim() !== "" && !UNIT.test(p.opsLogUnit.trim()));
  const file = $derived(p.opsLogSource === "file");
  const kernel = $derived(p.opsLogSource === "kernel");
  const history = $derived(HISTORY_CHOICES.filter((h) => !file || h.file));
  const pending = $derived(opsLogs.pendingHosts);
  const hostsChanged = $derived(pending.add.length + pending.remove.length > 0);
  const specChanged = $derived(opsLogs.pendingSpec);
  const blocker = $derived(opsLogs.blocker);
  const busy = $derived(opsLogs.starting || opsLogs.stopping);

  // A saved history choice that a log file can't use falls back to the nearest one.
  $effect(() => {
    if (file && p.opsLogHistory.startsWith("since:")) p.opsLogHistory = "last:100";
  });

  function loadUnits() {
    const first = opsLogs.hosts.find((id) => opsLogs.usable(id));
    if (first) void opsLogs.loadUnits(first);
  }

  const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;
</script>

<section class="shrink-0 border-b border-line bg-panel px-4 py-2" aria-label="Stream configuration" data-testid="stream-config">
  <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
    <div class="min-w-64 flex-1"><HostSelector hosts={opsLogs.hostOptions} chosen={opsLogs.hosts} onchange={(ids) => (opsLogs.hosts = ids)} /></div>
  </div>

  <div class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
    <label class="flex items-center gap-1.5 text-xs text-fg-muted">
      <span>Source</span>
      <select class="input h-8 w-36 py-0 text-xs" bind:value={p.opsLogSource} aria-label="Log source">
        <option value="journal">System journal</option>
        <option value="kernel">Kernel journal</option>
        <option value="file">A log file</option>
      </select>
    </label>

    {#if file}
      <label class="flex min-w-48 flex-1 items-center gap-1.5 text-xs text-fg-muted">
        <span>Path</span>
        <input class="input h-8 min-w-0 flex-1 py-0 font-mono text-xs" bind:value={p.opsLogPath} placeholder="/var/log/syslog" spellcheck="false" aria-label="Path of the log file on the hosts" />
      </label>
    {:else}
      <div class="flex items-center gap-1.5 text-xs text-fg-muted">
        <span>Unit</span>
        <Combobox
          class="w-52"
          inputClass="h-8 py-0 text-xs font-mono"
          ariaLabel="Unit to follow"
          options={unitOptions}
          bind:value={p.opsLogUnit}
          creatable
          clearable
          placeholder={kernel ? "Kernel messages" : "All units"}
          createLabel={(q) => `Use “${q}”`}
          emptyText={opsLogs.hosts.length ? "Type a unit name, such as nginx.service" : "Choose a host to list its units"}
        />
        {#if !kernel}<button class="text-[11px] text-accent hover:underline" onclick={loadUnits} title="List the units on the first chosen host">List units</button>{/if}
      </div>
      <label class="flex items-center gap-1.5 text-xs text-fg-muted">
        <span>Priority</span>
        <select class="input h-8 w-40 py-0 text-xs" bind:value={p.opsLogPriority} aria-label="Priority and worse to read from the host" title="Asked of the host: only entries this serious or worse are sent. To narrow what is already on screen, use the severity filter below.">
          {#each PRIORITY_CHOICES as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
        </select>
      </label>
    {/if}

    <label class="flex items-center gap-1.5 text-xs text-fg-muted">
      <span>History</span>
      <select class="input h-8 w-36 py-0 text-xs" bind:value={p.opsLogHistory} aria-label="Initial history to load" title="Read from the host when following starts. Older entries are only loaded by starting again.">
        {#each history as h (h.value)}<option value={h.value}>{h.label}</option>{/each}
      </select>
    </label>

    <div class="ml-auto flex items-center gap-2">
      {#if opsLogs.running}
        <button class="btn-secondary h-8 text-xs" disabled={busy} onclick={() => void opsLogs.stop()} data-testid="stop-following" title="End the remote commands and close the connections">
          {#if opsLogs.stopping}<Loader2 size={13} class="animate-spin" /> Stopping…{:else}<Square size={13} /> Stop Following{/if}
        </button>
      {:else}
        <button class="btn-primary h-8 text-xs" disabled={busy || !!blocker} aria-describedby="follow-why" onclick={() => void opsLogs.start()} data-testid="follow-logs">
          {#if opsLogs.starting}<Loader2 size={13} class="animate-spin" /> Starting…{:else}<Play size={13} /> Follow Logs{/if}
        </button>
      {/if}
    </div>
  </div>

  <div class="mt-1.5 min-h-4 text-[11px]" id="follow-why" aria-live="polite">
    {#if unitBad}
      <span class="text-danger">The unit must look like nginx.service.</span>
    {:else if opsLogs.startError}
      <span class="text-danger">{opsLogs.startError}</span>
    {:else if !opsLogs.running && blocker}
      <span class="text-fg-muted">{blocker}</span>
    {:else if opsLogs.running && (hostsChanged || specChanged)}
      <span class="inline-flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md border border-warning/40 bg-warning/10 px-2 py-1 text-warning" data-testid="pending-changes">
        <span>
          Not applied yet:
          {#if hostsChanged}{[pending.add.length ? `add ${plural(pending.add.length, "host")}` : "", pending.remove.length ? `stop following ${plural(pending.remove.length, "host")}` : ""].filter(Boolean).join(", ")}{/if}{#if hostsChanged && specChanged}; {/if}{#if specChanged}the source, unit, priority or history changed{/if}.
        </span>
        {#if hostsChanged}<button class="font-medium text-fg underline hover:text-accent" onclick={() => void opsLogs.applyHosts()}>Apply to the running stream</button>{/if}
        {#if specChanged}<button class="inline-flex items-center gap-1 font-medium text-fg underline hover:text-accent" onclick={() => void opsLogs.restart()}><RotateCcw size={11} /> Restart with the new settings</button>{/if}
        {#if hostsChanged}<button class="text-fg-muted underline hover:text-fg" onclick={() => opsLogs.revertHosts()}>Discard host changes</button>{/if}
      </span>
    {:else if !opsLogs.running}
      <span class="text-fg-muted">Reads over a second connection per host. Nothing is installed on the host.</span>
    {/if}
  </div>
</section>
