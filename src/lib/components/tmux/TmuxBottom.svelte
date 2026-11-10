<script lang="ts">
  import { Check, ChevronDown, ChevronUp, Clock, Copy, History, ScrollText, Search, SquareTerminal, X } from "lucide-svelte";
  import { untrack } from "svelte";
  import { paneKey, tmuxMonitor as mon, type BottomTab } from "$lib/stores/tmuxmonitor.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { filterOutput, hostOfServer } from "$lib/tmux/monitor";
  import TmuxScreen from "./TmuxScreen.svelte";
  import TmuxSearch from "./TmuxSearch.svelte";

  // Below the previews: the text of the selected pane (Output), a search over every watched pane, what changed on the hosts while
  // this page was open (Events), and what was done from here and how it went (Activity). Each is backed by real data; none is a
  // placeholder, and none is saved: they are gone when the app closes.
  const b = $derived(mon.prefs.bottom);
  const tabs = [
    { id: "output", label: "Output", icon: SquareTerminal },
    { id: "search", label: "Search", icon: Search },
    { id: "events", label: "Events", icon: Clock },
    { id: "activity", label: "Activity", icon: History },
  ] as const satisfies readonly { id: BottomTab; label: string; icon: typeof Search }[];

  const sel = $derived(mon.selectedRow);
  const target = $derived(mon.focusTarget);

  // -- output -----------------------------------------------------------------------------------------------------------------
  let paneChoice = $state(""); // "" follows the selection
  let query = $state("");
  let regex = $state(false);
  let caseSensitive = $state(false);
  let copied = $state(false);

  const choices = $derived(target ? target.session.windows.flatMap((w) => w.panes.map((p) => ({ id: p.id, label: `${p.id} ${p.command || "shell"} (${w.index}: ${w.name})` }))) : []);
  const outPane = $derived(paneChoice && choices.some((c) => c.id === paneChoice) ? paneChoice : (target?.pane?.id ?? ""));
  const outKey = $derived(sel && outPane ? paneKey(sel.server, outPane) : null);
  const preview = $derived(outKey ? mon.previews[outKey] : undefined);
  const out = $derived(preview?.capture ? filterOutput(preview.capture.text, query, { regex, caseSensitive, max: b.lines }) : null);

  // The Output tab asks the store to keep its pane captured while it is on screen (written untracked, cleared when it goes).
  const wanted = $derived(b.open && b.tab === "output" ? outKey : null);
  $effect(() => {
    const k = wanted;
    untrack(() => {
      mon.outputKey = k;
      if (k && !mon.previews[k]) void mon.capture(k);
    });
  });
  $effect(() => () => untrack(() => (mon.outputKey = null)));

  async function copy() {
    if (!out) return;
    try {
      await navigator.clipboard.writeText(out.lines.join("\n"));
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      mon.record(false, "Copy output failed", "The clipboard is not available.");
    }
  }

  // -- events / activity --------------------------------------------------------------------------------------------------------
  let onlyHost = $state(false);
  const hostLabel = (server: string) => {
    const d = vaultStore.hostById.get(hostOfServer(server) as never)?.data;
    return d?.label || d?.hostname || "host";
  };
  const events = $derived(onlyHost && sel ? mon.events.filter((e) => e.server === sel.server) : mon.events);
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour12: false });
  const KIND: Record<string, string> = { session_created: "NEW", session_ended: "END", session_renamed: "NAME", window_created: "NEW", window_ended: "END", window_renamed: "NAME", pane_created: "NEW", pane_ended: "END", attached: "ATTACH", detached: "DETACH", server_restarted: "SERVER" };
  const tone = (kind: string) => (/ended|detached|restarted/.test(kind) ? "bg-warning/15 text-warning" : "bg-accent/15 text-accent");

  function pick(id: BottomTab) {
    mon.prefs.bottom.tab = id;
    mon.prefs.bottom.open = true;
  }
  function tabKey(e: KeyboardEvent) {
    const at = tabs.findIndex((t) => t.id === b.tab);
    const d = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const next = tabs[(at + d + tabs.length) % tabs.length];
    pick(next.id);
    queueMicrotask(() => document.getElementById(`tmux-bottom-${next.id}`)?.focus());
  }
