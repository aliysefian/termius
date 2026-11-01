<script lang="ts">
  // A shell inside the container, in the drawer. It is a terminal session of its own (to the host, then `exec` into
  // the container, bash if the container has it and sh if not). Closing it ends that session and nothing else: the
  // container keeps running.
  import { onDestroy } from "svelte";
  import { SquareTerminal, X } from "lucide-svelte";
  import TerminalPane from "../TerminalPane.svelte";
  import { shellCommand } from "$lib/containerdata";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui, type Pane } from "$lib/stores/ui.svelte";
  import type { ContainerInfo } from "$lib/types";

  let { sourceKey, c, visible }: { sourceKey: string; c: ContainerInfo; /** Whether the Terminal tab is showing. */ visible: boolean } = $props();

  const src = $derived(containers.sources[sourceKey]);
  let pane = $state<Pane | null>(null);
  const status = $derived(pane ? ui.paneInfo[pane.id]?.status : undefined);

  function start() {
    const s = containers.sources[sourceKey];
    const line = s && shellCommand(s.runtime, c.id);
    if (!s || !line) {
      ui.notify("error", "That container's name or ID can't be used in a command.");
      return;
    }
    const id = `ctn-${crypto.randomUUID()}`;
    pane = { id, target: s.hostId ? { kind: "host", hostId: s.hostId, command: line } : { kind: "local", command: line } };
  }

  function close() {
    if (pane) delete ui.paneInfo[pane.id];
    pane = null;
  }

  onDestroy(() => {
    if (pane) delete ui.paneInfo[pane.id];
  });
</script>

{#if !pane}
  <div class="rounded-[10px] border border-line bg-base/50 p-5 text-center">
    <SquareTerminal size={22} class="mx-auto text-accent" aria-hidden="true" />
    {#if c.state !== "running"}
      <p class="mt-2 text-sm font-medium">The container isn't running</p>
      <p class="mx-auto mt-1 max-w-xs text-xs text-fg-muted">A shell needs a running container. Start it first.</p>
    {:else}
      <p class="mt-2 text-sm font-medium">Open a shell in {c.name}</p>
      <p class="mx-auto mt-1 max-w-xs text-xs text-fg-muted">It uses bash if the container has it and sh if not. Closing the terminal doesn't stop the container.</p>
      <button class="btn-primary mt-3" onclick={start}>Start a shell</button>
    {/if}
  </div>
{:else}
  <div class="flex h-full min-h-[18rem] flex-col gap-2">
    <div class="flex items-center gap-2 text-xs text-fg-muted">
      <span class="h-2 w-2 rounded-full {status === 'connected' ? 'bg-success' : status === 'error' ? 'bg-danger' : status === 'disconnected' ? 'bg-fg-muted/50' : 'animate-pulse bg-warning'}"></span>
      <span class="min-w-0 flex-1 truncate">{status === "connected" ? `Shell in ${c.name} on ${src?.label}` : status === "error" ? "The terminal couldn't connect" : status === "disconnected" ? "The shell ended. The container is still running." : "Connecting…"}</span>
      <button class="btn-secondary h-7 text-xs" onclick={close}><X size={12} /> Close terminal</button>
    </div>
    <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-[10px] border border-line bg-base">
      {#key pane.id}<TerminalPane {pane} active={visible} />{/key}
    </div>
  </div>
{/if}
