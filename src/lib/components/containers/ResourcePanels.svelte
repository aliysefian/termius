<script lang="ts">
  // Images, volumes and networks of a host: the same tables as before in the workspace's look, plus Inspect.
  import { Download, FileJson, Loader2, Trash2 } from "lucide-svelte";
  import Badge from "../Badge.svelte";
  import EmptyState from "../EmptyState.svelte";
  import Modal from "../Modal.svelte";
  import ContainerInspectPanel from "./ContainerInspectPanel.svelte";
  import * as api from "$lib/api";
  import { formatAge, imageRef, isAnonymousVolume, usedByText, volumeLabel } from "$lib/containerdata";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { Container } from "lucide-svelte";
  import type { ContainerImage, ContainerNetwork, ContainerVolume, ResourceKind } from "$lib/types";

  let { sourceKey, kind, images, volumes, networks, imagesError, resourcesLoaded, now, query }: {
    sourceKey: string;
    kind: "images" | "volumes" | "networks";
    images: ContainerImage[];
    volumes: ContainerVolume[];
    networks: ContainerNetwork[];
    imagesError: string | null;
    /** Volumes and networks arrive with a second request. */
    resourcesLoaded: boolean;
    now: number;
    /** Whether a search is narrowing the lists (for the empty message). */
    query: boolean;
  } = $props();

  const src = $derived(containers.sources[sourceKey]);
  let inspecting = $state<{ kind: ResourceKind; id: string; title: string } | null>(null);

  const th = "border-b border-line px-3 py-2";
  const td = "border-b border-line/60 px-3 py-2.5";
  const inspect = (k: ResourceKind, id: string, title: string) => (inspecting = { kind: k, id, title });
</script>

