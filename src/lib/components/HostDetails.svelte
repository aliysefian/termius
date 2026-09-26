<script lang="ts">
  import { FolderSync, Pencil, Play, TerminalSquare } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { sshCommand, timeAgo } from "$lib/sshcmd";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  let { id }: { id: Uuid } = $props();
  const rec = $derived(vaultStore.hostById.get(id));
  const d = $derived(rec?.data);
  const identityId = $derived(d ? vaultStore.effectiveIdentity(d) : undefined);
  const identity = $derived(identityId ? vaultStore.identityById.get(identityId)?.data : undefined);
  const jumpId = $derived(d ? (d.jump_host_id ?? (vaultStore.groupDefault(d, "default_jump_host_id") as Uuid | undefined)) : undefined);
  const proxyId = $derived(d ? (d.proxy_id ?? (vaultStore.groupDefault(d, "proxy_id") as Uuid | undefined)) : undefined);
  const env = $derived(envInfo(d ? vaultStore.effectiveEnv(d) : ""));
  const usage = $derived(settings.usage[id]);
  const history = $derived((settings.history[id] ?? []).slice(0, 8));
  const health = $derived(vaultStore.health[id]);

  const route = $derived.by(() => {
    const names: string[] = [];
    const seen = new Set<Uuid>();
    let cur = jumpId;
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      const h = vaultStore.hostById.get(cur)?.data;
      if (!h) break;
      names.unshift(h.label);
      cur = h.jump_host_id ?? (vaultStore.groupDefault(h, "default_jump_host_id") as Uuid | undefined);
    }
    return names;
  });

  const command = $derived(
    d
      ? sshCommand({
          host: d,
          hostById: vaultStore.hostById,
          identityById: vaultStore.identityById,
          identityFor: (h) => vaultStore.effectiveIdentity(h),
          jumpFor: (h) => h.jump_host_id ?? (vaultStore.groupDefault(h, "default_jump_host_id") as Uuid | undefined),
        })
      : "",
  );

  async function copy(text: string, what: string) {
    try {
      await writeText(text);
      ui.notify("info", `${what} copied.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
</script>

{#if d}
  <Modal title={d.label} onclose={() => (ui.modal = null)} width="max-w-2xl">
    <div class="space-y-4 text-sm">
      <div class="flex flex-wrap items-center gap-2">
        <span class="h-2.5 w-2.5 rounded-full" style:background={d.color ?? "#7B61FF"}></span>
        <code class="font-mono">{d.hostname}{d.port !== 22 ? `:${d.port}` : ""}</code>
        {#if env.value}<span class="rounded px-1 text-[10px] font-bold {env.cls ?? ''}">{env.label}</span>{/if}
        {#each d.tags as t (t)}<span class="rounded bg-accent/10 px-1.5 text-[11px] text-accent">{t}</span>{/each}
        {#if d.favorite}<span class="text-[11px] text-warning">★ favorite</span>{/if}
      </div>

      <dl class="grid grid-cols-[8rem_1fr] gap-x-3 gap-y-1 text-xs">
        <dt class="text-fg-muted">Group</dt><dd>{d.group || "—"}</dd>
        <dt class="text-fg-muted">Credential</dt><dd>{identity ? `${identity.label} (${identity.username})` : "asked when connecting"}{identityId && identityId !== d.identity_id ? " · from group" : ""}</dd>
        <dt class="text-fg-muted">Route</dt><dd>{route.length ? `${route.join(" → ")} → ${d.label}` : "direct"}</dd>
        {#if proxyId}<dt class="text-fg-muted">Proxy</dt><dd>{vaultStore.proxyById.get(proxyId)?.data?.name ?? "(missing)"}</dd>{/if}
        {#if d.keepalive_secs != null}<dt class="text-fg-muted">Keep-alive</dt><dd>{d.keepalive_secs} s</dd>{/if}
        {#if d.forward_agent || d.forward_x11}<dt class="text-fg-muted">Forwarding</dt><dd>{[d.forward_agent && "ssh-agent", d.forward_x11 && "X11"].filter(Boolean).join(", ")}</dd>{/if}
        {#if d.startup_command}<dt class="text-fg-muted">On connect</dt><dd class="font-mono">{d.startup_command}</dd>{/if}
        {#each Object.entries(d.custom ?? {}) as [k, v] (k)}<dt class="truncate text-fg-muted" title={k}>{k}</dt><dd>{v}</dd>{/each}
        <dt class="text-fg-muted">Last connected</dt><dd>{usage ? `${timeAgo(usage.last)} · ${usage.count} time${usage.count === 1 ? "" : "s"} from this computer` : "never from this computer"}</dd>
        {#if health}<dt class="text-fg-muted">Reachability</dt><dd class={health.state === "up" ? "text-success" : health.state === "down" ? "text-danger" : ""}>{health.state === "up" ? `up, ${health.latency_ms} ms${health.banner ? ` · ${health.banner}` : ""}` : health.state === "down" ? `down: ${health.reason}` : "behind a jump host"}</dd>{/if}
        <dt class="text-fg-muted">ssh</dt>
        <dd class="flex items-center gap-2"><code class="min-w-0 flex-1 truncate font-mono">{command}</code><button class="btn-ghost py-0.5 text-xs" onclick={() => copy(command, "Command")}>Copy</button></dd>
      </dl>

      {#if d.notes.trim()}
        <div class="notes rounded-md border border-line bg-base p-3 text-xs">{@html renderMarkdown(d.notes)}</div>
      {/if}

      {#if history.length}
        <div>
          <h3 class="mb-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Recent commands (this computer)</h3>
          <ul class="divide-y divide-line rounded-md border border-line">
            {#each history as h (h.at)}
              <li class="flex items-center gap-2 px-2 py-1 text-xs">
                <code class="min-w-0 flex-1 truncate font-mono" title={h.command}>{h.command.split("\n")[0]}</code>
                <span class="shrink-0 text-fg-muted">{timeAgo(h.at)}{h.exit ? ` · exit ${h.exit}` : ""}</span>
                <button class="icon-btn h-6 w-6" title="Run in a new tab" onclick={() => { ui.modal = null; ui.openTerminal(id, d.label, h.command); }}><Play size={11} /></button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
    {#snippet footer()}
      <button class="btn-ghost" onclick={() => (ui.modal = { kind: "host", id })}><Pencil size={14} /> Edit</button>
      <button class="btn-ghost" onclick={() => { ui.modal = null; ui.openSftpAt(id, ""); }}><FolderSync size={14} /> SFTP</button>
      <button class="btn-primary" onclick={() => { ui.modal = null; ui.openTerminal(id, d.label); }}><TerminalSquare size={14} /> Connect</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .notes :global(p) { margin: 0 0 0.5rem; }
  .notes :global(h3), .notes :global(h4), .notes :global(h5) { font-weight: 600; margin: 0.5rem 0 0.25rem; }
  .notes :global(ul) { list-style: disc; padding-left: 1.25rem; margin: 0 0 0.5rem; }
  .notes :global(code) { font-family: var(--font-mono); background: var(--color-panel); padding: 0 0.25rem; border-radius: 0.25rem; }
  .notes :global(pre) { background: var(--color-panel); padding: 0.5rem; border-radius: 0.375rem; overflow-x: auto; margin: 0 0 0.5rem; }
  .notes :global(a) { color: var(--color-accent); text-decoration: underline; }
</style>
