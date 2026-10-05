<script lang="ts">
  import {
    ChevronDown,
    ChevronRight,
    Container,
    Download,
    FileJson,
    Loader2,
    Pause,
    Play,
    RefreshCw,
    RotateCw,
    ScrollText,
    SquareTerminal,
    Square,
    Trash2,
    X,
  } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import Combobox from "./Combobox.svelte";
  import EmptyState from "./EmptyState.svelte";
  import {
    composeProject,
    countByState,
    filterContainers,
    filterImages,
    filterNetworks,
    filterVolumes,
    formatAge,
    groupByProject,
    imageRef,
    isLive,
    portLabels,
    stateTone,
    usedByText,
    type StateFilter,
  } from "$lib/containerdata";
  import { hostOptions } from "$lib/pickeroptions";
  import { containers, LOCAL, REFRESH_CHOICES } from "$lib/stores/containers.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { CONTAINER_RUNTIMES, envInfo, type ContainerRuntime } from "$lib/types";

  type Tab = "containers" | "images" | "volumes" | "networks";
  let tab = $state<Tab>("containers");
  let query = $state("");
  let stateFilter = $state<StateFilter>("all");
  let grouped = $state(true);
  let collapsed = $state<Record<string, boolean>>({});
  let picker = $state("");
  let now = $state(Date.now());

  // Ages tick without another round trip.
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 15_000);
    return () => clearInterval(t);
  });

  // The periodic refresh. Skipped while the window is hidden or the user paused it.
  $effect(() => {
    const key = containers.activeKey;
    const secs = containers.intervalSecs;
    if (!key || containers.paused) return;
    const t = setInterval(() => {
      if (!document.hidden) void containers.refresh(key, true);
    }, secs * 1000);
    return () => clearInterval(t);
  });

  const src = $derived(containers.active);
  const isDocker = $derived(src?.runtime === "docker");
  const listing = $derived(src?.listing ?? null);
  const resources = $derived(src?.resources ?? null);
  const counts = $derived(countByState(listing?.containers ?? []));
  const shownContainers = $derived(listing ? filterContainers(listing.containers, query, stateFilter) : []);
  const shownImages = $derived(listing ? filterImages(listing.images, query) : []);
  const shownVolumes = $derived(resources ? filterVolumes(resources.volumes, query) : []);
  const shownNetworks = $derived(resources ? filterNetworks(resources.networks, query) : []);
  const hasProjects = $derived(shownContainers.some((c) => composeProject(c)));
  const groups = $derived(grouped && hasProjects ? groupByProject(shownContainers) : null);

  // Volumes and networks belong to Docker for now; fall back if the runtime changes under the tab.
  $effect(() => {
    if (!isDocker && (tab === "volumes" || tab === "networks")) tab = "containers";
  });

  // Volumes and networks cost two more commands, so they are only fetched while a tab shows them.
  $effect(() => {
    const s = src;
    if (!s) return;
    const want = s.runtime === "docker" && (tab === "volumes" || tab === "networks");
    if (s.wantResources !== want) {
      s.wantResources = want;
      if (want) void containers.refresh(s.key);
    }
  });

  const options = $derived([
    { value: LOCAL, label: "This computer", detail: "docker, podman or nerdctl installed here" },
    ...hostOptions(vaultStore.hosts),
  ]);

  async function choose(key: string) {
    if (!key) return;
    picker = "";
    await containers.open(key);
  }

  const updated = $derived(src?.updatedAt ? new Date(src.updatedAt).toLocaleTimeString() : "");
  const hostEnv = (key: string) => (key === LOCAL ? undefined : envInfo(vaultStore.effectiveEnv(vaultStore.hostById.get(key)?.data)));
  const tabs = $derived<[Tab, string][]>([
    ["containers", `Containers${listing ? ` (${listing.containers.length})` : ""}`],
    ["images", `Images${listing ? ` (${listing.images.length})` : ""}`],
    ...(isDocker
      ? ([
          ["volumes", `Volumes${resources ? ` (${resources.volumes.length})` : ""}`],
          ["networks", `Networks${resources ? ` (${resources.networks.length})` : ""}`],
        ] as [Tab, string][])
      : []),
  ]);
</script>

