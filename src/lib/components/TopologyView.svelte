<script lang="ts">
  import { onMount } from "svelte";
  import { Maximize2, Minus, Plus, SquareTerminal, Activity } from "lucide-svelte";
  import EmptyState from "./EmptyState.svelte";
  import { COLUMN, NODE_H, NODE_W, bounds, buildGraph, connectedOnly, focus, layout, type Graph, type GraphInput, type GraphNode } from "$lib/resources/graph";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  // -- what is drawn: only links the data really holds ----------------------------------------------------
  const input = $derived<GraphInput>({
    hosts: vaultStore.hosts
      .filter((h) => h.data && !h.deleted && (!h.data.protocol || h.data.protocol === "ssh"))
      .map((h) => ({
        id: h.id,
        label: h.data!.label,
        hostname: h.data!.hostname,
        port: h.data!.port,
        env: vaultStore.effectiveEnv(h.data),
        tags: h.data!.tags ?? [],
        group: h.data!.group,
        jumpId: vaultStore.effectiveJump(h.data, h.id),
        proxyId: vaultStore.effectiveProxy(h.data),
      })),
    proxies: vaultStore.proxies.filter((p) => p.data && !p.deleted).map((p) => ({ id: p.id, name: p.data!.name, kind: p.data!.spec.kind })),
    forwards: vaultStore.forwards
      .filter((f) => f.data && !f.deleted)
      .map((f) => ({ id: f.id, label: f.data!.label, hostId: f.data!.host_id, summary: f.data!.kind === "dynamic" ? `SOCKS :${f.data!.bind_port}` : `${f.data!.kind === "remote" ? "remote " : ""}:${f.data!.bind_port} → ${f.data!.dest_host}:${f.data!.dest_port}` })),
    databases: vaultStore.dbConnections.filter((d) => d.data && !d.deleted).map((d) => ({ id: d.id, name: d.data!.name, engine: d.data!.engine, env: d.data!.environment ?? "", sshHostId: d.data!.ssh_host_id })),
  });
  const full = $derived(buildGraph(input));

  let query = $state("");
  let env = $state("");
  let showAll = $state(false);
  let selected = $state<string | null>(null);

  const graph = $derived.by<Graph>(() => {
    let g = showAll ? full : connectedOnly(full);
    const q = query.trim().toLowerCase();
    if (q || env) g = focus(g, (n) => (!env || n.env === env) && (!q || [n.label, n.sub, n.group, ...n.tags].some((x) => x.toLowerCase().includes(q))));
    return g;
  });
  const placed = $derived(layout(graph));
  const size = $derived(bounds(placed));
  const nodeById = $derived(new Map(graph.nodes.map((n) => [n.id, n])));
  const links = $derived(graph.edges.map((e) => ({ ...e, a: nodeById.get(e.from)!, b: nodeById.get(e.to)! })));
  const chosen = $derived(selected ? (nodeById.get(selected) ?? null) : null);
  const chosenLinks = $derived(chosen ? links.filter((l) => l.from === chosen.id || l.to === chosen.id) : []);
  const hiddenHosts = $derived(full.nodes.filter((n) => n.type === "host").length - connectedOnly(full).nodes.filter((n) => n.type === "host").length);

  // -- pan and zoom -------------------------------------------------------------------------------------
  let box = $state<HTMLDivElement>();
  let width = $state(800);
  let height = $state(500);
  let zoom = $state(1);
  let pan = $state({ x: 24, y: 24 });
  let drag: { x: number; y: number; px: number; py: number } | null = null;

  function fit() {
    if (!size.width || !size.height) return;
    const z = Math.min(1.2, Math.max(0.2, Math.min((width - 48) / size.width, (height - 48) / size.height)));
    zoom = z;
    pan = { x: (width - size.width * z) / 2, y: Math.max(24, (height - size.height * z) / 2) };
  }
  function zoomBy(factor: number, cx = width / 2, cy = height / 2) {
    const z = Math.min(3, Math.max(0.15, zoom * factor));
    pan = { x: cx - ((cx - pan.x) / zoom) * z, y: cy - ((cy - pan.y) / zoom) * z };
    zoom = z;
  }
  onMount(() => {
    fit();
    // The wheel zooms the drawing, not the page, so the listener must be allowed to stop the page scrolling.
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = box!.getBoundingClientRect();
      zoomBy(e.deltaY < 0 ? 1.12 : 1 / 1.12, e.clientX - r.left, e.clientY - r.top);
    };
    box?.addEventListener("wheel", wheel, { passive: false });
    return () => box?.removeEventListener("wheel", wheel);
  });
  // A different set of nodes is worth fitting again; moving one around is not.
  let shownKey = "";
  $effect(() => {
    const key = graph.nodes.map((n) => n.id).join("|");
    if (key !== shownKey) {
      shownKey = key;
      fit();
    }
  });

  function down(e: PointerEvent) {
    if ((e.target as Element).closest("[data-node]")) return;
    drag = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y };
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  const move = (e: PointerEvent) => drag && (pan = { x: drag.px + e.clientX - drag.x, y: drag.py + e.clientY - drag.y });
  const up = () => (drag = null);

  // -- looks and actions --------------------------------------------------------------------------------
  const ring: Record<string, string> = { production: "stroke-danger", staging: "stroke-warning", development: "stroke-success" };
  const word: Record<GraphNode["type"], string> = { host: "HOST", proxy: "PROXY", tunnel: "TUNNEL", database: "DATABASE" };
  const dot = (n: GraphNode) => {
    if (n.type !== "host") return "";
    const h = vaultStore.health[n.ref];
    return h?.state === "up" ? "fill-success" : h?.state === "down" ? "fill-danger" : h ? "fill-warning" : "fill-fg-muted/40";
  };
  const edgePath = (a: GraphNode, b: GraphNode) => {
    const p = placed.get(a.id)!;
    const q = placed.get(b.id)!;
    const [x1, y1, x2, y2] = [p.x + NODE_W, p.y + NODE_H / 2, q.x, q.y + NODE_H / 2];
    const dx = Math.max(40, (x2 - x1) / 2);
    return `M${x1},${y1} C${x1 + dx},${y1} ${x2 - dx},${y2} ${x2},${y2}`;
  };
  const hostOf = (n: GraphNode) => (n.type === "host" ? n : null);
  function openTerminal(n: GraphNode) {
    ui.openTerminal(n.ref, n.label);
  }
