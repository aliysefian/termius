<script lang="ts">
  import { ArrowDownToLine, ClipboardCopy, Columns3, Download, EllipsisVertical, Eraser, Filter, Pause, Play, Regex, Search, Server, Timer, WrapText, X } from "lucide-svelte";
  import DropMenu from "./DropMenu.svelte";
  import { FORMATS } from "$lib/ops/logexport";
  import { RANGE_MS, type TimeRange } from "$lib/ops/logfilter";
  import { SEVS, SEV_LABEL, type Sev } from "$lib/ops/logrows";
  import { opsLogs } from "$lib/stores/opslogs.svelte";
  import { settings } from "$lib/stores/settings.svelte";

  let { searchEl = $bindable() }: { searchEl?: HTMLInputElement } = $props();

  let width = $state(1200);
  const wide = $derived(width >= 1060);
  const medium = $derived(width >= 780);

  const f = $derived(opsLogs.filter);
  const set = (patch: Partial<typeof f>) => (opsLogs.filter = { ...opsLogs.filter, ...patch });
  const compiled = $derived(opsLogs.compiled);
  const total = $derived(opsLogs.paused ? opsLogs.shown.length : opsLogs.rows.length);

  const COLUMNS = [
    ["time", "Time"],
    ["host", "Host"],
    ["severity", "Severity"],
    ["unit", "Unit"],
  ] as const;
  const cols = $derived(settings.prefs.opsLogColumns);
  const toggleColumn = (id: string) => {
    const c = settings.prefs.opsLogColumns;
    settings.prefs.opsLogColumns = c.includes(id) ? c.filter((x) => x !== id) : [...c, id];
  };

  const toggleIn = (list: string[], v: string) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);

  // Hosts that can be filtered on: the ones in the stream, with how many events each has sent.
  const hostChoices = $derived(Object.entries(opsLogs.hostStates).map(([id, s]) => ({ id, label: opsLogs.label(id), count: s.count })).sort((a, b) => a.label.localeCompare(b.label)));
  // Units present in what is held, most frequent first (worked out when the menu opens).
  let unitChoices = $state<{ name: string; count: number }[]>([]);
  let unitQuery = $state("");
  function loadUnitChoices() {
    const counts = new Map<string, number>();
    for (const r of opsLogs.rows) {
      const u = r.unit || r.ident;
      if (u) counts.set(u, (counts.get(u) ?? 0) + 1);
    }
    unitChoices = [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => b.count - a.count || a.name.localeCompare(b.name)).slice(0, 300);
  }
  const unitShown = $derived(unitChoices.filter((u) => u.name.toLowerCase().includes(unitQuery.toLowerCase())).slice(0, 60));

  const RANGES: { id: TimeRange; label: string }[] = [
    { id: "live", label: "Live (everything received)" },
    { id: "5m", label: "Last 5 minutes" },
    { id: "15m", label: "Last 15 minutes" },
    { id: "1h", label: "Last hour" },
    { id: "custom", label: "Custom range" },
  ];
  const rangeLabel = $derived(f.range === "live" ? "Live" : f.range === "custom" ? "Custom" : `Last ${f.range.replace("m", " min").replace("h", " h")}`);
  const local = (ms: number | null) => {
    if (ms === null) return "";
    const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60_000);
    return d.toISOString().slice(0, 19);
  };
  const parseLocal = (v: string) => (v ? new Date(v).getTime() : null);
  void RANGE_MS;

  const sevActive = $derived(f.sev !== "all");
  const filterCount = $derived((f.hosts.length ? 1 : 0) + (f.units.length ? 1 : 0) + (sevActive ? 1 : 0) + (f.range !== "live" ? 1 : 0) + (f.text.trim() ? 1 : 0));
</script>

