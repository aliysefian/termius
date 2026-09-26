<script lang="ts">
  import { Copy, Pencil, Trash2 } from "lucide-svelte";
  import { errorMessage } from "$lib/types";
  import { hostDragStart } from "$lib/hostdrag.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Host, VaultRecord } from "$lib/types";

  let { host, depth }: { host: VaultRecord<Host>; depth: number } = $props();
  const d = $derived(host.data!);
  const identity = $derived(d.identity_id ? vaultStore.identityById.get(d.identity_id)?.data : undefined);
  const jump = $derived(d.jump_host_id ? vaultStore.hostById.get(d.jump_host_id)?.data : undefined);
  const indent = $derived(`${depth * 12 + 8}px`);

  function connect() {
    ui.openTerminal(host.id, d.label);
  }

  async function duplicate(e: MouseEvent) {
    e.stopPropagation();
    try {
      const rec = await vaultStore.saveHost(null, { ...$state.snapshot(d), label: `${d.label} (copy)` });
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
    await vaultStore.deleteHost(host.id);
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
    <div class="truncate text-sm">{d.label}</div>
    <div class="truncate text-xs text-fg-muted">
      {identity ? `${identity.username}@` : ""}{d.hostname}{d.port !== 22 ? `:${d.port}` : ""}{jump ? ` via ${jump.label}` : ""}
    </div>
  </div>
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
