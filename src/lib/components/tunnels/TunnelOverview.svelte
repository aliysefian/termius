<script lang="ts">
  import { ArrowDown, Copy, Globe, SquareTerminal } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import HopChain from "../HopChain.svelte";
  import { openTunnelTerminal } from "./actions";
  import { ui } from "$lib/stores/ui.svelte";
  import { KIND_TEXT, endpoints, formatUptime, mapping, type TunnelRow } from "$lib/tunnels";
  import { errorMessage } from "$lib/types";

  let { row, now }: { row: TunnelRow; now: number } = $props();

  const rule = $derived(row.rule);
  const ends = $derived(endpoints(rule, row.hostLabel, row.boundPort));
  const m = $derived(mapping(rule, row.boundPort));
  const running = $derived(row.status === "active");

  /** A local tunnel in front of a web service can be opened in the browser. */
  const url = $derived.by(() => {
    if (rule.kind !== "local" || !running || !row.boundPort) return null;
    const host = rule.bind_addr === "0.0.0.0" || rule.bind_addr === "::" ? "127.0.0.1" : rule.bind_addr;
    return `${row.boundPort === 443 || rule.dest_port === 443 ? "https" : "http"}://${host.includes(":") ? `[${host}]` : host}:${row.boundPort}`;
  });

  async function copy(text: string) {
    try {
      await writeText(text);
      ui.notify("info", `Copied ${text}`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  // What is known, said as exactly as it is known. An established SSH connection and a bound port don't prove that
  // the service behind them answers, so nothing here says so.
  const headline = $derived(
    {
      active: "SSH forwarding is established.",
      connecting: "Connecting to the host and setting up the forwarding…",
      stopping: "Stopping…",
      failed: "The tunnel stopped working.",
      stopped: "The tunnel is not running.",
    }[row.status],
  );
  const info = $derived<[string, string][]>([
    ["Type", `${KIND_TEXT[rule.kind]} forwarding`],
    [rule.kind === "remote" ? "Remote bind address" : "Local bind address", rule.bind_addr || "all interfaces"],
    [rule.kind === "remote" ? "Remote port" : "Local port", m.from.split(":").pop() ?? ""],
    ...(rule.kind === "dynamic" ? [] : ([[rule.kind === "remote" ? "Local destination" : "Destination host", rule.dest_host], ["Destination port", String(rule.dest_port)]] as [string, string][])),
    ["SSH host", row.hostLabel],
  ]);
  const health = $derived<[string, string, "ok" | "bad" | "none"][]>([
    ["SSH connection", running || row.status === "stopping" ? "Established" : row.status === "connecting" ? "Connecting" : row.status === "failed" ? "Lost or refused" : "Not running", running ? "ok" : row.status === "failed" ? "bad" : "none"],
    [rule.kind === "remote" ? "Remote forward" : "Listener", running ? (rule.kind === "remote" ? `Accepted by the server on ${m.from}` : `Listening on ${m.from}`) : row.status === "failed" ? "Not established" : "Not running", running ? "ok" : "none"],
    ["Active for", running && row.activeSince ? formatUptime(now - row.activeSince) : "–", "none"],
    ["Last error", row.error ?? "None", row.error ? "bad" : "none"],
    ["Reconnecting", "Not automatic: a tunnel that fails is started again by you", "none"],
    ["Service behind it", "Not checked", "none"],
  ]);
</script>

<div class="space-y-5">
  <!-- Status -->
  <section aria-labelledby="t-status">
    <h3 id="t-status" class="sr-only">Status</h3>
    <div class="flex items-start gap-3 rounded-[10px] border border-line bg-base/50 p-3.5">
      <span class="mt-1 h-2.5 w-2.5 shrink-0 rounded-full {row.status === 'active' ? 'bg-success' : row.status === 'failed' ? 'bg-danger' : row.status === 'stopped' ? 'bg-fg-muted/50' : 'animate-pulse bg-warning motion-reduce:animate-none'}" aria-hidden="true"></span>
      <div class="min-w-0">
        <p class="text-sm font-medium">{headline}</p>
        {#if row.error}<p class="mt-1 break-words text-xs text-danger">{row.error}</p>{/if}
        {#if !row.host}<p class="mt-1 text-xs text-danger">Its SSH host no longer exists. Edit the tunnel to choose another.</p>{/if}
      </div>
    </div>
  </section>

  <!-- The picture -->
  <section aria-labelledby="t-map">
    <h3 id="t-map" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Port mapping</h3>
    <div class="flex flex-col items-stretch gap-1.5 text-center">
      <div class="rounded-[10px] border border-line bg-base/50 px-3 py-2.5">
        <div class="text-[11px] uppercase tracking-wide text-fg-muted">{ends.start.label}</div>
        <div class="mt-0.5 break-all font-mono text-sm tabular-nums">{ends.start.value}</div>
      </div>
      <div class="flex flex-col items-center text-xs text-fg-muted">
        <ArrowDown size={14} aria-hidden="true" />
        <span class="my-0.5 rounded-full border border-accent/30 bg-accent/10 px-2.5 py-0.5 text-accent">{ends.middle}</span>
        <ArrowDown size={14} aria-hidden="true" />
      </div>
      <div class="rounded-[10px] border border-line bg-base/50 px-3 py-2.5">
        <div class="text-[11px] uppercase tracking-wide text-fg-muted">{ends.end.label}</div>
        <div class="mt-0.5 break-all font-mono text-sm tabular-nums">{ends.end.value}</div>
      </div>
    </div>
  </section>

  <!-- Information -->
  <section aria-labelledby="t-info">
    <h3 id="t-info" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Information</h3>
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
      {#each info as [k, v] (k)}
        <dt class="text-fg-muted">{k}</dt>
        <dd class="min-w-0 break-words font-mono text-xs leading-5 tabular-nums">{v}</dd>
      {/each}
      <dt class="text-fg-muted">Starts with the vault</dt>
      <dd class="text-xs leading-5">{rule.auto_start ? "Yes" : "No"}</dd>
    </dl>
  </section>

  <!-- The route when the host is behind a bastion; live numbers come from an open terminal to it. -->
  {#if row.host}
    <section aria-labelledby="t-route" class="overflow-hidden">
      <h3 id="t-route" class="sr-only">Route</h3>
      <HopChain hostId={rule.host_id} polling={false} />
    </section>
  {/if}

  <!-- Health: only what the backend knows -->
  <section aria-labelledby="t-health">
    <h3 id="t-health" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Connection health</h3>
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
      {#each health as [k, v, tone] (k)}
        <dt class="text-fg-muted">{k}</dt>
        <dd class="min-w-0 break-words text-xs leading-5 {tone === 'ok' ? 'text-success' : tone === 'bad' ? 'text-danger' : ''}">{v}</dd>
      {/each}
    </dl>
  </section>

  <!-- Quick actions -->
  <section aria-labelledby="t-quick" class="flex flex-wrap gap-2">
    <h3 id="t-quick" class="sr-only">Quick actions</h3>
    <button class="btn-secondary" disabled={!row.host} onclick={() => openTunnelTerminal(row)}><SquareTerminal size={14} /> Open SSH Terminal</button>
    {#if url}<button class="btn-secondary" onclick={() => openUrl(url).catch((e) => ui.notify("error", errorMessage(e)))}><Globe size={14} /> Open in browser</button>{/if}
    <button class="btn-secondary" onclick={() => void copy(rule.kind === "remote" ? m.to : m.from)}><Copy size={14} /> Copy address</button>
  </section>
</div>