<section class="flex min-h-12 shrink-0 flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-line bg-panel px-4 py-1.5" aria-label="Log filters" bind:clientWidth={width} data-testid="log-toolbar">
  <div class="relative min-w-40 max-w-md flex-1">
    <Search size={13} class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
    <input
      bind:this={searchEl}
      id="ops-log-search"
      class="input h-8 w-full pl-8 pr-16 text-xs {compiled.error ? 'border-danger' : ''}"
      type="text"
      placeholder={f.regex ? "Regular expression…" : "Search messages, hosts, units…"}
      aria-label="Search the logs"
      aria-invalid={!!compiled.error}
      spellcheck="false"
      value={f.text}
      oninput={(e) => set({ text: e.currentTarget.value })}
      onkeydown={(e) => {
        if (e.key === "Escape" && f.text) {
          e.stopPropagation();
          set({ text: "" });
        }
      }}
    />
    <div class="absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5">
      {#if f.text}<button class="icon-btn h-6 w-6" aria-label="Clear the search" onclick={() => set({ text: "" })}><X size={12} /></button>{/if}
      <button class="icon-btn h-6 w-6 {f.regex ? 'bg-accent/20 text-accent' : ''}" aria-pressed={f.regex} aria-label="Regular expression" title="Treat the search as a regular expression" onclick={() => set({ regex: !f.regex })}><Regex size={13} /></button>
    </div>
  </div>
  {#if compiled.error}<span class="max-w-64 truncate text-[11px] text-danger" role="alert" title={compiled.error}>{compiled.error}</span>{/if}

  <div class="flex items-center gap-1">
    <select class="input h-8 w-32 py-0 text-xs" aria-label="Severity" value={f.sev} onchange={(e) => set({ sev: e.currentTarget.value as Sev | "all" })} title="Filters what has already arrived by how serious it is">
      <option value="all">All severities</option>
      {#each SEVS as s (s)}<option value={s}>{SEV_LABEL[s]}</option>{/each}
    </select>
    {#if sevActive}
      <button class="h-8 rounded-md border px-2 text-[11px] {f.atLeast ? 'border-accent/60 bg-accent/10 text-accent' : 'border-line text-fg-muted'}" aria-pressed={f.atLeast} title={f.atLeast ? `${SEV_LABEL[f.sev as Sev]} and worse. Click for only ${SEV_LABEL[f.sev as Sev].toLowerCase()}.` : `Only ${SEV_LABEL[f.sev as Sev].toLowerCase()}. Click to include worse ones.`} onclick={() => set({ atLeast: !f.atLeast })}>{f.atLeast ? "and worse" : "exactly"}</button>
    {/if}
  </div>

  <DropMenu ariaLabel="Filter by host" label={f.hosts.length ? `${f.hosts.length} host${f.hosts.length === 1 ? "" : "s"}` : "Hosts"} active={f.hosts.length > 0} width={260}>
    {#snippet icon()}<Server size={13} />{/snippet}
    {#snippet children()}
      {#if hostChoices.length === 0}<p class="px-2 py-3 text-center text-fg-muted">Follow some hosts first.</p>{/if}
      <ul>
        {#each hostChoices as h (h.id)}
          <li><label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-panel-hover"><input type="checkbox" class="accent-input" checked={f.hosts.includes(h.id)} onchange={() => set({ hosts: toggleIn(f.hosts, h.id) })} /><span class="min-w-0 flex-1 truncate">{h.label}</span><span class="text-fg-muted">{h.count.toLocaleString()}</span></label></li>
        {/each}
      </ul>
      {#if f.hosts.length}<button class="mt-1 w-full rounded px-2 py-1 text-left text-accent hover:underline" onclick={() => set({ hosts: [] })}>Show all hosts</button>{/if}
    {/snippet}
  </DropMenu>

  {#if medium}
    <DropMenu ariaLabel="Filter by unit" label={f.units.length ? (f.units.length === 1 ? f.units[0] : `${f.units.length} units`) : "Units"} active={f.units.length > 0} width={300} onopen={loadUnitChoices}>
      {#snippet icon()}<Filter size={13} />{/snippet}
      {#snippet children()}
        <input class="input mb-1 h-7 w-full text-xs" placeholder="Find a unit or program" aria-label="Find a unit" bind:value={unitQuery} />
        {#if unitShown.length === 0}<p class="px-2 py-3 text-center text-fg-muted">No units in what has been received yet.</p>{/if}
        <ul>
          {#each unitShown as u (u.name)}
            <li><label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-panel-hover"><input type="checkbox" class="accent-input" checked={f.units.includes(u.name)} onchange={() => set({ units: toggleIn(f.units, u.name) })} /><span class="min-w-0 flex-1 truncate font-mono">{u.name}</span><span class="text-fg-muted">{u.count.toLocaleString()}</span></label></li>
          {/each}
        </ul>
        {#if f.units.length}<button class="mt-1 w-full rounded px-2 py-1 text-left text-accent hover:underline" onclick={() => set({ units: [] })}>Show all units</button>{/if}
      {/snippet}
    </DropMenu>

    <DropMenu ariaLabel="Time range" label={rangeLabel} active={f.range !== "live"} width={300}>
      {#snippet icon()}<Timer size={13} />{/snippet}
      {#snippet children()}
        <p class="mb-1 px-2 text-[11px] text-fg-muted">This narrows what has already been received. To read older entries from the host, change <strong>History</strong> above and follow again.</p>
        <ul>
          {#each RANGES as r (r.id)}
            <li><label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-panel-hover"><input type="radio" name="ops-range" class="accent-input" checked={f.range === r.id} onchange={() => set({ range: r.id })} />{r.label}</label></li>
          {/each}
        </ul>
        {#if f.range === "custom"}
          <div class="mt-1 grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1 px-2">
            <label for="ops-from">From</label><input id="ops-from" type="datetime-local" step="1" class="input h-7 text-xs" value={local(f.from)} onchange={(e) => set({ from: parseLocal(e.currentTarget.value) })} />
            <label for="ops-to">To</label><input id="ops-to" type="datetime-local" step="1" class="input h-7 text-xs" value={local(f.to)} onchange={(e) => set({ to: parseLocal(e.currentTarget.value) })} />
          </div>
        {/if}
      {/snippet}
    </DropMenu>
  {/if}

  <span class="text-[11px] text-fg-muted" aria-live="polite" data-testid="line-count">{opsLogs.shown.length.toLocaleString()}{opsLogs.isFiltered ? ` of ${total.toLocaleString()}` : ""} events{#if opsLogs.filtered.timedOut} · stopped at the time limit{/if}</span>
  {#if filterCount > 0}<button class="text-[11px] text-accent hover:underline" onclick={() => (opsLogs.filter = { ...opsLogs.filter, text: "", sev: "all", hosts: [], units: [], range: "live", from: null, to: null })}>Clear filters</button>{/if}

  <div class="ml-auto flex items-center gap-1">
    <button
      class="inline-flex h-8 items-center gap-1 rounded-md border px-2 text-xs {opsLogs.paused ? 'border-warning/60 bg-warning/10 text-warning' : 'border-line text-fg-muted hover:bg-panel-hover hover:text-fg'}"
      aria-pressed={opsLogs.paused}
      title={opsLogs.paused ? "Resume the view" : "Hold the view still. Collecting from the hosts goes on; to stop that, use Stop Following."}
      onclick={() => opsLogs.togglePause()}
      data-testid="pause"
    >{#if opsLogs.paused}<Play size={13} /> Resume{:else}<Pause size={13} /> Pause{/if}</button>
    <button class="icon-btn h-8 w-8 {opsLogs.follow && !opsLogs.paused ? 'text-accent' : ''}" aria-pressed={opsLogs.follow} aria-label="Follow the newest events" title={opsLogs.follow ? "Staying at the newest events. Scroll up to stop." : "Jump to the newest events and follow them"} onclick={() => (opsLogs.follow = true)}><ArrowDownToLine size={14} /></button>

    {#if medium}
      <button class="icon-btn h-8 w-8 {settings.prefs.opsLogWrap ? 'bg-accent/15 text-accent' : ''}" aria-pressed={settings.prefs.opsLogWrap} aria-label="Wrap long messages" title="Wrap long messages" onclick={() => (settings.prefs.opsLogWrap = !settings.prefs.opsLogWrap)}><WrapText size={14} /></button>
      <DropMenu ariaLabel="Columns" align="right" width={230}>
        {#snippet icon()}<Columns3 size={13} />{/snippet}
        {#snippet children()}
          <ul>
            {#each COLUMNS as [id, name] (id)}
              <li><label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-panel-hover"><input type="checkbox" class="accent-input" checked={cols.includes(id)} onchange={() => toggleColumn(id)} />{name}</label></li>
            {/each}
          </ul>
          <div class="mt-1 border-t border-line px-2 pt-2">
            <label class="mb-1 block text-fg-muted" for="ops-max">Keep this many events</label>
            <select id="ops-max" class="input h-7 w-full py-0 text-xs" bind:value={settings.prefs.opsLogMax}>
              {#each [5000, 10000, 20000, 50000, 100000] as n (n)}<option value={n}>{n.toLocaleString()}</option>{/each}
            </select>
            <label class="mb-1 mt-2 block text-fg-muted" for="ops-order">Order</label>
            <select id="ops-order" class="input h-7 w-full py-0 text-xs" value={settings.prefs.opsLogOrder} onchange={(e) => opsLogs.setOrder(e.currentTarget.value as "time" | "arrival")} title="By time puts hosts' events in the order they happened (according to each host's own clock). By arrival keeps the order they reached this window.">
              <option value="time">By time</option>
              <option value="arrival">By arrival</option>
            </select>
          </div>
        {/snippet}
      </DropMenu>
      <button class="icon-btn h-8 w-8" aria-label="Copy the events shown" title="Copy the events shown" disabled={!opsLogs.shown.length} onclick={() => void opsLogs.copyShown()}><ClipboardCopy size={14} /></button>
    {/if}

    <DropMenu ariaLabel="Export the events shown" align="right" width={230} title="Save the events shown">
      {#snippet icon()}<Download size={14} />{/snippet}
      {#snippet children(close)}
        <p class="px-2 pb-1 text-[11px] text-fg-muted">{opsLogs.shown.length.toLocaleString()} event{opsLogs.shown.length === 1 ? "" : "s"} shown{opsLogs.rawHeld ? ". Secrets were not hidden when these were read." : ". Secrets are hidden."}</p>
        {#each FORMATS as fmt (fmt.id)}
          <button class="block w-full rounded px-2 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40" disabled={!opsLogs.shown.length} onclick={() => { close(); void opsLogs.exportShown(fmt.id); }}>{fmt.label}</button>
        {/each}
        {#if !medium}<button class="mt-1 block w-full rounded px-2 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40" disabled={!opsLogs.shown.length} onclick={() => { close(); void opsLogs.copyShown(); }}>Copy to the clipboard</button>{/if}
      {/snippet}
    </DropMenu>
    <button class="icon-btn h-8 w-8" aria-label="Clear the display" title="Clear this window's display. Nothing on a host is deleted." disabled={!opsLogs.rows.length} onclick={() => opsLogs.clear()}><Eraser size={14} /></button>

    {#if !medium}
      <DropMenu ariaLabel="More" align="right" width={240}>
        {#snippet icon()}<EllipsisVertical size={14} />{/snippet}
        {#snippet children(close)}
          <button class="block w-full rounded px-2 py-1.5 text-left hover:bg-panel-hover" onclick={() => { settings.prefs.opsLogWrap = !settings.prefs.opsLogWrap; close(); }}>{settings.prefs.opsLogWrap ? "Don't wrap messages" : "Wrap long messages"}</button>
          <div class="mt-1 border-t border-line px-2 pt-2 text-fg-muted">Columns</div>
          {#each COLUMNS as [id, name] (id)}
            <label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-panel-hover"><input type="checkbox" class="accent-input" checked={cols.includes(id)} onchange={() => toggleColumn(id)} />{name}</label>
          {/each}
        {/snippet}
      </DropMenu>
    {/if}
  </div>
  {#if !wide}<span class="sr-only">Some controls are in menus on a narrow window.</span>{/if}
</section>
