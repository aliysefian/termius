<script lang="ts">
  import { Maximize2, ZoomIn, ZoomOut } from "lucide-svelte";
  import { NODE_H, NODE_W, type Neighborhood } from "$lib/infra/graph";
  import { infra } from "$lib/stores/infra.svelte";
  import { STATUS_TONE, TYPE_ICON } from "./style";

  let { graph }: { graph: Neighborhood } = $props();

  let k = $state(1);
  let tx = $state(16);
  let ty = $state(16);
  let box = $state<HTMLDivElement | null>(null);
  let drag: { x: number; y: number } | null = null;
  const pos = $derived(new Map(graph.nodes.map((n) => [n.id, n])));

  function fit() {
    if (!box || graph.width === 0) return;
    const w = box.clientWidth;
    const h = box.clientHeight;
    k = Math.min(1.5, Math.max(0.3, Math.min((w - 32) / graph.width, (h - 32) / graph.height)));
    tx = (w - graph.width * k) / 2;
    ty = (h - graph.height * k) / 2;
  }
  // Fit again when the neighbourhood changes (a different resource, or a refresh that changed what is around it).
  $effect(() => {
    void graph.nodes.length;
    void graph.nodes[0]?.id;
    fit();
  });
  function zoom(f: number) {
    k = Math.min(2.5, Math.max(0.25, k * f));
  }
  const colour = (type: string) => (type === "port" ? "stroke-fg-muted/60" : "stroke-line");
</script>

<div bind:this={box} class="relative h-full min-h-[240px] overflow-hidden rounded-lg border border-line bg-panel" data-testid="infra-graph">
  <svg
    class="h-full w-full cursor-grab touch-none select-none active:cursor-grabbing"
    role="img"
    aria-label="Relationships of the selected resource"
    onwheel={(e) => { e.preventDefault(); zoom(e.deltaY < 0 ? 1.1 : 0.9); }}
    onpointerdown={(e) => { if ((e.target as Element).closest("[data-node]")) return; drag = { x: e.clientX - tx, y: e.clientY - ty }; (e.currentTarget as Element).setPointerCapture(e.pointerId); }}
    onpointermove={(e) => { if (drag) { tx = e.clientX - drag.x; ty = e.clientY - drag.y; } }}
    onpointerup={() => (drag = null)}
  >
    <g transform="translate({tx} {ty}) scale({k})">
      {#each graph.edges as e (`${e.from}>${e.to}`)}
        {@const a = pos.get(e.from)}
        {@const b = pos.get(e.to)}
        {#if a && b}
          <path d="M{a.x + NODE_W} {a.y + NODE_H / 2} C{a.x + NODE_W + 30} {a.y + NODE_H / 2}, {b.x - 30} {b.y + NODE_H / 2}, {b.x} {b.y + NODE_H / 2}" class="fill-none stroke-fg-muted/50" stroke-width="1.4"><title>{e.type === "publishes_port" ? "publishes port" : "contains"}: {e.evidence}</title></path>
        {/if}
      {/each}
      {#each graph.nodes as n (n.id)}
        {@const I = TYPE_ICON[n.resource.type]}
        <g data-node transform="translate({n.x} {n.y})" role="button" tabindex="0" aria-label="{n.resource.name}, {n.resource.statusText}" class="cursor-pointer outline-none" onclick={() => infra.select(n.id)} onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") infra.select(n.id); }}>
          <rect width={NODE_W} height={NODE_H} rx="8" class="fill-base {n.id === infra.selectedId ? 'stroke-accent' : colour(n.resource.type)}" stroke-width={n.id === infra.selectedId ? 2 : 1} opacity={n.resource.freshness === "stale" ? 0.6 : 1} />
          <foreignObject width={NODE_W} height={NODE_H}>
            <div class="flex h-full items-center gap-2 px-2 text-xs">
              <I size={15} class="shrink-0 text-fg-muted" />
              <div class="min-w-0 flex-1">
                <div class="truncate font-medium">{n.resource.name}</div>
                <div class="truncate text-[10px] text-fg-muted">{n.resource.statusText}</div>
              </div>
              <span class="h-2.5 w-2.5 shrink-0 rounded-full {STATUS_TONE[n.resource.status]}"></span>
            </div>
          </foreignObject>
        </g>
      {/each}
    </g>
  </svg>
  <div class="absolute right-2 top-2 flex flex-col gap-1">
    <button class="icon-btn h-7 w-7 bg-base" title="Zoom in" aria-label="Zoom in" onclick={() => zoom(1.2)}><ZoomIn size={14} /></button>
    <button class="icon-btn h-7 w-7 bg-base" title="Zoom out" aria-label="Zoom out" onclick={() => zoom(0.8)}><ZoomOut size={14} /></button>
    <button class="icon-btn h-7 w-7 bg-base" title="Fit to view" aria-label="Fit to view" onclick={fit}><Maximize2 size={14} /></button>
  </div>
  {#if graph.truncated}<p class="absolute bottom-2 left-2 rounded bg-base/90 px-2 py-1 text-[11px] text-warning">Only the nearest {graph.nodes.length} are drawn. Select a node to see what is around it.</p>{/if}
</div>
