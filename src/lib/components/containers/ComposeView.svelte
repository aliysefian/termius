<script lang="ts">
  import { Copy, FolderTree, Play, RotateCw, Square, Trash2 } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Badge from "../Badge.svelte";
  import { groupSummaries, projectState } from "$lib/containerview";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type ContainerInfo } from "$lib/types";

  let { sourceKey, list, isDocker, onshow }: { sourceKey: string; list: ContainerInfo[]; isDocker: boolean; /** Show this project's containers. */ onshow: (project: string) => void } = $props();

  const projects = $derived(groupSummaries(list).filter((g) => g.project));

  async function copy(text: string, what: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied the ${what}.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
  const services = (cs: ContainerInfo[]) => [...new Set(cs.map((c) => c.labels["com.docker.compose.service"] ?? c.labels["io.podman.compose.service"] ?? c.name))];
</script>

<div class="min-h-0 flex-1 overflow-auto">
  <table class="w-full border-separate border-spacing-0 text-sm">
    <thead class="sticky top-0 z-10 bg-panel text-left text-xs font-medium text-fg-muted">
      <tr>
        <th class="border-b border-line py-2 pl-4 pr-3">Project</th>
        <th class="border-b border-line px-3 py-2">Services</th>
        <th class="border-b border-line px-3 py-2">State</th>
        <th class="border-b border-line px-3 py-2">Directory and Compose file</th>
        <th class="border-b border-line py-2 pl-2 pr-4 text-right"><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      {#each projects as g (g.project)}
        {@const pbusy = !!containers.busy[`project:${g.project}`]}
        {@const st = projectState(g)}
        <tr class="hover:bg-panel-hover">
          <td class="border-b border-line/60 py-3 pl-4 pr-3">
            <button class="flex items-center gap-2 text-left font-medium hover:text-accent" title="Show the containers of {g.project}" onclick={() => onshow(g.project!)}><FolderTree size={14} class="text-accent" /> {g.project}</button>
            <Badge tone="accent">compose</Badge>
          </td>
          <td class="max-w-[16rem] truncate border-b border-line/60 px-3 py-3 text-xs text-fg-muted" title={services(g.containers).join(", ")}>{services(g.containers).join(", ")}</td>
          <td class="border-b border-line/60 px-3 py-3 text-xs">
            <span class={st === "running" ? "text-success" : st === "partial" ? "text-warning" : "text-fg-muted"}>{g.running} of {g.containers.length} running</span>
            {#if g.unhealthy}<span class="ml-2 text-danger">{g.unhealthy} unhealthy</span>{/if}
          </td>
          <td class="max-w-[22rem] border-b border-line/60 px-3 py-3 font-mono text-[11px] text-fg-muted">
            {#if g.workingDir}<div class="flex items-center gap-1"><span class="truncate" title={g.workingDir}>{g.workingDir}</span><button class="shrink-0 hover:text-fg" aria-label="Copy the directory of {g.project}" onclick={() => void copy(g.workingDir!, "directory")}><Copy size={10} /></button></div>{/if}
            {#if g.configFiles}<div class="flex items-center gap-1"><span class="truncate" title={g.configFiles}>{g.configFiles}</span><button class="shrink-0 hover:text-fg" aria-label="Copy the Compose file of {g.project}" onclick={() => void copy(g.configFiles!, "Compose file path")}><Copy size={10} /></button></div>{/if}
            {#if !g.workingDir && !g.configFiles}–{/if}
          </td>
          <td class="border-b border-line/60 py-3 pl-2 pr-3 text-right">
            <div class="inline-flex items-center gap-0.5">
              <button class="icon-btn h-8 w-8" title={!isDocker ? "Compose actions need Docker" : "Start every container"} aria-label="Start {g.project}" disabled={!isDocker || pbusy || g.running === g.containers.length} onclick={() => containers.compose(sourceKey, g.project!, "start", g.containers.length)}><Play size={14} /></button>
              <button class="icon-btn h-8 w-8" title={!isDocker ? "Compose actions need Docker" : "Stop every container"} aria-label="Stop {g.project}" disabled={!isDocker || pbusy || g.running === 0} onclick={() => containers.compose(sourceKey, g.project!, "stop", g.containers.length)}><Square size={14} /></button>
              <button class="icon-btn h-8 w-8" title={!isDocker ? "Compose actions need Docker" : "Restart every container"} aria-label="Restart {g.project}" disabled={!isDocker || pbusy} onclick={() => containers.compose(sourceKey, g.project!, "restart", g.containers.length)}><RotateCw size={14} class={pbusy ? "animate-spin" : ""} /></button>
              <button class="icon-btn h-8 w-8 hover:text-danger" title={!isDocker ? "Compose actions need Docker" : "Take it down: remove its containers and networks, keep its volumes"} aria-label="Take down {g.project}" disabled={!isDocker || pbusy} onclick={() => containers.compose(sourceKey, g.project!, "down", g.containers.length)}><Trash2 size={14} /></button>
            </div>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>
