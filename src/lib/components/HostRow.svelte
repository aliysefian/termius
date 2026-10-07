<script lang="ts">
  import { Copy, Info, Pencil, Star, TerminalSquare, Trash2 } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { settings } from "$lib/stores/settings.svelte";
  import { sshCommand, timeAgo } from "$lib/sshcmd";
  import { envInfo, errorMessage } from "$lib/types";
  import { ask } from "$lib/dialogs.svelte";
  import { hostDragStart } from "$lib/hostdrag.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Host, VaultRecord } from "$lib/types";

  let { host, depth, treeKey, focused }: { host: VaultRecord<Host>; depth: number; treeKey: string; focused: boolean } = $props();
  const d = $derived(host.data!);
  const identityId = $derived(vaultStore.effectiveIdentity(d));
  const identity = $derived(identityId ? vaultStore.identityById.get(identityId)?.data : undefined);
  const jump = $derived(d.jump_host_id ? vaultStore.hostById.get(d.jump_host_id)?.data : undefined);
  const env = $derived(envInfo(d.environment));
  const health = $derived(vaultStore.health[host.id]);
  const indent = $derived(`${depth * 12 + 8}px`);
  const usage = $derived(settings.usage[host.id]);
  const usageText = $derived(usage ? `Last connected ${timeAgo(usage.last)} · ${usage.count} time${usage.count === 1 ? "" : "s"}` : "Never connected from this computer");

  async function copyCommand(e: MouseEvent) {
    e.stopPropagation();
    const cmd = sshCommand({
      host: d,
      hostId: host.id,
      hostById: vaultStore.hostById,
      identityById: vaultStore.identityById,
      identityFor: (h) => vaultStore.effectiveIdentity(h),
      jumpFor: (h, id) => vaultStore.effectiveJump(h, id),
      proxyFor: (h) => vaultStore.proxyById.get(vaultStore.effectiveProxy(h) ?? "")?.data?.spec,
    });
    try {
      await writeText(cmd);
      ui.notify("info", `Copied: ${cmd}`);
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }

  function connect() {
    ui.openTerminal(host.id, d.label);
  }

  const selected = $derived(ui.selectedHosts.has(host.id));

  /** Ctrl/Shift+click ticks hosts for bulk edit; a plain click clears. */
  function click(e: MouseEvent) {
    if (e.ctrlKey || e.metaKey || e.shiftKey) {
      e.preventDefault();
      ui.toggleHostSelected(host.id);
    } else if (ui.selectedHosts.size) {
      ui.selectedHosts = new Set();
    }
  }

  async function duplicate(e: MouseEvent) {
    e.stopPropagation();
    if (!(await ask(`Duplicate "${d.label}"? A copy is added to the list and opened for editing.`, { title: "Duplicate host", confirm: "Duplicate" }))) return;
    try {
      const rec = await vaultStore.saveHost(null, { ...$state.snapshot(d), label: `${d.label} (copy)`, favorite: false });
      ui.modal = { kind: "host", id: rec.id };
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }

  function remove(e: MouseEvent) {
    e.stopPropagation();
    void vaultStore.requestDeleteHost(host.id);
  }
</script>

<!-- Enter/Space/F2/Delete/arrows are handled by the tree container's keydown
     (HostTree.svelte), which sees this row's keydowns bubble up to it. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="host-row group relative flex cursor-pointer items-center gap-2.5 rounded-md py-1.5 pr-1 {selected ? 'bg-accent/15 ring-1 ring-inset ring-accent/40' : 'hover:bg-panel-hover'}"
  aria-selected={selected}
  aria-level={depth + 1}
  data-tree-key={treeKey}
  onclick={click}
  style:padding-left={indent}
  role="treeitem"
  tabindex={focused ? 0 : -1}
  draggable="true"
  ondragstart={(e) => hostDragStart(e, host.id)}
  ondblclick={connect}
  title="Double-click to connect, Space for details, Ctrl+click to select. {usageText}"
>
  <span class="ml-4 h-2 w-2 shrink-0 rounded-full {d.color ? '' : 'bg-accent'}" style:background={d.color || undefined}></span>
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-1.5">
      <span class="truncate text-sm">{d.label}</span>
      {#if env.value}
        <Badge tone={env.tone}>{env.short ?? ""}</Badge>
      {/if}
      {#if health}
        <span
          class="ml-auto shrink-0 font-mono text-[11px] {health.state === 'up' ? 'text-success' : health.state === 'down' ? 'text-danger' : 'text-fg-muted'}"
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
  {#if d.favorite}
    <!-- Always visible, so favorites stand out even at narrow widths. -->
    <Star size={12} class="shrink-0 text-warning group-hover:invisible group-focus-within:invisible" fill="currentColor" aria-label="Favorite" />
  {/if}
  <!-- Actions float over the row on hover instead of reserving width, so the
       label keeps the full row when the panel is narrow. -->
  <div
    class="host-actions absolute inset-y-0 right-1 my-auto hidden h-7 items-center rounded-md pl-1
      {selected ? 'bg-panel' : 'bg-panel-hover'} group-hover:flex group-focus-within:flex"
  >
    <button
      class="icon-btn h-6 w-6 {d.favorite ? 'text-warning' : ''}"
      title={d.favorite ? "Remove from favorites" : "Add to favorites"}
      aria-pressed={!!d.favorite}
      onclick={(e) => {
        e.stopPropagation();
        vaultStore.toggleFavorite(host.id).catch((err) => ui.notify("error", errorMessage(err)));
      }}
    >
      <Star size={12} fill={d.favorite ? "currentColor" : "none"} />
    </button>
    <button class="icon-btn h-6 w-6 @max-[16rem]:hidden" title="Details" onclick={(e) => { e.stopPropagation(); ui.modal = { kind: "host-details", id: host.id }; }}>
      <Info size={12} />
    </button>
    <button class="icon-btn h-6 w-6 @max-[18rem]:hidden" title="Copy as ssh command" onclick={copyCommand}>
      <TerminalSquare size={12} />
    </button>
    <button class="icon-btn h-6 w-6 @max-[18rem]:hidden" title="Duplicate" onclick={duplicate}>
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
