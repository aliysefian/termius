<script lang="ts">
  import { Copy, Pencil, Star, Trash2 } from "lucide-svelte";
  import { envInfo, errorMessage } from "$lib/types";
  import { hostDragStart } from "$lib/hostdrag.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Host, VaultRecord } from "$lib/types";

  let { host, depth }: { host: VaultRecord<Host>; depth: number } = $props();
  const d = $derived(host.data!);
  const identityId = $derived(vaultStore.effectiveIdentity(d));
  const identity = $derived(identityId ? vaultStore.identityById.get(identityId)?.data : undefined);
  const jump = $derived(d.jump_host_id ? vaultStore.hostById.get(d.jump_host_id)?.data : undefined);
  const env = $derived(envInfo(d.environment));
  const health = $derived(vaultStore.health[host.id]);
  const indent = $derived(`${depth * 12 + 8}px`);

  function connect() {
    ui.openTerminal(host.id, d.label);
  }

  async function duplicate(e: MouseEvent) {
    e.stopPropagation();
    try {
      const rec = await vaultStore.saveHost(null, { ...$state.snapshot(d), label: `${d.label} (copy)`, favorite: false });
      ui.modal = { kind: "host", id: rec.id };
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }

  async function remove(e: MouseEvent) {
    e.stopPropagation();
    const dependents = vaultStore.hosts.filter((h) => h.data?.jump_host_id === host.id).length;
    const note = dependents ? ` ${dependents} host(s) use it as a jump host and will connect directly instead.` : "";
    if (!confirm(`Delete host "${d.label}"?${note}`)) return;
    await vaultStore.deleteHost(host.id).catch((err) => ui.notify("error", errorMessage(err)));
  }
</script>

<div
  class="group flex cursor-pointer items-center gap-2.5 rounded-md py-1.5 pr-1 hover:bg-panel-hover"
  style:padding-left={indent}
  role="button"
  tabindex="0"
  draggable="true"
  ondragstart={(e) => hostDragStart(e, host.id)}
  ondblclick={connect}
  onkeydown={(e) => e.key === "Enter" && connect()}
  title="Double-click to connect"
>
  <span class="ml-4 h-2 w-2 shrink-0 rounded-full" style:background={d.color ?? "#7B61FF"}></span>
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-1.5">
      <span class="truncate text-sm">{d.label}</span>
      {#if env.value}
        <span class="shrink-0 rounded px-1 text-[9px] font-bold {env.cls ?? ''}">{env.short ?? ""}</span>
      {/if}
      {#if health}
        <span
          class="ml-auto shrink-0 font-mono text-[10px] {health.state === 'up' ? 'text-success' : health.state === 'down' ? 'text-danger' : 'text-fg-muted'}"
          title={health.state === "up"
            ? `Reachable in ${health.latency_ms} ms${health.banner ? `\n${health.banner}` : ""}`
            : health.state === "down"
              ? `Unreachable: ${health.reason}`
              : "Behind a jump host, not probed"}
        >
          {health.state === "up" ? `${health.latency_ms}ms` : health.state === "down" ? "down" : "jump"}
        </span>
      {/if}
    </div>
    <div class="truncate text-xs text-fg-muted">
      {identity ? `${identity.username}@` : ""}{d.hostname}{d.port !== 22 ? `:${d.port}` : ""}{jump ? ` via ${jump.label}` : ""}
    </div>
  </div>
  <button
    class="icon-btn h-6 w-6 {d.favorite ? 'text-warning' : 'opacity-0 group-hover:opacity-100'}"
    title={d.favorite ? "Remove from favorites" : "Add to favorites"}
    aria-pressed={!!d.favorite}
    onclick={(e) => {
      e.stopPropagation();
      vaultStore.toggleFavorite(host.id).catch((err) => ui.notify("error", errorMessage(err)));
    }}
  >
    <Star size={12} fill={d.favorite ? "currentColor" : "none"} />
  </button>
  <div class="flex opacity-0 group-hover:opacity-100">
    <button class="icon-btn h-6 w-6" title="Duplicate" onclick={duplicate}>
      <Copy size={12} />
    </button>
    <button class="icon-btn h-6 w-6" title="Edit" onclick={(e) => { e.stopPropagation(); ui.modal = { kind: "host", id: host.id }; }}>
      <Pencil size={12} />
    </button>
    <button class="icon-btn h-6 w-6 hover:text-danger" title="Delete" onclick={remove}>
      <Trash2 size={12} />
    </button>
  </div>
</div>