<div class="min-h-0 flex-1 overflow-auto">
  {#if kind === "images"}
    {#if imagesError}
      <p class="m-4 whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 font-mono text-xs text-warning" role="alert">Images couldn't be listed: {imagesError}</p>
    {/if}
    {#if images.length === 0 && !imagesError}
      <EmptyState compact={query} icon={Container} text={query ? "No image matches the search." : `No images on ${src?.label}.`} />
    {:else if images.length}
      <table class="w-full border-separate border-spacing-0 text-sm">
        <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
          <tr>
            <th class="{th} pl-4">Repository</th><th class={th}>Tag</th><th class={th}>Image ID</th><th class="{th} text-right">Size</th><th class="{th} text-right">Created</th><th class="{th} text-right">Containers</th><th class="{th} pr-4 text-right"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each images as i (i.id + i.repository + i.tag)}
            {@const ref = imageRef(i)}
            <tr class="hover:bg-panel-hover">
              <td class="{td} max-w-[20rem] truncate pl-4 font-mono text-[13px]" title={i.repository}>{i.repository}</td>
              <td class="{td} font-mono text-xs">{i.tag}</td>
              <td class="{td} font-mono text-xs text-fg-muted">{i.id.slice(0, 12)}</td>
              <td class="{td} text-right text-xs tabular-nums">{i.size_text || "–"}</td>
              <td class="{td} text-right text-xs text-fg-muted" title={i.created ? new Date(i.created * 1000).toLocaleString() : ""}>{formatAge(i.created, now)}</td>
              <td class="{td} text-right text-xs {i.containers ? '' : 'text-fg-muted'}">{i.containers ?? "–"}</td>
              <td class="{td} pr-3 text-right">
                <button class="icon-btn h-8 w-8" title="Inspect (JSON)" aria-label="Inspect image {ref}" onclick={() => inspect("image", ref, ref)}><FileJson size={15} /></button>
                <button class="icon-btn h-8 w-8 hover:text-danger" title={i.containers ? `Used by ${i.containers} container${i.containers === 1 ? "" : "s"}; removing the image doesn't stop them` : "Remove this image…"} aria-label="Remove image {ref}" disabled={!!containers.busy[`image:${ref}`]} onclick={() => containers.removeResource(sourceKey, "image", ref, ref === i.id ? i.id.slice(0, 12) : ref)}><Trash2 size={15} /></button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if !resourcesLoaded}
    <div class="flex items-center justify-center gap-2 p-10 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Loading {kind}…</div>
  {:else if kind === "volumes"}
    {#if volumes.length === 0}
      <EmptyState compact={query} icon={Container} text={query ? "No volume matches the search." : `No volumes on ${src?.label}.`} />
    {:else}
      <table class="w-full border-separate border-spacing-0 text-sm">
        <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
          <tr><th class="{th} pl-4">Name</th><th class={th}>Driver</th><th class={th}>Used by</th><th class={th}>Mountpoint</th>{#if src?.sizes}<th class="{th} text-right">Size</th>{/if}<th class="{th} pr-4 text-right"><span class="sr-only">Actions</span></th></tr>
        </thead>
        <tbody>
          {#each volumes as v (v.name)}
            <tr class="hover:bg-panel-hover">
              <td class="{td} max-w-[20rem] pl-4" title={v.name}>
                <div class="truncate font-mono text-[13px]">{isAnonymousVolume(v.name) ? v.name.slice(0, 12) : v.name}</div>
                <div class="flex gap-1">
                  {#if isAnonymousVolume(v.name)}<Badge tone="neutral" title="Created without a name, so it is known by its ID">anonymous</Badge>{/if}
                  {#if v.labels["com.docker.compose.project"]}<Badge tone="accent" title="Compose project">{v.labels["com.docker.compose.project"]}</Badge>{/if}
                </div>
              </td>
              <td class="{td} text-xs">{v.driver}</td>
              <td class="{td} max-w-[16rem] truncate text-xs {v.used_by.length ? '' : 'text-fg-muted'}" title={v.used_by.join("\n")}>{usedByText(v.used_by)}</td>
              <td class="{td} max-w-[18rem] truncate font-mono text-[11px] text-fg-muted" title={v.mountpoint ?? ""}>{v.mountpoint ?? "–"}</td>
              {#if src?.sizes}<td class="{td} text-right text-xs text-fg-muted">{v.size ?? "–"}</td>{/if}
              <td class="{td} pr-3 text-right">
                <button class="icon-btn h-8 w-8" title="Inspect (JSON)" aria-label="Inspect volume {volumeLabel(v.name)}" onclick={() => inspect("volume", v.name, volumeLabel(v.name))}><FileJson size={15} /></button>
                <button class="icon-btn h-8 w-8 hover:text-danger" title={v.used_by.length ? `In use by ${v.used_by.join(", ")}` : "Remove this volume and its data…"} aria-label="Remove {volumeLabel(v.name)}" disabled={v.used_by.length > 0 || !!containers.busy[`volume:${v.name}`]} onclick={() => containers.removeResource(sourceKey, "volume", v.name, volumeLabel(v.name))}><Trash2 size={15} /></button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else}
    {#if networks.length === 0}
      <EmptyState compact={query} icon={Container} text={query ? "No network matches the search." : `No networks on ${src?.label}.`} />
    {:else}
      <table class="w-full border-separate border-spacing-0 text-sm">
        <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
          <tr><th class="{th} pl-4">Name</th><th class={th}>Driver</th><th class={th}>Scope</th><th class={th}>Used by</th><th class="{th} text-right">Created</th><th class="{th} pr-4 text-right"><span class="sr-only">Actions</span></th></tr>
        </thead>
        <tbody>
          {#each networks as n (n.id + n.name)}
            <tr class="hover:bg-panel-hover">
              <td class="{td} max-w-[20rem] pl-4">
                <div class="flex items-center gap-1.5">
                  <span class="truncate font-mono text-[13px]" title={n.id}>{n.name}</span>
                  {#if n.predefined}<Badge tone="neutral" title="Built in; it can't be removed">built-in</Badge>{/if}
                  {#if n.internal}<Badge tone="warning" title="No outside access">internal</Badge>{/if}
                  {#if n.ipv6}<Badge tone="neutral">IPv6</Badge>{/if}
                </div>
                {#if n.labels["com.docker.compose.project"]}<Badge tone="accent" title="Compose project">{n.labels["com.docker.compose.project"]}</Badge>{/if}
              </td>
              <td class="{td} text-xs">{n.driver}</td>
              <td class="{td} text-xs">{n.scope}</td>
              <td class="{td} max-w-[16rem] truncate text-xs {n.used_by.length ? '' : 'text-fg-muted'}" title={n.used_by.join("\n")}>{usedByText(n.used_by)}</td>
              <td class="{td} text-right text-xs text-fg-muted">{formatAge(n.created, now)}</td>
              <td class="{td} pr-3 text-right">
                <button class="icon-btn h-8 w-8" title="Inspect (JSON): subnet, gateway, connected containers" aria-label="Inspect network {n.name}" onclick={() => inspect("network", n.name, n.name)}><FileJson size={15} /></button>
                <button class="icon-btn h-8 w-8 hover:text-danger" title={n.predefined ? "Built in; it can't be removed" : n.used_by.length ? `In use by ${n.used_by.join(", ")}` : "Remove this network…"} aria-label="Remove network {n.name}" disabled={n.predefined || n.used_by.length > 0 || !!containers.busy[`network:${n.name}`]} onclick={() => containers.removeResource(sourceKey, "network", n.name, n.name)}><Trash2 size={15} /></button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
</div>

{#if inspecting}
  {@const target = inspecting}
  <Modal title="Inspect · {target.title}" onclose={() => (inspecting = null)} width="max-w-4xl">
    <div class="h-[65vh]">
      {#key target.kind + target.id}
        <ContainerInspectPanel load={() => (src ? api.containers.inspectResource(src.sessionId, src.runtime, target.kind, target.id) : Promise.reject(new Error("This host was closed.")))} />
      {/key}
    </div>
  </Modal>
{/if}