</script>

<section class="flex min-h-0 flex-col border-t border-line bg-panel/30" style={b.open ? `height:${b.height}px` : ""} aria-label="Output, search, events and activity">
  <div class="flex shrink-0 items-center gap-1 px-2 py-1.5" role="tablist" aria-label="Bottom panel" tabindex="-1" onkeydown={tabKey}>
    {#each tabs as t (t.id)}
      <button
        id="tmux-bottom-{t.id}"
        role="tab"
        aria-selected={b.open && b.tab === t.id}
        tabindex={b.tab === t.id ? 0 : -1}
        class="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs transition-colors {b.open && b.tab === t.id ? 'bg-accent-solid text-white' : 'text-fg-muted hover:bg-panel-hover hover:text-fg'}"
        onclick={() => pick(t.id)}
      ><t.icon size={13} /> {t.label}{#if t.id === "events" && mon.events.length}<span class="rounded-full bg-base/40 px-1.5 text-[10px] tabular-nums">{mon.events.length}</span>{/if}</button>
    {/each}
    <button class="ml-auto grid h-7 w-7 place-items-center rounded-md text-fg-muted hover:bg-panel-hover hover:text-fg" aria-expanded={b.open} aria-label={b.open ? "Collapse the panel" : "Expand the panel"} title={b.open ? "Collapse" : "Expand"} onclick={() => (mon.prefs.bottom.open = !b.open)}>
      {#if b.open}<ChevronDown size={15} />{:else}<ChevronUp size={15} />{/if}
    </button>
  </div>

  {#if b.open}
    <div class="min-h-0 flex-1 overflow-hidden" role="tabpanel" aria-label={tabs.find((t) => t.id === b.tab)?.label}>
      {#if b.tab === "output"}
        <div class="flex h-full min-h-0 flex-col">
          <div class="flex shrink-0 flex-wrap items-center gap-2 px-3 pb-2 text-xs">
            <div class="relative min-w-40 flex-1 sm:max-w-xs">
              <Search size={13} class="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-muted" />
              <input class="w-full rounded-md border border-line bg-base py-1 pl-7 pr-2 text-xs focus:border-accent focus:outline-none" placeholder="Filter this output…" aria-label="Filter the output shown" bind:value={query} />
            </div>
            <label class="flex items-center gap-1 text-fg-muted"><input type="checkbox" bind:checked={regex} /> Regex</label>
            <label class="flex items-center gap-1 text-fg-muted"><input type="checkbox" bind:checked={caseSensitive} /> Case</label>
            <select class="max-w-56 rounded-md border border-line bg-base px-2 py-1 text-xs" aria-label="Pane" value={paneChoice} onchange={(e) => (paneChoice = e.currentTarget.value)} disabled={!choices.length}>
              <option value="">{choices.length ? "Selected pane" : "No pane"}</option>
              {#each choices as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
            </select>
            <select class="rounded-md border border-line bg-base px-2 py-1 text-xs" aria-label="Lines shown" value={b.lines} onchange={(e) => { mon.prefs.bottom.lines = Number(e.currentTarget.value); if (outKey) void mon.capture(outKey); }}>
              {#each [50, 100, 200, 500] as n (n)}<option value={n}>{n} lines</option>{/each}
            </select>
            <button class="ml-auto flex items-center gap-1 rounded-md border border-line px-2 py-1 text-fg-muted hover:bg-panel-hover hover:text-fg disabled:opacity-50" disabled={!out?.lines.length} onclick={copy} aria-label="Copy the lines shown">
              {#if copied}<Check size={12} /> Copied{:else}<Copy size={12} /> Copy{/if}
            </button>
          </div>
          <div class="min-h-0 flex-1 border-t border-line/60">
            {#if !target}
              <p class="p-4 text-xs text-fg-muted">Select a session, window or pane to see its output here.</p>
            {:else if out?.error}
              <p class="p-4 text-xs text-danger" role="alert">{out.error}</p>
            {:else if out}
              {#if out.lines.length}
                <TmuxScreen text={out.lines.join("\n")} ansi={false} maxLines={b.lines} highlight={query ? { text: query, caseSensitive, regex } : null} ariaLabel="Output of pane {outPane}" />
              {:else}
                <p class="p-4 text-xs text-fg-muted">{query ? "No line matches the filter." : "This pane has no output yet."}</p>
              {/if}
            {:else if preview?.error}
              <div class="p-4 text-xs text-fg-muted" role="status">
                <p>Unable to capture pane output. {preview.error}</p>
                <button class="mt-2 rounded border border-line px-2 py-1 hover:bg-panel-hover" onclick={() => outKey && mon.capture(outKey)}>Retry</button>
              </div>
            {:else}
              <p class="p-4 text-xs text-fg-muted">Capturing the pane…</p>
            {/if}
          </div>
        </div>
      {:else if b.tab === "search"}
        <TmuxSearch embedded focus={false} />
      {:else if b.tab === "events"}
        <div class="flex h-full min-h-0 flex-col">
          <div class="flex shrink-0 items-center gap-3 px-3 pb-2 text-xs text-fg-muted">
            <label class="flex items-center gap-1"><input type="checkbox" bind:checked={onlyHost} disabled={!sel} /> Selected host only</label>
            <span>Changes seen between readings while this page was open.</span>
            {#if mon.events.length}<button class="ml-auto rounded border border-line px-2 py-0.5 hover:bg-panel-hover hover:text-fg" onclick={() => (mon.events = [])}>Clear</button>{/if}
          </div>
          <ul class="min-h-0 flex-1 divide-y divide-line/50 overflow-y-auto border-t border-line/60 font-mono text-xs" aria-live="polite">
            {#each events as e, i (e.at + e.text + i)}
              <li class="flex items-center gap-3 px-3 py-1.5">
                <span class="shrink-0 tabular-nums text-fg-muted">{time(e.at)}</span>
                <span class="w-14 shrink-0 rounded px-1.5 py-0.5 text-center text-[10px] {tone(e.kind)}">{KIND[e.kind]}</span>
                <span class="hidden w-28 shrink-0 truncate text-fg-muted sm:inline">{hostLabel(e.server)}</span>
                <span class="min-w-0 flex-1 truncate font-sans" title={e.text}>{e.text}</span>
              </li>
            {:else}
              <li class="p-4 font-sans text-fg-muted">No changes yet. Events appear when a session, window or pane is created, renamed or ended, or a client attaches or detaches, while this page is open.</li>
            {/each}
          </ul>
        </div>
      {:else}
        <ul class="h-full min-h-0 divide-y divide-line/50 overflow-y-auto border-t border-line/60 text-xs" aria-live="polite">
          {#each mon.activity as a, i (a.at + a.text + i)}
            <li class="flex items-start gap-3 px-3 py-1.5">
              <span class="shrink-0 font-mono tabular-nums text-fg-muted">{time(a.at)}</span>
              <span class="mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-full {a.ok ? 'bg-success/20 text-success' : 'bg-danger/20 text-danger'}" title={a.ok ? "Succeeded" : "Failed"}>{#if a.ok}<Check size={10} />{:else}<X size={10} />{/if}</span>
              <span class="min-w-0 flex-1"><span>{a.text}</span>{#if a.detail}<span class="block text-fg-muted">{a.detail}</span>{/if}</span>
            </li>
          {:else}
            <li class="p-4 text-fg-muted">Nothing done yet. Creating, renaming, splitting, killing and attaching are listed here, with whether they worked.</li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</section>
