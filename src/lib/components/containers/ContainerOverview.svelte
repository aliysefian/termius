<script lang="ts">
  import { onMount } from "svelte";
  import { Copy, ExternalLink, Loader2, Unplug } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import * as api from "$lib/api";
  import { composeProject, formatAge, portLabels } from "$lib/containerdata";
  import { portTargets, summarizeInspect, type InspectSummary } from "$lib/containerview";
  import { containers } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type ContainerInfo } from "$lib/types";

  let { sourceKey, c }: { sourceKey: string; c: ContainerInfo } = $props();

  const src = $derived(containers.sources[sourceKey]);
  let sum = $state<InspectSummary | null>(null);
  let loading = $state(true);
  let note = $state<string | null>(null);

  // The list already says a lot; `inspect` adds what it doesn't (restart policy, entrypoint, addresses, mounts).
  onMount(async () => {
    const s = containers.sources[sourceKey];
    if (!s) return;
    try {
      sum = summarizeInspect(JSON.parse(await api.containers.inspect(s.sessionId, s.runtime, c.id)));
    } catch (e) {
      note = errorMessage(e);
    } finally {
      loading = false;
    }
  });

  const targets = $derived(portTargets(c.ports));
  const labels = $derived(portLabels(c.ports));
  const project = $derived(composeProject(c));
  const service = $derived(c.labels["com.docker.compose.service"] ?? c.labels["io.podman.compose.service"] ?? null);
  const when = (iso: string | null) => (iso ? new Date(iso).toLocaleString() : null);
  const remote = $derived(!!src?.hostId);

  async function copy(text: string) {
    try {
      await writeText(text);
      ui.notify("info", "Copied.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

</script>

{#snippet kv(k: string, v: string | null, mono = true, copyable = false)}
  <dt class="text-fg-muted">{k}</dt>
  <dd class="min-w-0 break-words text-xs leading-5 {mono ? 'font-mono' : ''}">
    {#if v}{v}{#if copyable}<button class="ml-1 inline-flex align-middle text-fg-muted hover:text-fg" title="Copy" aria-label="Copy {k}" onclick={() => void copy(v)}><Copy size={11} /></button>{/if}{:else}<span class="text-fg-muted">–</span>{/if}
  </dd>
{/snippet}

<div class="space-y-5">
  {#if note}<p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning" role="status">Some details couldn't be read: {note}</p>{/if}

  <section aria-labelledby="co-id">
    <h3 id="co-id" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Identity</h3>
    <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-sm">
      {@render kv("Name", c.name, false)}
      {@render kv("Container ID", c.id, true, true)}
      {@render kv("Image", c.image, true, true)}
      {@render kv("Status", c.status, false)}
      {@render kv("Created", when(sum?.created ?? null) ?? (c.created ? new Date(c.created * 1000).toLocaleString() : null), false)}
      {@render kv("Age", c.created ? formatAge(c.created) : null, false)}
      {#if sum}
        {@render kv("Started", when(sum.startedAt), false)}
        {#if sum.exitCode !== null}{@render kv("Exit code", String(sum.exitCode))}{/if}
        {#if sum.error}{@render kv("Error", sum.error, false)}{/if}
        {#if sum.healthStatus}{@render kv("Health", sum.healthStatus, false)}{/if}
      {/if}
    </dl>
  </section>

  <section aria-labelledby="co-net">
    <h3 id="co-net" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Networking</h3>
    {#if targets.length}
      <ul class="mb-2 space-y-1">
        {#each targets as t (t.hostPort)}
          {@const tunnel = containers.portTunnels[`${sourceKey}:${t.hostPort}`]}
          <li class="flex flex-wrap items-center gap-2 rounded-md border border-line bg-base/50 px-2.5 py-1.5 text-xs">
            <span class="font-mono tabular-nums">{t.address}:{t.hostPort} → {t.containerPort}</span>
            {#if tunnel?.port}<span class="text-success" title="Reached on this computer through a tunnel">via tunnel :{tunnel.port}</span>{/if}
            <span class="flex-1"></span>
            {#if c.state === "running"}
              <button class="btn-secondary h-7 text-xs" title={t.web ? (remote ? "Opens a tunnel to this host's port, then the browser" : "Open in the browser") : "Not usually a web port, but it can be opened as one"} onclick={() => void containers.openPort(sourceKey, t)}>
                {#if tunnel && !tunnel.port}<Loader2 size={12} class="animate-spin" />{:else}<ExternalLink size={12} />{/if}
                {remote ? "Open via tunnel" : "Open"}
              </button>
              {#if tunnel}<button class="icon-btn h-7 w-7" title="Close the tunnel" aria-label="Close the tunnel to port {t.hostPort}" onclick={() => void containers.closePortTunnel(sourceKey, t.hostPort)}><Unplug size={13} /></button>{/if}
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-sm">
      {@render kv("Published", labels.filter((p) => p.published).map((p) => p.text).join("\n") || null)}
      {@render kv("Exposed only", labels.filter((p) => !p.published).map((p) => p.text).join(", ") || null)}
      {#if sum}
        {#each sum.networks as n (n.name)}
          {@render kv(`Network ${n.name}`, [n.ip, n.gateway && `gateway ${n.gateway}`, n.aliases.length ? `aliases ${n.aliases.join(", ")}` : null].filter(Boolean).join(" · ") || null)}
        {:else}
          {@render kv("Networks", c.networks.join(", ") || null)}
        {/each}
      {:else}
        {@render kv("Networks", c.networks.join(", ") || null)}
      {/if}
    </dl>
    {#if remote && c.state === "running" && targets.length}
      <p class="mt-2 text-[11px] text-fg-muted">These ports are on {src?.label}, not on this computer. "Open via tunnel" forwards one through SSHVault's own tunnels and stops it when this host is closed.</p>
    {/if}
  </section>

  <section aria-labelledby="co-run">
    <h3 id="co-run" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Runtime</h3>
    {#if loading}
      <p class="flex items-center gap-2 text-xs text-fg-muted"><Loader2 size={13} class="animate-spin" /> Reading the container's configuration…</p>
    {:else}
      <dl class="grid grid-cols-[7rem_1fr] gap-x-3 gap-y-1.5 text-sm">
        {@render kv("Restart policy", sum?.restartPolicy ?? null, false)}
        {@render kv("Entrypoint", sum?.entrypoint ?? null)}
        {@render kv("Command", sum?.command ?? (c.command || null))}
        {@render kv("Working dir", sum?.workingDir ?? null)}
        {@render kv("User", sum?.user ?? null)}
        {@render kv("Environment", sum ? `${sum.envCount} variable${sum.envCount === 1 ? "" : "s"} (see Inspect)` : null, false)}
        {#if project}{@render kv("Compose project", project, false)}{/if}
        {#if service}{@render kv("Compose service", service, false)}{/if}
        {#if c.pod}{@render kv("Pod", c.pod, false)}{/if}
      </dl>
    {/if}
  </section>

  <section aria-labelledby="co-store">
    <h3 id="co-store" class="mb-2 text-xs font-medium uppercase tracking-wide text-fg-muted">Storage</h3>
    {#if sum && sum.mounts.length}
      <ul class="space-y-1.5">
        {#each sum.mounts as m, i (i)}
          <li class="rounded-md border border-line bg-base/50 px-2.5 py-1.5 text-xs">
            <div class="flex items-center gap-2"><span class="rounded bg-fg-muted/10 px-1.5 py-px text-[10px] uppercase text-fg-muted">{m.type || "mount"}</span><span class="min-w-0 flex-1 truncate font-mono" title={m.destination}>{m.destination}</span><span class="text-fg-muted">{m.rw ? "read-write" : "read-only"}</span></div>
            <div class="mt-0.5 truncate font-mono text-[11px] text-fg-muted" title={m.source}>{m.name ?? m.source}</div>
          </li>
        {/each}
      </ul>
    {:else if !sum && c.mounts.length}
      <ul class="space-y-1">{#each c.mounts as m (m)}<li class="truncate font-mono text-xs" title={m}>{m}</li>{/each}</ul>
    {:else}
      <p class="text-xs text-fg-muted">{loading ? "Reading…" : "No mounts."}</p>
    {/if}
  </section>
</div>