</script>

<div class="flex min-h-0 min-w-0 flex-1 flex-col bg-base">
  <div class="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-4 py-2.5">
    <h1 class="mr-2 text-sm font-semibold">Topology</h1>
    <input class="input h-8 w-56 text-xs" placeholder="Search name, address, tag, group…" aria-label="Search" bind:value={query} />
    <select class="input h-8 w-36 py-0 text-xs" bind:value={env} aria-label="Environment">
      <option value="">All environments</option>
      <option value="production">Production</option>
      <option value="staging">Staging</option>
      <option value="development">Development</option>
    </select>
    <label class="flex items-center gap-1.5 text-xs text-fg-muted" title="Hosts with no jump host, proxy, tunnel or database are left out, so a long list doesn't bury the picture.">
      <input type="checkbox" class="accent-input" bind:checked={showAll} /> Show hosts with no links{hiddenHosts ? ` (${hiddenHosts})` : ""}
    </label>
    <span class="ml-auto flex items-center gap-1">
      <span class="mr-2 text-[11px] text-fg-muted">{graph.nodes.length} items · {graph.edges.length} links</span>
      <button class="icon-btn" title="Zoom out" aria-label="Zoom out" onclick={() => zoomBy(1 / 1.25)}><Minus size={15} /></button>
      <button class="icon-btn" title="Zoom in" aria-label="Zoom in" onclick={() => zoomBy(1.25)}><Plus size={15} /></button>
      <button class="icon-btn" title="Fit to the window" aria-label="Fit to the window" onclick={fit}><Maximize2 size={15} /></button>
    </span>
  </div>

  <div class="flex min-h-0 flex-1">
    <div class="relative min-w-0 flex-1 overflow-hidden" bind:this={box} bind:clientWidth={width} bind:clientHeight={height}>
      {#if graph.nodes.length === 0}
        <div class="flex h-full items-center justify-center">
          <EmptyState art="containers" text={full.nodes.length ? "Nothing matches the search." : "No links to draw yet. A jump host, a proxy, a tunnel rule or a database reached through a host shows up here. Only links you have saved are drawn."} />
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <svg class="h-full w-full cursor-grab select-none active:cursor-grabbing" role="img" aria-label="Map of {graph.nodes.length} items and {graph.edges.length} links. The same links are listed as text in the panel." onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up}>
          <defs>
            <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" class="fill-fg-muted" /></marker>
          </defs>
          <g transform="translate({pan.x},{pan.y}) scale({zoom})">
            {#each links as l (l.id)}
              {@const lit = !!chosen && (l.from === chosen.id || l.to === chosen.id)}
              <path d={edgePath(l.a, l.b)} fill="none" class={lit ? "stroke-accent" : "stroke-fg-muted/60"} stroke-width={lit ? 2.2 : 1.4} marker-end="url(#arrow)" />
            {/each}
            {#each graph.nodes as n (n.id)}
              {@const p = placed.get(n.id)!}
              <g data-node transform="translate({p.x},{p.y})" role="button" tabindex="0" aria-label="{word[n.type]} {n.label}, {n.sub}{n.env ? `, ${n.env}` : ''}" aria-pressed={selected === n.id} class="cursor-pointer outline-none" onclick={() => (selected = selected === n.id ? null : n.id)} onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), (selected = selected === n.id ? null : n.id))}>
                <rect width={NODE_W} height={NODE_H} rx="8" class="fill-panel {selected === n.id ? 'stroke-accent' : (ring[n.env] ?? 'stroke-line')}" stroke-width={selected === n.id ? 2.5 : 1.5} />
                <text x="12" y="17" class="fill-fg-muted" font-size="9" letter-spacing="0.6">{word[n.type]}</text>
                {#if n.type === "host"}<circle cx={NODE_W - 14} cy="14" r="4.5" class={dot(n)} />{/if}
                <text x="12" y="33" class="fill-fg" font-size="13" font-weight="600">{n.label.length > 24 ? n.label.slice(0, 23) + "…" : n.label}</text>
                <text x="12" y="46" class="fill-fg-muted" font-size="10">{n.sub.length > 30 ? n.sub.slice(0, 29) + "…" : n.sub}</text>
              </g>
            {/each}
          </g>
        </svg>
      {/if}
    </div>

    <aside class="w-72 shrink-0 overflow-y-auto border-l border-line bg-panel p-4 text-sm" aria-label="Details">
      {#if chosen}
        <div class="text-[10px] font-semibold tracking-wide text-fg-muted">{word[chosen.type]}</div>
        <h2 class="break-words text-[1rem] font-semibold text-fg">{chosen.label}</h2>
        <p class="break-words font-mono text-xs text-fg-muted">{chosen.sub}</p>
        {#if chosen.env}<p class="mt-1 text-xs capitalize">{chosen.env}</p>{/if}
        {#if chosen.group}<p class="mt-1 text-xs text-fg-muted">Group {chosen.group}</p>{/if}
        {#if chosen.tags.length}<p class="mt-1 text-xs text-fg-muted">{chosen.tags.join(" · ")}</p>{/if}
        {#if hostOf(chosen)}
          <div class="mt-3 flex flex-wrap gap-2">
            <button class="btn-primary py-1 text-xs" onclick={() => openTerminal(chosen)}><SquareTerminal size={13} /> Open a terminal</button>
            <button class="btn-secondary py-1 text-xs" onclick={() => (ui.modal = { kind: "host-monitor", id: chosen.ref })}><Activity size={13} /> Monitor</button>
          </div>
        {/if}
        <h3 class="mb-1 mt-4 text-xs font-semibold">Links ({chosenLinks.length})</h3>
        <ul class="space-y-1 text-xs">
          {#each chosenLinks as l (l.id)}
            <li><button class="text-left hover:underline" onclick={() => (selected = l.from === chosen.id ? l.to : l.from)}>{l.a.label} <span class="text-fg-muted">→ {l.label} →</span> {l.b.label}</button></li>
          {:else}
            <li class="text-fg-muted">No links.</li>
          {/each}
        </ul>
      {:else}
        <h2 class="text-sm font-semibold text-fg">What this shows</h2>
        <p class="mt-1 text-xs text-fg-muted">Only links you have saved: a host's jump host, the proxy it goes through, a tunnel rule and the host it runs over, a database reached through a host. Nothing is guessed. Lines run left to right, from where traffic enters to where it ends.</p>
        <p class="mt-2 text-xs text-fg-muted">Click an item for details. Drag to move around, scroll to zoom. The dot on a host is its last reachability check (Fleet runs them).</p>
        <h3 class="mb-1 mt-4 text-xs font-semibold">All links ({links.length})</h3>
        <ul class="space-y-1 text-xs">
          {#each links as l (l.id)}
            <li><button class="text-left hover:underline" onclick={() => (selected = l.to)}>{l.a.label} <span class="text-fg-muted">→ {l.label} →</span> {l.b.label}</button></li>
          {/each}
        </ul>
      {/if}
    </aside>
  </div>
</div>
