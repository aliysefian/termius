<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { FileText, Loader2, Network, Pause, Play, RefreshCw, ScrollText, SquareTerminal, Square, Trash2, X } from "lucide-svelte";
  import Combobox from "./Combobox.svelte";
  import ContainerTabs from "./ContainerTabs.svelte";
  import EmptyState from "./EmptyState.svelte";
  import KubeLogs from "./KubeLogs.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { RESOURCE_KINDS, ageOf, isName, matches, matchesResource, portProblem, resourceTone, shellLine, toneOf, type KubeInfo, type KubePod, type KubeResource, type ResourceKind, type Scope } from "$lib/kubedata";
  import { ui } from "$lib/stores/ui.svelte";
  import { kubeState as k, type Forward } from "$lib/stores/kube.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { envInfo, errorMessage, type Uuid } from "$lib/types";

  const LOCAL = "local";
  const REFRESH_MS = 10_000;

  // -- the source: this computer or a host with saved credentials -------------------------------
  let picker = $state("");
  let connecting = $state(false);
  let error = $state<string | null>(null);

  const options = $derived([
    { value: LOCAL, label: "This computer" },
    ...vaultStore.hosts
      .filter((h) => h.data && vaultStore.effectiveIdentity(h.data))
      .map((h) => ({ value: h.id, label: h.data!.label }))
      .sort((a, b) => a.label.localeCompare(b.label)),
  ]);
  const production = $derived(k.source?.hostId ? envInfo(vaultStore.effectiveEnv(vaultStore.hostById.get(k.source.hostId)?.data)).value === "production" : false);

  async function choose(value: string) {
    if (!value) return;
    await closeSource();
    connecting = true;
    error = null;
    try {
      const hostId = value === LOCAL ? null : value;
      const r = await api.kube.open(hostId);
      k.source = { key: value, label: hostId ? (vaultStore.hostById.get(hostId)?.data?.label ?? "host") : "This computer", hostId, session: r.session_id, info: r.info };
      k.context = r.info.current ?? r.info.contexts[0] ?? "";
      k.namespace = "";
    } catch (e) {
      error = errorMessage(e);
    } finally {
      connecting = false;
      picker = "";
    }
  }

  async function closeSource() {
    logsFor = null;
    describe = null;
    pods = [];
    await k.close();
  }

  // -- pods ------------------------------------------------------------------------------------------
  let namespaces = $state<string[]>([]);
  let pods = $state<KubePod[]>([]);
  let loading = $state(false);
  let query = $state("");
  let auto = $state(true);
  let loadedAt = $state(0);
  let generation = 0;

  /** What is listed: pods, or one of the other read-only kinds. */
  let kind = $state<"pods" | ResourceKind>("pods");
  let resources = $state<KubeResource[]>([]);
  const kindLabel = $derived(RESOURCE_KINDS.find((x) => x.value === kind)?.label ?? "Pods");
  const shownResources = $derived(resources.filter((r) => matchesResource(r, query)));
  /** Columns for the other kinds come from the first row's details. */
  const columns = $derived(resources[0]?.details.map((d) => d.label) ?? []);

  const scope = $derived<Scope>(k.namespace ? { scope: "namespace", name: k.namespace } : { scope: "all" });
  const shown = $derived(pods.filter((p) => matches(p, query)));
  const unhealthy = $derived(pods.filter((p) => toneOf(p) === "bad").length);

  async function load(quiet = false) {
    if (!k.source || !k.context) return;
    const mine = ++generation;
    if (!quiet) loading = true;
    try {
      const wanted = kind;
      const [list, ns] = await Promise.all([
        wanted === "pods" ? api.kube.pods(k.source.session, k.context, scope) : Promise.resolve([] as KubePod[]),
        namespaces.length && quiet ? Promise.resolve(namespaces) : api.kube.namespaces(k.source.session, k.context),
      ]);
      const others = wanted === "pods" ? [] : await api.kube.resources(k.source.session, k.context, scope, wanted);
      if (mine !== generation) return;
      pods = list;
      resources = others;
      namespaces = ns;
      loadedAt = Date.now();
      error = null;
    } catch (e) {
      if (mine === generation) error = errorMessage(e);
    } finally {
      if (mine === generation) loading = false;
    }
  }

  // Reload when what is looked at changes.
  $effect(() => {
    k.source;
    k.context;
    k.namespace;
    kind;
    // The load reads and writes other state; only the four above should make it run again.
    untrack(() => {
      pods = [];
      resources = [];
      if (k.source && k.context) void load();
    });
  });

  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    timer = setInterval(() => {
      if (auto && k.source && !loading && !document.hidden) void load(true);
    }, REFRESH_MS);
  });
  // Leaving the page keeps the connection (the store holds it); only this page's timer stops.
  onDestroy(() => clearInterval(timer));

  // -- one pod ------------------------------------------------------------------------------------------
  let logsFor = $state<KubePod | null>(null);
  let describe = $state<{ pod: string; text: string } | null>(null);
  let busy = $state<string | null>(null);

  async function showDescribe(p: KubePod) {
    if (!k.source) return;
    busy = `describe:${p.name}`;
    try {
      describe = { pod: p.name, text: await api.kube.describe(k.source.session, k.context, p.namespace, p.name) };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = null;
    }
  }

  async function showResource(r: KubeResource) {
    if (!k.source || kind === "pods") return;
    busy = `describe:${r.name}`;
    try {
      describe = { pod: `${kindLabel.replace(/s$/, "")} ${r.name}`, text: await api.kube.describeResource(k.source.session, k.context, r.namespace, kind, r.name) };
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = null;
    }
  }

  async function remove(p: KubePod) {
    if (!k.source) return;
    const where = `${p.namespace}/${p.name} on ${k.context}`;
    const ok = await ask(`Delete the pod ${where}? ${p.owner ? `It belongs to ${p.owner}, which will usually start a new one.` : "Nothing owns it, so it will not come back."}`, {
      title: "Delete pod",
      confirm: "Delete",
      danger: true,
      // On a production host, the name has to be typed.
      requireText: production ? p.name : undefined,
    });
    if (!ok) return;
    busy = `delete:${p.name}`;
    try {
      await api.kube.deletePod(k.source.session, k.context, p.namespace, p.name);
      ui.notify("info", `Deleting ${p.name}.`);
      await load(true);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = null;
    }
  }

  function shell(p: KubePod, container?: string) {
    const line = shellLine(k.context, p.namespace, p.name, container);
    if (!k.source || !line) return ui.notify("error", "That pod's name can't be used in a command.");
    const title = `${p.name} · ${k.source.label}`;
    if (k.source.hostId) ui.openTerminal(k.source.hostId, title, line);
    else ui.openLocal(line, title);
  }

  // -- port-forwards (local only: on a host the port would open on the host) ------------------------------
  let fwdFor = $state<KubePod | null>(null);
  let fwdLocal = $state("");
  let fwdRemote = $state("");
  let fwdService = $state("");

  const fwdProblem = $derived(portProblem(fwdLocal) ?? portProblem(fwdRemote) ?? (fwdService && !isName(fwdService) ? "That isn't a service name." : null));

  async function startForward() {
    if (!k.source || !fwdFor || fwdProblem) return;
    const p = fwdFor;
    const kind = fwdService ? "service" : "pod";
    const name = fwdService || p.name;
    const label = `${kind}/${name} :${fwdRemote} → localhost:${fwdLocal}`;
    // Changes go through the list the page reads (not the object that was pushed into it), so they show.
    const patch = (fields: Partial<Forward>) => {
      const f = k.forwards.find((x) => x.label === label);
      if (f) Object.assign(f, fields);
    };
    k.forwards = [...k.forwards, { stream: "", label, state: "starting", message: "" }];
    try {
      const stream = await api.kube.forward(k.source.session, k.context, p.namespace, { kind, name }, Number(fwdLocal), Number(fwdRemote), (e) => {
        const now = k.forwards.find((x) => x.label === label);
        if (!now) return;
        if (e.event === "chunk") {
          if (/Forwarding from/.test(e.text)) patch({ state: "listening" });
          else patch({ message: e.text.trim().split("\n").pop() ?? "" });
        } else patch({ state: now.state === "listening" || !e.error ? "ended" : "error", message: e.error ?? now.message });
      });
      patch({ stream });
      fwdFor = null;
      fwdLocal = fwdRemote = fwdService = "";
    } catch (e) {
      patch({ state: "error", message: errorMessage(e) });
    }
  }

  async function stopForward(f: Forward) {
    if (f.stream) await api.kube.stop(f.stream).catch(() => {});
    k.forwards = k.forwards.filter((x) => x !== f);
  }

  const dot = { good: "bg-success", warn: "bg-warning", bad: "bg-danger", muted: "bg-fg-muted/50" } as const;
  const text = { good: "", warn: "text-warning", bad: "text-danger", muted: "text-fg-muted" } as const;
