<script lang="ts">
  import { FolderSync, Pencil, Play, TerminalSquare } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { sshCommand, timeAgo } from "$lib/sshcmd";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";
  import { connectionLog } from "$lib/stores/connectionlog.svelte";
  import { hostMetrics } from "$lib/stores/hostmetrics.svelte";
  import { formatUptime } from "$lib/hostmetrics";
  import { Activity } from "lucide-svelte";

  let { id }: { id: Uuid } = $props();
  const rec = $derived(vaultStore.hostById.get(id));
  const d = $derived(rec?.data);
  const identityId = $derived(d ? vaultStore.effectiveIdentity(d) : undefined);
  const identity = $derived(identityId ? vaultStore.identityById.get(identityId)?.data : undefined);
  const jumpId = $derived(vaultStore.effectiveJump(d, id));
  const proxyId = $derived(vaultStore.effectiveProxy(d));
  const env = $derived(envInfo(d ? vaultStore.effectiveEnv(d) : ""));
  const usage = $derived(settings.usage[id]);
  const history = $derived((settings.history[id] ?? []).slice(0, 8));
  const sessions = $derived(connectionLog.forHost(id, 5));
  const monitored = $derived(hostMetrics.isMonitored(id));
  const canMonitor = $derived(hostMetrics.canMonitor(id));
  const reading = $derived(hostMetrics.readings[id]);

  function formatDuration(ms: number): string {
    const s = Math.round(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m`;
    return `${Math.floor(m / 60)}h ${m % 60}m`;
  }
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
      cur = vaultStore.effectiveJump(h, cur);
    }
    return names;
  });

  const command = $derived(
    d
      ? sshCommand({
          host: d,
          hostId: id,
          hostById: vaultStore.hostById,
          identityById: vaultStore.identityById,
          identityFor: (h) => vaultStore.effectiveIdentity(h),
          jumpFor: (h, hid) => vaultStore.effectiveJump(h, hid),
          proxyFor: (h) => vaultStore.proxyById.get(vaultStore.effectiveProxy(h) ?? "")?.data?.spec,
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
        <span class="h-2.5 w-2.5 rounded-full {d.color ? '' : 'bg-accent'}" style:background={d.color || undefined}></span>
        <code class="font-mono">{d.hostname}{d.port !== 22 ? `:${d.port}` : ""}</code>
        {#if env.value}<Badge tone={env.tone}>{env.label}</Badge>{/if}
        {#each d.tags as t (t)}<Badge tone="accent">{t}</Badge>{/each}
        {#if d.favorite}<span class="text-[11px] text-warning">★ favorite</span>{/if}
      </div>

      <dl class="grid grid-cols-[8rem_1fr] gap-x-3 gap-y-1 text-xs">
        <dt class="text-fg-muted">Group</dt><dd>{d.group || "—"}</dd>
        <dt class="text-fg-muted">Credential</dt><dd>{identity ? `${identity.label} (${identity.username})` : "asked when connecting"}{identityId && identityId !== d.identity_id ? " · from group" : ""}</dd>
        <dt class="text-fg-muted">Route</dt><dd>{route.length ? `${route.join(" → ")} → ${d.label}` : "direct"}{jumpId && jumpId !== d.jump_host_id ? " · jump from group" : ""}</dd>
        {#if proxyId}<dt class="text-fg-muted">Proxy</dt><dd>{vaultStore.proxyById.get(proxyId)?.data?.name ?? "(missing)"}{proxyId !== d.proxy_id ? " · from group" : ""}</dd>{/if}
        {#if d.keepalive_secs != null}<dt class="text-fg-muted">Keep-alive</dt><dd>{d.keepalive_secs} s</dd>{/if}
        {#if d.forward_agent || d.forward_x11}<dt class="text-fg-muted">Forwarding</dt><dd>{[d.forward_agent && "ssh-agent", d.forward_x11 && "X11"].filter(Boolean).join(", ")}</dd>{/if}
        {#if d.startup_command}<dt class="text-fg-muted">On connect</dt><dd class="font-mono">{d.startup_command}</dd>{/if}
        {#each Object.entries(d.custom ?? {}) as [k, v] (k)}<dt class="truncate text-fg-muted" title={k}>{k}</dt><dd>{v}</dd>{/each}
        <dt class="text-fg-muted">Last connected</dt><dd>{usage ? `${timeAgo(usage.last)} · ${usage.count} time${usage.count === 1 ? "" : "s"} from this computer` : "never from this computer"}</dd>
        {#if health}<dt class="text-fg-muted">Reachability</dt><dd class={health.state === "up" ? "text-success" : health.state === "down" ? "text-danger" : ""}>{health.state === "up" ? `up, ${health.latency_ms} ms${health.banner ? ` · ${health.banner}` : ""}` : health.state === "down" ? `down: ${health.reason}` : "behind a jump host"}</dd>{/if}
        <dt class="text-fg-muted">ssh</dt>
        <dd class="flex items-center gap-2"><code class="min-w-0 flex-1 truncate font-mono">{command}</code><button class="btn-ghost py-0.5 text-xs" onclick={() => copy(command, "Command")}>Copy</button></dd>
      </dl>

      <div class="rounded-md border border-line p-3">
        <div class="mb-1 flex items-center justify-between gap-2">
          <h3 class="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-fg-muted"><Activity size={11} /> Live metrics</h3>
          <div class="flex items-center gap-1">
            <button
              class="btn-ghost py-0.5 text-xs"
              disabled={!canMonitor}
              title={canMonitor ? "Network, processes, ports and interfaces, while this window is open" : "Needs saved credentials to run unattended"}
              onclick={() => (ui.modal = { kind: "host-monitor", id })}
            >
              Open detail view…
            </button>
            <button
              class="btn-ghost py-0.5 text-xs"
              disabled={!canMonitor}
              title={canMonitor ? "" : "Needs saved credentials to run unattended"}
              onclick={() => hostMetrics.setMonitored(id, !monitored)}
            >
              {monitored ? "Stop monitoring" : "Start monitoring"}
            </button>
          </div>
        </div>
        {#if !monitored}
          <p class="text-xs text-fg-muted">
            Off. Turning it on runs a small read-only script on this host every 30 seconds (CPU, memory, disk, load,
            uptime) while the vault is unlocked. Nothing is installed; nothing leaves this computer.
          </p>
        {:else if reading?.error}
          <p class="text-xs text-danger">Last poll failed: {reading.error}</p>
        {:else if reading?.metrics}
          {@const m = reading.metrics}
          <div class="grid grid-cols-5 gap-2 text-center text-xs">
            <div><div class="font-mono text-sm">{m.cpuPct != null ? `${m.cpuPct}%` : "—"}</div><div class="text-fg-muted">CPU</div></div>
            <div><div class="font-mono text-sm">{m.memPct != null ? `${m.memPct}%` : "—"}</div><div class="text-fg-muted">Memory</div></div>
            <div><div class="font-mono text-sm">{m.diskPct != null ? `${m.diskPct}%` : "—"}</div><div class="text-fg-muted">Disk (/)</div></div>
            <div><div class="truncate font-mono text-sm" title={m.load ?? ""}>{m.load ?? "—"}</div><div class="text-fg-muted">Load</div></div>
            <div><div class="font-mono text-sm">{m.uptimeSecs != null ? formatUptime(m.uptimeSecs) : "—"}</div><div class="text-fg-muted">Uptime</div></div>
          </div>
          <p class="mt-1 text-[11px] text-fg-muted">As of {timeAgo(reading.at)}.{m.cpuPct == null ? " CPU and memory couldn't be read on this host." : ""}</p>
        {:else}
          <p class="text-xs text-fg-muted">Waiting for the first reading…</p>
        {/if}
      </div>

      {#if d.notes.trim()}
        <div class="notes rounded-md border border-line bg-base p-3 text-xs">{@html renderMarkdown(d.notes)}</div>
      {/if}

      {#if sessions.length}
        <div>
          <h3 class="mb-1 text-[11px] font-medium uppercase tracking-wide text-fg-muted">Recent connections (this computer)</h3>
          <ul class="divide-y divide-line rounded-md border border-line">
            {#each sessions as s (s.id)}
              <li class="flex items-center justify-between gap-2 px-2 py-1 text-xs">
                <span>{timeAgo(s.startedAt)}</span>
                <span class="text-fg-muted">
                  {#if s.endedAt}{formatDuration(s.endedAt - s.startedAt)}{#if s.exitCode != null} · exit {s.exitCode}{:else if s.reason === "dropped"} · dropped{:else if s.reason === "failed"} · failed{/if}{:else}connected now{/if}
                </span>
              </li>
            {/each}
          </ul>
        </div>
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
