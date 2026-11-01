<script lang="ts">
  import { ChevronDown, ChevronRight, Copy, Ellipsis, FolderTree, Loader2, Play, RotateCw, ScrollText, Square, Trash2 } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Badge from "../Badge.svelte";
  import StateBadge from "./StateBadge.svelte";
  import ContainerMenu from "./ContainerMenu.svelte";
  import { runMenu, type DrawerTab } from "./actions";
  import { formatAge, portLabels } from "$lib/containerdata";
  import { collapseKey, COLUMNS, projectState, rowPrimary, type ColumnId, type GroupSummary } from "$lib/containerview";
  import { containers } from "$lib/stores/containers.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type ContainerInfo } from "$lib/types";

  let { sourceKey, rows, groups, columns, now, isDocker, onopen }: {
    sourceKey: string;
    /** The containers shown, in order (also the keyboard order). */
    rows: ContainerInfo[];
    /** Compose groups, or null for a flat list. */
    groups: GroupSummary[] | null;
    columns: ColumnId[];
    now: number;
    isDocker: boolean;
    /** Show the details drawer (on a tab). */
    onopen: (tab?: DrawerTab) => void;
  } = $props();

  const src = $derived(containers.sources[sourceKey]);
  let menu = $state<{ c: ContainerInfo; x: number; y: number } | null>(null);
  let projectInfo = $state<string | null>(null);
  const has = (id: ColumnId) => columns.includes(id);
  const colspan = $derived(3 + columns.length + 1);
  /** The rows that can be reached by keyboard: not the ones in a collapsed project. */
  const navRows = $derived(groups ? groups.flatMap((g) => (settings.prefs.containerCollapsed[collapseKey(sourceKey, g.project)] ? [] : g.containers)) : rows);
  const allChecked = $derived(rows.length > 0 && rows.every((r) => containers.checked.has(r.id)));
  const someChecked = $derived(rows.some((r) => containers.checked.has(r.id)));
  const collapsed = (project: string | null) => !!settings.prefs.containerCollapsed[collapseKey(sourceKey, project)];
  const toggle = (project: string | null) => {
    const k = collapseKey(sourceKey, project);
    settings.prefs.containerCollapsed = { ...settings.prefs.containerCollapsed, [k]: !settings.prefs.containerCollapsed[k] };
  };
  const label = (id: ColumnId) => COLUMNS.find((c) => c.id === id)?.label ?? id;

  function pick(c: ContainerInfo) {
    containers.select(c.id);
    onopen();
  }

  function openMenu(e: MouseEvent, c: ContainerInfo) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const context = e.type === "contextmenu";
    menu = { c, x: context ? e.clientX : r.right - 224, y: context ? e.clientY : r.bottom + 4 };
  }

  function primary(c: ContainerInfo) {
    runMenu(sourceKey, c, rowPrimary(c) === "stop" ? "stop" : rowPrimary(c) === "resume" ? "resume" : "start", () => {});
  }

  function key(e: KeyboardEvent, c: ContainerInfo) {
    if ((e.target as HTMLElement).closest("button, input, a")) return;
    const i = navRows.findIndex((r) => r.id === c.id);
    const go = (to: number) => {
      const next = navRows[Math.min(navRows.length - 1, Math.max(0, to))];
      if (!next) return;
      e.preventDefault();
      containers.select(next.id);
      queueMicrotask(() => document.querySelector<HTMLElement>(`[data-container-row="${next.id}"]`)?.focus());
    };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(navRows.length - 1);
    else if (e.key === "Enter") {
      e.preventDefault();
      pick(c);
    } else if (e.key === " ") {
      e.preventDefault();
      containers.toggleChecked(c.id);
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      menu = { c, x: r.left + 40, y: r.bottom };
    }
  }

  async function copyText(text: string, what: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied the ${what}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  const service = (c: ContainerInfo) => c.labels["com.docker.compose.service"] ?? c.labels["io.podman.compose.service"] ?? null;
</script>

{#snippet containerRow(c: ContainerInfo, indent: boolean)}
  {@const busy = !!containers.busy[c.id]}
  {@const selected = containers.selectedId === c.id}
  {@const main = rowPrimary(c)}
  {@const ports = portLabels(c.ports)}
  {@const st = src?.stats[c.id] ?? src?.stats[c.id.slice(0, 12)]}
  <tr
    data-container-row={c.id}
    tabindex="0"
    aria-selected={selected}
    class="group cursor-pointer outline-none transition-colors focus-visible:bg-accent/10 {selected ? 'bg-accent/10' : 'hover:bg-panel-hover'}"
    onclick={() => pick(c)}
    onkeydown={(e) => key(e, c)}
    oncontextmenu={(e) => {
      e.preventDefault();
      containers.select(c.id);
      openMenu(e, c);
    }}
  >
    <td class="relative w-10 border-b border-line/60 py-2.5 pl-4 pr-1">
      {#if selected}<span class="absolute inset-y-0 left-0 w-0.5 bg-accent" aria-hidden="true"></span>{/if}
      <input type="checkbox" class="accent-input" aria-label="Select {c.name}" checked={containers.checked.has(c.id)} onclick={(e) => e.stopPropagation()} onchange={() => containers.toggleChecked(c.id)} />
    </td>
    <td class="max-w-[18rem] border-b border-line/60 py-2.5 pr-3 {indent ? 'pl-6' : 'pl-3'}">
      <div class="truncate text-sm font-medium" title="{c.name} · {c.id}">{c.name}</div>
      <div class="flex items-center gap-1.5 text-[11px] text-fg-muted">
        <span class="font-mono">{c.id.slice(0, 12)}</span>
        <button class="inline-flex opacity-0 hover:text-fg focus-visible:opacity-100 group-hover:opacity-100" title="Copy the container ID" aria-label="Copy the ID of {c.name}" onclick={(e) => { e.stopPropagation(); void copyText(c.id, "container ID"); }}><Copy size={10} /></button>
        {#if indent && service(c)}<span class="truncate">· {service(c)}</span>{/if}
        {#if c.pod}<Badge tone="neutral" title="Podman pod">pod {c.pod}</Badge>{/if}
      </div>
    </td>
    {#if has("image")}<td class="max-w-[14rem] truncate border-b border-line/60 px-3 py-2.5 font-mono text-xs text-fg-muted" title={c.image}>{c.image}</td>{/if}
    <td class="border-b border-line/60 px-3 py-2.5"><StateBadge {c} /></td>
    {#if has("ports")}
      <td class="max-w-[14rem] border-b border-line/60 px-3 py-2.5 font-mono text-[11px] tabular-nums">
        {#each ports.slice(0, 2) as p (p.text)}<div class="truncate {p.published ? '' : 'text-fg-muted'}" title={p.published ? "Published" : "Exposed, not published"}>{p.text}</div>{/each}
        {#if ports.length > 2}<div class="text-fg-muted" title={ports.map((p) => p.text).join("\n")}>+{ports.length - 2} more</div>{/if}
        {#if ports.length === 0}<span class="text-fg-muted">–</span>{/if}
      </td>
    {/if}
    {#if has("cpu")}<td class="border-b border-line/60 px-3 py-2.5 font-mono text-xs tabular-nums">{#if st && st.cpu_pct !== null}{st.cpu_pct.toFixed(1)}%{:else}<span class="text-fg-muted">{c.state === "running" ? "…" : "–"}</span>{/if}</td>{/if}
    {#if has("memory")}<td class="border-b border-line/60 px-3 py-2.5 font-mono text-xs tabular-nums" title={st ? `net ${st.net_io} · disk ${st.block_io} · ${st.pids ?? "?"} processes` : ""}>{#if st && st.mem_usage}{st.mem_usage.split(" / ")[0]}{:else}<span class="text-fg-muted">{c.state === "running" ? "…" : "–"}</span>{/if}</td>{/if}
    {#if has("size")}<td class="border-b border-line/60 px-3 py-2.5 text-xs text-fg-muted">{c.size ?? "–"}</td>{/if}
    {#if has("age")}<td class="border-b border-line/60 px-3 py-2.5 text-xs tabular-nums text-fg-muted" title={c.created ? new Date(c.created * 1000).toLocaleString() : ""}>{formatAge(c.created, now)}</td>{/if}
    {#if has("created")}<td class="whitespace-nowrap border-b border-line/60 px-3 py-2.5 text-xs text-fg-muted">{c.created ? new Date(c.created * 1000).toLocaleDateString() : "–"}</td>{/if}
    <td class="w-28 border-b border-line/60 py-2.5 pl-2 pr-3 text-right">
      <div class="inline-flex items-center gap-0.5">
        <button class="icon-btn h-8 w-8" title={main === "stop" ? "Stop" : main === "resume" ? "Resume" : "Start"} aria-label="{main === 'stop' ? 'Stop' : main === 'resume' ? 'Resume' : 'Start'} {c.name}" disabled={busy} onclick={(e) => { e.stopPropagation(); primary(c); }}>
          {#if busy}<Loader2 size={15} class="animate-spin" />{:else if main === "stop"}<Square size={15} />{:else}<Play size={15} />{/if}
        </button>
        <button class="icon-btn h-8 w-8" title="Logs" aria-label="Logs of {c.name}" onclick={(e) => { e.stopPropagation(); containers.select(c.id); onopen("logs"); }}><ScrollText size={15} /></button>
        <button class="icon-btn h-8 w-8" title="More actions" aria-label="More actions for {c.name}" aria-haspopup="menu" onclick={(e) => { e.stopPropagation(); openMenu(e, c); }}><Ellipsis size={16} /></button>
      </div>
    </td>
  </tr>
{/snippet}

<div class="min-h-0 flex-1 overflow-auto">
  <table class="w-full border-separate border-spacing-0 text-sm">
    <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
      <tr>
        <th class="w-10 border-b border-line py-2 pl-4 pr-1">
          <input type="checkbox" class="accent-input" aria-label="Select all containers shown" checked={allChecked} indeterminate={someChecked && !allChecked} onchange={(e) => (containers.checked = e.currentTarget.checked ? new Set(rows.map((r) => r.id)) : new Set())} />
        </th>
        <th class="border-b border-line py-2 pl-3 pr-3">Name</th>
        {#if has("image")}<th class="border-b border-line px-3 py-2">{label("image")}</th>{/if}
        <th class="border-b border-line px-3 py-2">State</th>
        {#if has("ports")}<th class="border-b border-line px-3 py-2">{label("ports")}</th>{/if}
        {#if has("cpu")}<th class="border-b border-line px-3 py-2">{label("cpu")}</th>{/if}
        {#if has("memory")}<th class="border-b border-line px-3 py-2">{label("memory")}</th>{/if}
        {#if has("size")}<th class="border-b border-line px-3 py-2">{label("size")}</th>{/if}
        {#if has("age")}<th class="border-b border-line px-3 py-2">{label("age")}</th>{/if}
        {#if has("created")}<th class="border-b border-line px-3 py-2">{label("created")}</th>{/if}
        <th class="border-b border-line py-2 pl-2 pr-4 text-right"><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      {#if groups}
        {#each groups as g (g.project ?? "\u0000none")}
          {@const pbusy = g.project ? !!containers.busy[`project:${g.project}`] : false}
          {@const folded = collapsed(g.project)}
          {@const pstate = projectState(g)}
          <tr class="bg-base/60">
            <td class="border-b border-line/60 px-4 py-2" colspan={colspan - 1}>
              <div class="flex flex-wrap items-center gap-2">
                <button class="flex items-center gap-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" aria-expanded={!folded} aria-label="{folded ? 'Expand' : 'Collapse'} {g.project ?? 'containers without a project'}" onclick={() => toggle(g.project)}>
                  {#if folded}<ChevronRight size={14} />{:else}<ChevronDown size={14} />{/if}
                  <FolderTree size={14} class={g.project ? "text-accent" : "text-fg-muted"} aria-hidden="true" />
                  <span class="text-sm font-semibold {g.project ? '' : 'text-fg-muted'}">{g.project ?? "Standalone containers"}</span>
                </button>
                {#if g.project}<Badge tone="accent" title="Docker Compose project">compose</Badge>{/if}
                <span class="text-xs {pstate === 'running' ? 'text-success' : pstate === 'partial' ? 'text-warning' : 'text-fg-muted'}">{g.running} of {g.containers.length} running</span>
                {#if g.unhealthy}<span class="text-xs text-danger">{g.unhealthy} unhealthy</span>{/if}
                {#if g.project && projectInfo === g.project}
                  <span class="basis-full pl-6 text-[11px] text-fg-muted">
                    {#if g.workingDir}<span class="mr-3">Directory <span class="font-mono text-fg">{g.workingDir}</span> <button class="inline-flex align-middle hover:text-fg" aria-label="Copy the directory" onclick={() => void copyText(g.workingDir!, "directory")}><Copy size={10} /></button></span>{/if}
                    {#if g.configFiles}<span>Compose file <span class="font-mono text-fg">{g.configFiles}</span> <button class="inline-flex align-middle hover:text-fg" aria-label="Copy the Compose file path" onclick={() => void copyText(g.configFiles!, "Compose file path")}><Copy size={10} /></button></span>{/if}
                    {#if !g.workingDir && !g.configFiles}No directory or Compose file was recorded for this project.{/if}
                  </span>
                {/if}
              </div>
            </td>
            <td class="border-b border-line/60 py-1.5 pl-2 pr-3 text-right">
              {#if g.project}
                <div class="inline-flex items-center gap-0.5">
                  <button class="icon-btn h-8 w-8" title={!isDocker ? "Compose actions need Docker" : g.running === g.containers.length ? "All of its containers are running" : `Start every container of ${g.project}`} aria-label="Start all of {g.project}" disabled={!isDocker || pbusy || g.running === g.containers.length} onclick={() => containers.compose(sourceKey, g.project!, "start", g.containers.length)}><Play size={14} /></button>
                  <button class="icon-btn h-8 w-8" title={!isDocker ? "Compose actions need Docker" : g.running === 0 ? "None of its containers is running" : `Stop every container of ${g.project}`} aria-label="Stop all of {g.project}" disabled={!isDocker || pbusy || g.running === 0} onclick={() => containers.compose(sourceKey, g.project!, "stop", g.containers.length)}><Square size={14} /></button>
                  <button class="icon-btn h-8 w-8" title={!isDocker ? "Compose actions need Docker" : `Restart every container of ${g.project}`} aria-label="Restart all of {g.project}" disabled={!isDocker || pbusy} onclick={() => containers.compose(sourceKey, g.project!, "restart", g.containers.length)}><RotateCw size={14} class={pbusy ? "animate-spin" : ""} /></button>
                  <button class="icon-btn h-8 w-8 {projectInfo === g.project ? 'text-accent' : ''}" title="Project details: directory and Compose file" aria-label="Details of {g.project}" aria-pressed={projectInfo === g.project} onclick={() => (projectInfo = projectInfo === g.project ? null : g.project)}><FolderTree size={14} /></button>
                  <button class="icon-btn h-8 w-8 hover:text-danger" title={!isDocker ? "Compose actions need Docker" : `Take ${g.project} down: remove its containers and networks, keep its volumes`} aria-label="Take down {g.project}" disabled={!isDocker || pbusy} onclick={() => containers.compose(sourceKey, g.project!, "down", g.containers.length)}><Trash2 size={14} /></button>
                </div>
              {/if}
            </td>
          </tr>
          {#if !folded}
            {#each g.containers as c (c.id)}{@render containerRow(c, true)}{/each}
          {/if}
        {/each}
      {:else}
        {#each rows as c (c.id)}{@render containerRow(c, false)}{/each}
      {/if}
    </tbody>
  </table>
</div>

{#if menu}
  <ContainerMenu c={menu.c} busy={!!containers.busy[menu.c.id]} x={menu.x} y={menu.y} onpick={(id) => runMenu(sourceKey, menu!.c, id, (t) => { containers.select(menu!.c.id); onopen(t); })} onclose={() => (menu = null)} />
{/if}