{#snippet containerRow(c: import("$lib/types").ContainerInfo, indent: boolean)}
  {@const live = isLive(c.state)}
  {@const busy = !!containers.busy[c.id]}
  {@const project = composeProject(c)}
  <tr class="border-b border-line/60 hover:bg-panel-hover/50">
    <td class="max-w-64 py-2 {indent ? 'pl-9 pr-4' : 'px-4'}">
      <div class="truncate font-medium text-sm" title="{c.name} · {c.id}">{c.name}</div>
      <div class="flex items-center gap-1 text-[11px] text-fg-muted">
        <span class="font-mono">{c.id.slice(0, 12)}</span>
        {#if project && !indent}<Badge tone="accent" title="Compose project">{project}</Badge>{/if}
        {#if c.labels["com.docker.compose.service"] && indent}<span>service {c.labels["com.docker.compose.service"]}</span>{/if}
        {#if c.pod}<Badge tone="neutral" title="Podman pod">pod {c.pod}</Badge>{/if}
      </div>
    </td>
    <td class="max-w-56 truncate px-2 py-2 font-mono" title={c.image}>{c.image}</td>
    <td class="px-2 py-2">
      <Badge tone={stateTone(c.state)}>{c.state}</Badge>
      <div class="mt-0.5 text-[11px] text-fg-muted">{c.status}</div>
    </td>
    <td class="max-w-56 px-2 py-2 font-mono text-[11px]">
      {#each portLabels(c.ports).slice(0, 3) as p (p.text)}<div class="truncate {p.published ? '' : 'text-fg-muted'}" title={p.published ? "Published" : "Exposed, not published"}>{p.text}</div>{/each}
      {#if portLabels(c.ports).length > 3}<div class="text-fg-muted" title={portLabels(c.ports).map((p) => p.text).join("\n")}>+{portLabels(c.ports).length - 3} more</div>{/if}
    </td>
    <td class="px-2 py-2 text-right text-fg-muted" title={c.created ? new Date(c.created * 1000).toLocaleString() : ""}>{formatAge(c.created, now)}</td>
    {#if src?.sizes}<td class="px-2 py-2 text-fg-muted">{c.size ?? "—"}</td>{/if}
    <td class="px-4 py-2">
      <div class="flex items-center justify-end gap-0.5">
        <button class="icon-btn h-7 w-7" title="Logs" aria-label="Logs of {c.name}" onclick={() => (ui.modal = { kind: "container-logs", sourceKey: src!.key, id: c.id, name: c.name })}><ScrollText size={14} /></button>
        <button class="icon-btn h-7 w-7" title={live ? "Open a shell in a terminal tab" : "Start it first to open a shell"} aria-label="Shell in {c.name}" disabled={!live} onclick={() => containers.openShell(src!.key, c)}><SquareTerminal size={14} /></button>
        <button class="icon-btn h-7 w-7" title="Inspect (JSON)" aria-label="Inspect {c.name}" onclick={() => (ui.modal = { kind: "container-inspect", sourceKey: src!.key, id: c.id, name: c.name })}><FileJson size={14} /></button>
        <span class="mx-1 h-4 w-px bg-line"></span>
        {#if live}
          <button class="icon-btn h-7 w-7" title="Stop" aria-label="Stop {c.name}" disabled={busy} onclick={() => containers.act(src!.key, c, { action: "stop" })}><Square size={14} /></button>
        {:else}
          <button class="icon-btn h-7 w-7" title="Start" aria-label="Start {c.name}" disabled={busy} onclick={() => containers.act(src!.key, c, { action: "start" })}><Play size={14} /></button>
        {/if}
        <button class="icon-btn h-7 w-7" title="Restart" aria-label="Restart {c.name}" disabled={busy} onclick={() => containers.act(src!.key, c, { action: "restart" })}><RotateCw size={14} class={busy ? "animate-spin" : ""} /></button>
        <button class="icon-btn h-7 w-7 hover:text-danger" title="Remove…" aria-label="Remove {c.name}" disabled={busy} onclick={() => containers.act(src!.key, c, { action: "remove", force: false })}><Trash2 size={14} /></button>
      </div>
    </td>
  </tr>
{/snippet}

<div class="flex min-h-0 flex-1 flex-col bg-base">
  <!-- Source bar -->
  <div class="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-4 py-2.5">
    <h1 class="mr-2 flex items-center gap-2 text-sm font-semibold"><Container size={16} class="text-accent" /> Containers</h1>

    {#each Object.values(containers.sources) as s (s.key)}
      {@const env = hostEnv(s.key)}
      <div class="flex items-center rounded-md border {containers.activeKey === s.key ? 'border-accent bg-accent/10' : 'border-line hover:bg-panel-hover'}">
        <button class="flex items-center gap-1.5 px-2.5 py-1 text-xs" onclick={() => (containers.activeKey = s.key)} aria-pressed={containers.activeKey === s.key}>
          <span>{s.label}</span>
          {#if env?.short}<Badge tone={env.tone ?? "neutral"}>{env.short}</Badge>{/if}
        </button>
        <button class="icon-btn mr-0.5 h-5 w-5" title="Close {s.label}" aria-label="Close {s.label}" onclick={() => containers.close(s.key)}><X size={12} /></button>
      </div>
    {/each}
    {#each Object.keys(containers.connecting).filter((k) => containers.connecting[k]) as k (k)}
      <span class="flex items-center gap-1.5 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> Connecting to {containers.label(k)}…</span>
    {/each}

    <div class="ml-auto w-64">
      <Combobox bind:value={picker} {options} placeholder="Open a host or this computer…" ariaLabel="Open a source" emptyText="No host matches" onchange={choose} />
    </div>
  </div>

  {#if !src}
    <div class="flex flex-1 items-center justify-center">
      <EmptyState icon={Container} text="Open this computer or one of your hosts to see its containers.">
        <button class="btn-primary" onclick={() => containers.open(LOCAL)}>Open this computer</button>
      </EmptyState>
    </div>
  {:else}
    <!-- Controls -->
    <div class="flex flex-wrap items-center gap-2 border-b border-line px-4 py-2">
      <div class="flex overflow-hidden rounded-md border border-line text-xs" role="tablist" aria-label="What to list">
        {#each tabs as [v, text] (v)}
          <button role="tab" aria-selected={tab === v} class="px-3 py-1 {tab === v ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}" onclick={() => (tab = v)}>{text}</button>
        {/each}
      </div>

      <input class="input h-8 w-56 text-xs" placeholder="Search name, image, port, label…" aria-label="Search" bind:value={query} />

      {#if tab === "containers"}
        <div class="flex overflow-hidden rounded-md border border-line text-xs" role="group" aria-label="Filter by state">
          {#each [["all", `All ${counts.all}`], ["running", `Running ${counts.running}`], ["stopped", `Stopped ${counts.stopped}`]] as [v, text] (v)}
            <button aria-pressed={stateFilter === v} class="px-2.5 py-1 {stateFilter === v ? 'bg-accent/20 text-accent' : 'text-fg-muted hover:bg-panel-hover'}" onclick={() => (stateFilter = v as StateFilter)}>{text}</button>
          {/each}
        </div>
        {#if hasProjects}
          <label class="flex items-center gap-1.5 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={grouped} /> Group by Compose project</label>
        {/if}
      {:else if tab === "images"}
        <button class="btn-secondary py-1 text-xs" onclick={() => (ui.modal = { kind: "container-pull", sourceKey: src.key })}><Download size={12} /> Pull image…</button>
        <button class="btn-secondary py-1 text-xs" onclick={() => (ui.modal = { kind: "container-prune", sourceKey: src.key, what: "images" })}><Trash2 size={12} /> Remove unused…</button>
      {:else if tab === "volumes"}
        <button class="btn-secondary py-1 text-xs" onclick={() => (ui.modal = { kind: "container-prune", sourceKey: src.key, what: "volumes" })}><Trash2 size={12} /> Remove unused…</button>
      {:else if tab === "networks"}
        <button class="btn-secondary py-1 text-xs" onclick={() => (ui.modal = { kind: "container-prune", sourceKey: src.key, what: "networks" })}><Trash2 size={12} /> Remove unused…</button>
      {/if}

      <div class="ml-auto flex flex-wrap items-center gap-2 text-xs text-fg-muted">
        <label class="flex items-center gap-1.5">
          Runtime
          <select class="input h-7 w-28 py-0 text-xs" value={src.runtime} onchange={(e) => containers.setRuntime(src.key, e.currentTarget.value as ContainerRuntime)} aria-label="Container runtime">
            {#each CONTAINER_RUNTIMES as r (r)}<option value={r}>{r}{src.detected.includes(r) ? "" : " (not found)"}</option>{/each}
          </select>
        </label>
        <label class="flex items-center gap-1.5" title="Ask for container and volume sizes. Slower on hosts with many large containers.">
          <input type="checkbox" class="accent-input" checked={src.sizes} onchange={(e) => containers.setSizes(src.key, e.currentTarget.checked)} /> Sizes
        </label>
        <button class="btn-secondary py-1 text-xs" onclick={() => (containers.paused = !containers.paused)} aria-pressed={containers.paused} title={containers.paused ? "Resume refreshing on a timer" : "Stop refreshing on a timer"}>
          {#if containers.paused}<Play size={12} /> Resume{:else}<Pause size={12} /> Pause{/if}
        </button>
        <select class="input h-7 w-20 py-0 text-xs" bind:value={containers.intervalSecs} aria-label="Refresh every" disabled={containers.paused}>
          {#each REFRESH_CHOICES as n (n)}<option value={n}>{n} s</option>{/each}
        </select>
        <button class="icon-btn h-7 w-7" title="Refresh now" aria-label="Refresh now" onclick={() => containers.refresh(src.key)} disabled={src.loading}>
          <RefreshCw size={14} class={src.loading ? "animate-spin" : ""} />
        </button>
        <span class="w-20 text-right">{containers.paused ? "paused" : updated}</span>
      </div>
    </div>

    {#if src.error}
      <p class="m-4 whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{src.error}</p>
    {:else if src.resourcesError && (tab === "volumes" || tab === "networks")}
      <p class="m-4 whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 font-mono text-xs text-warning" role="alert">{src.resourcesError}</p>
    {/if}

    <div class="min-h-0 flex-1 overflow-auto">
      {#if !listing && !src.error}
        <div class="flex items-center justify-center gap-2 p-10 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Loading…</div>
      {:else if listing && tab === "containers"}
        {#if shownContainers.length === 0}
          <EmptyState compact={listing.containers.length > 0} icon={Container} text={listing.containers.length ? "No container matches the search or filter." : `No containers on ${src.label}.`} />
        {:else}
          <table class="w-full min-w-[56rem] text-left text-xs">
            <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
              <tr>
                <th class="px-4 py-2 font-medium">Name</th>
                <th class="px-2 py-2 font-medium">Image</th>
                <th class="px-2 py-2 font-medium">State</th>
                <th class="px-2 py-2 font-medium">Ports</th>
                <th class="px-2 py-2 text-right font-medium">Age</th>
                {#if src.sizes}<th class="px-2 py-2 font-medium">Size</th>{/if}
                <th class="px-4 py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#if groups}
                {#each groups as g (g.project ?? "\u0000none")}
                  {@const key = g.project ?? "\u0000none"}
                  {@const pbusy = g.project ? !!containers.busy[`project:${g.project}`] : false}
                  <tr class="border-b border-line bg-panel/70">
                    <td class="px-3 py-1.5" colspan={src.sizes ? 6 : 5}>
                      <button class="flex items-center gap-1.5 text-left" onclick={() => (collapsed[key] = !collapsed[key])} aria-expanded={!collapsed[key]}>
                        {#if collapsed[key]}<ChevronRight size={13} />{:else}<ChevronDown size={13} />{/if}
                        {#if g.project}
                          <span class="text-sm font-semibold" title={[g.workingDir && `Run in ${g.workingDir}`, g.configFiles && `Files: ${g.configFiles}`].filter(Boolean).join("\n")}>{g.project}</span>
                          <Badge tone="accent" title="Compose project">compose</Badge>
                        {:else}
                          <span class="text-sm font-semibold text-fg-muted">No project</span>
                        {/if}
                        <span class="font-normal text-fg-muted">{g.running} of {g.containers.length} running</span>
                      </button>
                    </td>
                    <td class="px-4 py-1.5">
                      {#if g.project && isDocker}
                        <div class="flex items-center justify-end gap-0.5">
                          <button class="icon-btn h-7 w-7" title="Start every container of {g.project}" aria-label="Start all of {g.project}" disabled={pbusy || g.running === g.containers.length} onclick={() => containers.compose(src.key, g.project!, "start", g.containers.length)}><Play size={14} /></button>
                          <button class="icon-btn h-7 w-7" title="Stop every container of {g.project}" aria-label="Stop all of {g.project}" disabled={pbusy || g.running === 0} onclick={() => containers.compose(src.key, g.project!, "stop", g.containers.length)}><Square size={14} /></button>
                          <button class="icon-btn h-7 w-7" title="Restart every container of {g.project}" aria-label="Restart all of {g.project}" disabled={pbusy} onclick={() => containers.compose(src.key, g.project!, "restart", g.containers.length)}><RotateCw size={14} class={pbusy ? "animate-spin" : ""} /></button>
                          <button class="icon-btn h-7 w-7 hover:text-danger" title="Take {g.project} down: remove its containers and networks, keep its volumes" aria-label="Take down {g.project}" disabled={pbusy} onclick={() => containers.compose(src.key, g.project!, "down", g.containers.length)}><Trash2 size={14} /></button>
                        </div>
                      {/if}
                    </td>
                  </tr>
                  {#if !collapsed[key]}
                    {#each g.containers as c (c.id)}{@render containerRow(c, true)}{/each}
                  {/if}
                {/each}
              {:else}
                {#each shownContainers as c (c.id)}{@render containerRow(c, false)}{/each}
              {/if}
            </tbody>
          </table>
        {/if}
      {:else if listing && tab === "images"}
        {#if listing.images_error}
          <p class="m-4 whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 font-mono text-xs text-warning" role="alert">Images couldn't be listed: {listing.images_error}</p>
        {/if}
        {#if shownImages.length === 0 && !listing.images_error}
          <EmptyState compact={listing.images.length > 0} icon={Container} text={listing.images.length ? "No image matches the search." : `No images on ${src.label}.`} />
        {:else if shownImages.length}
          <table class="w-full min-w-[40rem] text-left text-xs">
            <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
              <tr>
                <th class="px-4 py-2 font-medium">Repository</th>
                <th class="px-2 py-2 font-medium">Tag</th>
                <th class="px-2 py-2 font-medium">Image ID</th>
                <th class="px-2 py-2 text-right font-medium">Size</th>
                <th class="px-2 py-2 text-right font-medium">Age</th>
                <th class="px-2 py-2 text-right font-medium">Containers</th>
                <th class="px-4 py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each shownImages as i (i.id + i.repository + i.tag)}
                {@const ref = imageRef(i)}
                <tr class="border-b border-line/60 hover:bg-panel-hover/50">
                  <td class="max-w-72 truncate px-4 py-2 font-mono text-sm" title={i.repository}>{i.repository}</td>
                  <td class="px-2 py-2 font-mono">{i.tag}</td>
                  <td class="px-2 py-2 font-mono text-fg-muted">{i.id.slice(0, 12)}</td>
                  <td class="px-2 py-2 text-right">{i.size_text || "—"}</td>
                  <td class="px-2 py-2 text-right text-fg-muted" title={i.created ? new Date(i.created * 1000).toLocaleString() : ""}>{formatAge(i.created, now)}</td>
                  <td class="px-2 py-2 text-right text-fg-muted">{i.containers ?? "—"}</td>
                  <td class="px-4 py-2 text-right">
                    <button class="icon-btn h-7 w-7 hover:text-danger" title="Remove this image…" aria-label="Remove image {ref}" disabled={!!containers.busy[`image:${ref}`]} onclick={() => containers.removeResource(src.key, "image", ref, ref === i.id ? i.id.slice(0, 12) : ref)}><Trash2 size={14} /></button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {:else if tab === "volumes"}
        {#if !resources}
          <div class="flex items-center justify-center gap-2 p-10 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Loading volumes…</div>
        {:else if shownVolumes.length === 0}
          <EmptyState compact={resources.volumes.length > 0} icon={Container} text={resources.volumes.length ? "No volume matches the search." : `No volumes on ${src.label}.`} />
        {:else}
          <table class="w-full min-w-[44rem] text-left text-xs">
            <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
              <tr>
                <th class="px-4 py-2 font-medium">Name</th>
                <th class="px-2 py-2 font-medium">Driver</th>
                <th class="px-2 py-2 font-medium">Used by</th>
                {#if src.sizes}<th class="px-2 py-2 text-right font-medium">Size</th>{/if}
                <th class="px-4 py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each shownVolumes as v (v.name)}
                <tr class="border-b border-line/60 hover:bg-panel-hover/50">
                  <td class="max-w-80 px-4 py-2" title={v.mountpoint}>
                    <div class="truncate font-mono text-sm">{v.name}</div>
                    {#if v.labels["com.docker.compose.project"]}<Badge tone="accent" title="Compose project">{v.labels["com.docker.compose.project"]}</Badge>{/if}
                  </td>
                  <td class="px-2 py-2">{v.driver}</td>
                  <td class="max-w-64 truncate px-2 py-2 {v.used_by.length ? '' : 'text-fg-muted'}" title={v.used_by.join("\n")}>{usedByText(v.used_by)}</td>
                  {#if src.sizes}<td class="px-2 py-2 text-right text-fg-muted">{v.size ?? "—"}</td>{/if}
                  <td class="px-4 py-2 text-right">
                    <button class="icon-btn h-7 w-7 hover:text-danger" title={v.used_by.length ? `In use by ${v.used_by.join(", ")}` : "Remove this volume and its data…"} aria-label="Remove volume {v.name}" disabled={v.used_by.length > 0 || !!containers.busy[`volume:${v.name}`]} onclick={() => containers.removeResource(src.key, "volume", v.name, v.name)}><Trash2 size={14} /></button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {:else if tab === "networks"}
        {#if !resources}
          <div class="flex items-center justify-center gap-2 p-10 text-sm text-fg-muted"><Loader2 size={16} class="animate-spin" /> Loading networks…</div>
        {:else if shownNetworks.length === 0}
          <EmptyState compact={resources.networks.length > 0} icon={Container} text={resources.networks.length ? "No network matches the search." : `No networks on ${src.label}.`} />
        {:else}
          <table class="w-full min-w-[44rem] text-left text-xs">
            <thead class="sticky top-0 z-10 bg-panel text-fg-muted">
              <tr>
                <th class="px-4 py-2 font-medium">Name</th>
                <th class="px-2 py-2 font-medium">Driver</th>
                <th class="px-2 py-2 font-medium">Scope</th>
                <th class="px-2 py-2 font-medium">Used by</th>
                <th class="px-2 py-2 text-right font-medium">Age</th>
                <th class="px-4 py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each shownNetworks as n (n.id + n.name)}
                <tr class="border-b border-line/60 hover:bg-panel-hover/50">
                  <td class="max-w-72 px-4 py-2">
                    <div class="flex items-center gap-1.5">
                      <span class="truncate font-mono text-sm" title={n.id}>{n.name}</span>
                      {#if n.predefined}<Badge tone="neutral" title="Built in; it can't be removed">built-in</Badge>{/if}
                      {#if n.internal}<Badge tone="warning" title="No outside access">internal</Badge>{/if}
                      {#if n.ipv6}<Badge tone="neutral">IPv6</Badge>{/if}
                    </div>
                    {#if n.labels["com.docker.compose.project"]}<Badge tone="accent" title="Compose project">{n.labels["com.docker.compose.project"]}</Badge>{/if}
                  </td>
                  <td class="px-2 py-2">{n.driver}</td>
                  <td class="px-2 py-2">{n.scope}</td>
                  <td class="max-w-64 truncate px-2 py-2 {n.used_by.length ? '' : 'text-fg-muted'}" title={n.used_by.join("\n")}>{usedByText(n.used_by)}</td>
                  <td class="px-2 py-2 text-right text-fg-muted">{formatAge(n.created, now)}</td>
                  <td class="px-4 py-2 text-right">
                    <button class="icon-btn h-7 w-7 hover:text-danger" title={n.predefined ? "Built in; it can't be removed" : n.used_by.length ? `In use by ${n.used_by.join(", ")}` : "Remove this network…"} aria-label="Remove network {n.name}" disabled={n.predefined || n.used_by.length > 0 || !!containers.busy[`network:${n.name}`]} onclick={() => containers.removeResource(src.key, "network", n.name, n.name)}><Trash2 size={14} /></button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}
    </div>
  {/if}
</div>