</script>

<div class="flex min-h-0 min-w-0 flex-1 flex-col bg-base">
  <ContainerTabs current="kubernetes" />

  <div class="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-4 py-2.5">
    <h1 class="mr-2 text-sm font-semibold">Kubernetes</h1>
    {#if k.source}
      <span class="flex items-center gap-1.5 rounded-md border border-accent bg-accent/10 px-2.5 py-1 text-xs">
        {k.source.label}
        <span class="text-fg-muted">kubectl {k.source.info.version}</span>
        <button class="icon-btn h-5 w-5" title="Close {k.source.label}" aria-label="Close {k.source.label}" onclick={() => void closeSource()}><X size={12} /></button>
      </span>
    {/if}
    {#if connecting}<span class="flex items-center gap-1.5 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> Connecting…</span>{/if}
    <div class="ml-auto w-64">
      <Combobox bind:value={picker} {options} placeholder="Open a host or this computer…" ariaLabel="Open a source" emptyText="No host matches" onchange={choose} />
    </div>
  </div>

  {#if error && !k.source}
    <p class="m-4 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger whitespace-pre-wrap">{error}</p>
  {/if}

  {#if !k.source}
    <div class="flex flex-1 items-center justify-center">
      <EmptyState art="containers" text="Open this computer or one of your hosts to see its clusters. It uses the kubectl that is already there and its own kubeconfig.">
        <button class="btn-primary" disabled={connecting} onclick={() => void choose(LOCAL)}>Open this computer</button>
      </EmptyState>
    </div>
  {:else}
    <div class="flex flex-wrap items-center gap-2 border-b border-line px-4 py-2">
      <label class="flex items-center gap-1.5 text-xs text-fg-muted">Context
        <select class="input w-56 py-1 text-xs" bind:value={k.context} aria-label="Context">
          {#each k.source.info.contexts as c (c)}<option value={c}>{c}</option>{:else}<option value="">No contexts</option>{/each}
        </select>
      </label>
      <label class="flex items-center gap-1.5 text-xs text-fg-muted">Namespace
        <select class="input w-44 py-1 text-xs" bind:value={k.namespace} aria-label="Namespace">
          <option value="">All namespaces</option>
          {#each namespaces as n (n)}<option value={n}>{n}</option>{/each}
        </select>
      </label>
      <label class="flex items-center gap-1.5 text-xs text-fg-muted">Show
        <select class="input w-36 py-1 text-xs" bind:value={kind} aria-label="Kind">
          <option value="pods">Pods</option>
          {#each RESOURCE_KINDS as r (r.value)}<option value={r.value}>{r.label}</option>{/each}
        </select>
      </label>
      <input class="input w-52 py-1 text-xs" bind:value={query} placeholder="Search {kind === 'pods' ? 'pods' : kindLabel.toLowerCase()}…" aria-label="Search {kind === 'pods' ? 'pods' : kindLabel.toLowerCase()}" />
      <span class="ml-auto flex items-center gap-2">
        {#if unhealthy && kind === "pods"}<span class="text-xs text-danger">{unhealthy} unhealthy</span>{/if}
        <span class="text-[11px] text-fg-muted">{kind === "pods" ? `${shown.length} of ${pods.length} pods` : `${shownResources.length} of ${resources.length} ${kindLabel.toLowerCase()}`}{loadedAt ? ` · ${new Date(loadedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}` : ""}</span>
        <button class="icon-btn" title={auto ? "Pause refreshing (every 10 s)" : "Refresh every 10 s"} aria-pressed={auto} onclick={() => (auto = !auto)}>{#if auto}<Pause size={15} />{:else}<Play size={15} />{/if}</button>
        <button class="btn-secondary py-1 text-xs" disabled={loading} onclick={() => void load()}>
          {#if loading}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if} Refresh
        </button>
      </span>
    </div>

    <div class="flex-1 overflow-y-auto">
      {#if error}
        <p class="m-4 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger whitespace-pre-wrap">{error}</p>
      {/if}
      {#if !k.source.info.contexts.length}
        <div class="p-10"><EmptyState art="security" text="kubectl has no contexts here. Set up a kubeconfig on this machine first." /></div>
      {:else if loading && !pods.length && !resources.length}
        <div class="flex items-center gap-2 p-8 text-sm text-fg-muted"><Loader2 size={15} class="animate-spin" /> Asking the cluster…</div>
      {:else if kind !== "pods"}
        {#if !resources.length && !error}
          <div class="p-10"><EmptyState art="containers" text={k.namespace && kind !== "nodes" ? `No ${kindLabel.toLowerCase()} in ${k.namespace}.` : `No ${kindLabel.toLowerCase()} found.`} /></div>
        {:else if resources.length}
          <table class="w-full text-left text-xs">
            <thead class="sticky top-0 z-10 bg-base text-fg-muted">
              <tr>
                <th class="px-4 py-1.5 font-medium">{kindLabel.replace(/s$/, "")}</th><th class="font-medium">Status</th>
                {#each columns as c (c)}<th class="font-medium">{c}</th>{/each}
                <th class="font-medium">Age</th><th class="pr-4 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each shownResources as r (`${r.namespace}/${r.name}`)}
                {@const tone = resourceTone(kind, r)}
                <tr class="border-t border-line/60 hover:bg-panel-hover/50">
                  <td class="max-w-80 px-4 py-1.5">
                    <div class="truncate font-medium" title={r.name}>{r.name}</div>
                    {#if r.namespace && !k.namespace}<div class="truncate text-[11px] text-fg-muted">{r.namespace}</div>{/if}
                  </td>
                  <td class="whitespace-nowrap {text[tone]}"><span class="mr-1.5 inline-block h-2 w-2 rounded-full align-middle {dot[tone]}"></span>{r.status}</td>
                  {#each r.details as dt (dt.label)}<td class="max-w-72 truncate text-fg-muted" title={dt.value}>{dt.value || "—"}</td>{/each}
                  <td class="font-mono text-fg-muted">{ageOf(r.created)}</td>
                  <td class="pr-4 text-right whitespace-nowrap">
                    <button class="icon-btn h-7 w-7" title="Describe" aria-label="Describe {r.name}" disabled={busy === `describe:${r.name}`} onclick={() => void showResource(r)}><FileText size={14} /></button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {:else if !pods.length && !error}
        <div class="p-10"><EmptyState art="containers" text={k.namespace ? `No pods in ${k.namespace}.` : "No pods in this cluster."} /></div>
      {:else if pods.length}
        <table class="w-full text-left text-xs">
          <thead class="sticky top-0 z-10 bg-base text-fg-muted">
            <tr><th class="px-4 py-1.5 font-medium">Pod</th><th class="font-medium">Status</th><th class="font-medium">Ready</th><th class="font-medium">Restarts</th><th class="font-medium">Age</th><th class="font-medium">Node</th><th class="pr-4 text-right font-medium">Actions</th></tr>
          </thead>
          <tbody>
            {#each shown as p (`${p.namespace}/${p.name}`)}
              {@const tone = toneOf(p)}
              <tr class="border-t border-line/60 hover:bg-panel-hover/50">
                <td class="max-w-80 px-4 py-1.5">
                  <div class="truncate font-medium" title={p.name}>{p.name}</div>
                  <div class="truncate text-[11px] text-fg-muted">{#if !k.namespace}{p.namespace} · {/if}{p.owner ?? "no owner"}</div>
                </td>
                <td class="whitespace-nowrap {text[tone]}"><span class="mr-1.5 inline-block h-2 w-2 rounded-full align-middle {dot[tone]}"></span>{p.status}</td>
                <td class="font-mono">{p.ready}</td>
                <td class="font-mono {p.restarts > 3 ? 'text-warning' : ''}">{p.restarts}</td>
                <td class="font-mono text-fg-muted">{ageOf(p.created)}</td>
                <td class="max-w-40 truncate text-fg-muted" title={p.node ?? ""}>{p.node ?? "—"}</td>
                <td class="pr-4 text-right whitespace-nowrap">
                  <button class="icon-btn h-7 w-7" title="Logs" aria-label="Logs of {p.name}" onclick={() => (logsFor = p)}><ScrollText size={14} /></button>
                  <button class="icon-btn h-7 w-7" title="Describe" aria-label="Describe {p.name}" disabled={busy === `describe:${p.name}`} onclick={() => void showDescribe(p)}><FileText size={14} /></button>
                  <button class="icon-btn h-7 w-7" title="Open a shell in a terminal tab" aria-label="Shell in {p.name}" onclick={() => shell(p, p.containers.length > 1 ? undefined : p.containers[0])}><SquareTerminal size={14} /></button>
                  {#if !k.source.hostId}
                    <button class="icon-btn h-7 w-7" title="Forward a port to this computer" aria-label="Port-forward {p.name}" onclick={() => (fwdFor = p)}><Network size={14} /></button>
                  {/if}
                  <button class="icon-btn h-7 w-7 hover:text-danger" title="Delete the pod…" aria-label="Delete {p.name}" disabled={busy === `delete:${p.name}`} onclick={() => void remove(p)}><Trash2 size={14} /></button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      {#if k.forwards.length}
        <div class="m-4 rounded-xl border border-line bg-panel">
          <div class="border-b border-line px-3 py-2 text-sm font-semibold">Port-forwards</div>
          <ul class="divide-y divide-line/60">
            {#each k.forwards as f (f.label)}
              <li class="flex items-center gap-3 px-3 py-2 text-xs">
                <span class="h-2 w-2 shrink-0 rounded-full {f.state === 'listening' ? 'bg-success' : f.state === 'error' ? 'bg-danger' : f.state === 'ended' ? 'bg-fg-muted/50' : 'bg-warning'}"></span>
                <span class="min-w-0 flex-1 truncate font-mono">{f.label}</span>
                <span class="max-w-64 truncate text-fg-muted" title={f.message}>{f.state === "listening" ? "listening on 127.0.0.1" : f.message || f.state}</span>
                <button class="btn-ghost py-0.5 text-xs" onclick={() => void stopForward(f)}><Square size={11} /> Stop</button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if fwdFor}
        <form class="m-4 rounded-xl border border-line bg-panel p-4" onsubmit={(e) => { e.preventDefault(); void startForward(); }}>
          <div class="mb-2 flex items-center justify-between"><h2 class="text-sm font-semibold">Forward a port from {fwdFor.name}</h2><button type="button" class="icon-btn" aria-label="Cancel" onclick={() => (fwdFor = null)}><X size={15} /></button></div>
          <div class="grid grid-cols-3 gap-3">
            <div><label class="label" for="kf-remote">Port in the pod or service</label><input id="kf-remote" class="input font-mono" bind:value={fwdRemote} inputmode="numeric" placeholder="80" /></div>
            <div><label class="label" for="kf-local">On this computer</label><input id="kf-local" class="input font-mono" bind:value={fwdLocal} inputmode="numeric" placeholder="8080" /></div>
            <div><label class="label" for="kf-svc">Or a service <span class="font-normal normal-case text-fg-muted">(optional)</span></label><input id="kf-svc" class="input font-mono" bind:value={fwdService} placeholder="web" spellcheck="false" /></div>
          </div>
          {#if fwdProblem && (fwdLocal || fwdRemote || fwdService)}<p class="mt-2 text-xs text-danger">{fwdProblem}</p>{/if}
          <p class="mt-2 text-xs text-fg-muted">It listens on 127.0.0.1 only, until you stop it or close this page.</p>
          <div class="mt-3 flex justify-end"><button class="btn-primary" type="submit" disabled={!!fwdProblem || !fwdLocal || !fwdRemote}>Forward</button></div>
        </form>
      {/if}

      {#if describe}
        <div class="m-4 rounded-xl border border-line bg-panel p-3">
          <div class="mb-1 flex items-center justify-between text-xs"><span class="font-medium">{describe.pod}</span><button class="btn-ghost py-0.5 text-xs" onclick={() => (describe = null)}>Close</button></div>
          <pre class="max-h-96 overflow-auto rounded-md bg-base p-3 font-mono text-[11px] leading-5 whitespace-pre-wrap">{describe.text}</pre>
        </div>
      {/if}

      {#if logsFor}
        <div class="m-4">
          {#key `${logsFor.namespace}/${logsFor.name}`}
            <KubeLogs session={k.source.session} context={k.context} namespace={logsFor.namespace} pod={logsFor.name} containers={logsFor.containers} onclose={() => (logsFor = null)} />
          {/key}
        </div>
      {/if}
    </div>
  {/if}
</div>
