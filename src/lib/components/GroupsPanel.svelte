<script lang="ts">
  import { FolderTree, Network, Pencil, Plus, ShieldAlert, Trash2 } from "lucide-svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { ENVIRONMENTS, errorMessage, type HostGroup, type Proxy, type ProxySpec, type Uuid } from "$lib/types";

  // -- groups ------------------------------------------------------------

  /** Every folder that has hosts (including parents) or saved defaults. */
  const paths = $derived.by(() => {
    const set = new Set<string>();
    for (const h of vaultStore.hosts) {
      const parts = (h.data?.group ?? "").split("/").map((p) => p.trim()).filter(Boolean);
      for (let i = 1; i <= parts.length; i++) set.add(parts.slice(0, i).join("/"));
    }
    for (const g of vaultStore.groups) if (g.data) set.add(g.data.path);
    return [...set].sort();
  });

  function hostCount(path: string) {
    return vaultStore.hosts.filter((h) => {
      const g = (h.data?.group ?? "").split("/").map((p) => p.trim()).filter(Boolean).join("/");
      return g === path || g.startsWith(`${path}/`);
    }).length;
  }

  let editing = $state<string | null>(null);
  let draft = $state<HostGroup>({ path: "", notes: "" });
  let newPath = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  function edit(path: string) {
    const rec = vaultStore.groupByPath.get(path);
    draft = rec?.data ? structuredClone($state.snapshot(rec.data)) : { path, notes: "" };
    editing = path;
    error = null;
  }

  async function saveGroup(e: SubmitEvent) {
    e.preventDefault();
    if (!editing) return;
    const rec = vaultStore.groupByPath.get(editing);
    busy = true;
    error = null;
    try {
      const g: HostGroup = {
        ...$state.snapshot(draft),
        default_identity_id: draft.default_identity_id || undefined,
        default_jump_host_id: draft.default_jump_host_id || undefined,
        proxy_id: draft.proxy_id || undefined,
        environment: draft.environment || undefined,
      };
      await vaultStore.saveGroup(rec?.id ?? null, g, rec?.rev ?? null);
      editing = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function clearGroup(path: string) {
    const rec = vaultStore.groupByPath.get(path);
    if (!rec || !await ask(`Remove the defaults for "${path}"? Its hosts stay where they are.`)) return;
    try {
      await vaultStore.deleteGroup(rec.id, rec.rev);
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }

  function summary(g: HostGroup | undefined): string {
    if (!g) return "No defaults";
    const parts: string[] = [];
    if (g.default_identity_id) parts.push(`credential ${vaultStore.identityById.get(g.default_identity_id)?.data?.label ?? "(missing)"}`);
    if (g.default_jump_host_id) parts.push(`via ${vaultStore.hostById.get(g.default_jump_host_id)?.data?.label ?? "(missing)"}`);
    if (g.proxy_id) parts.push(`proxy ${vaultStore.proxyById.get(g.proxy_id)?.data?.name ?? "(missing)"}`);
    if (g.environment) parts.push(g.environment);
    return parts.join(" · ") || "No defaults";
  }

  // -- proxies -------------------------------------------------------------

  let proxyEditing = $state<Uuid | "new" | null>(null);
  let proxyName = $state("");
  let proxyKind = $state<ProxySpec["kind"]>("socks5");
  let proxyHost = $state("");
  let proxyPort = $state(1080);
  let proxyUser = $state("");
  let proxyPass = $state("");
  let proxyCommand = $state("");
  let proxyApproved = $state(false);
  let proxyHadPassword = $state(false);

  function editProxy(id: Uuid | "new") {
    const p = id === "new" ? undefined : vaultStore.proxyById.get(id)?.data;
    proxyEditing = id;
    proxyName = p?.name ?? "";
    proxyKind = p?.spec.kind ?? "socks5";
    proxyHost = p && p.spec.kind !== "command" ? p.spec.host : "";
    proxyPort = p && p.spec.kind !== "command" ? p.spec.port : 1080;
    proxyUser = p && p.spec.kind !== "command" ? (p.spec.username ?? "") : "";
    proxyHadPassword = !!p && p.spec.kind !== "command" && p.spec.password !== undefined;
    proxyPass = "";
    proxyCommand = p?.spec.kind === "command" ? p.spec.command : "";
    proxyApproved = p?.spec.kind === "command" ? p.spec.approved : false;
    error = null;
  }

  async function saveProxy(e: SubmitEvent) {
    e.preventDefault();
    if (!proxyEditing) return;
    const spec: ProxySpec =
      proxyKind === "command"
        ? { kind: "command", command: proxyCommand.trim(), approved: proxyApproved }
        : {
            kind: proxyKind,
            host: proxyHost.trim(),
            port: Number(proxyPort),
            username: proxyUser.trim() || undefined,
            // Empty keeps the saved password.
            password: proxyUser.trim() ? (proxyPass || (proxyHadPassword ? "" : undefined)) : undefined,
          };
    const id = proxyEditing === "new" ? null : proxyEditing;
    busy = true;
    error = null;
    try {
      const p: Proxy = { name: proxyName.trim(), spec };
      await vaultStore.saveProxy(id, p, id ? (vaultStore.proxyById.get(id)?.rev ?? null) : null);
      proxyEditing = null;
      proxyPass = "";
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  function proxyUsers(id: Uuid) {
    return (
      vaultStore.hosts.filter((h) => h.data?.proxy_id === id).length +
      vaultStore.groups.filter((g) => g.data?.proxy_id === id).length
    );
  }

  async function removeProxy(id: Uuid) {
    const n = proxyUsers(id);
    if (!await ask(`Delete this proxy?${n ? ` ${n} host(s) or group(s) use it and will connect directly.` : ""}`)) return;
    try {
      await vaultStore.deleteProxy(id);
    } catch (err) {
      ui.notify("error", errorMessage(err));
    }
  }

  function describe(p: Proxy) {
    return p.spec.kind === "command" ? p.spec.command : `${p.spec.kind.toUpperCase()} ${p.spec.host}:${p.spec.port}${p.spec.username ? ` as ${p.spec.username}` : ""}`;
  }
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-3xl space-y-8">
    <section class="space-y-3">
      <h1 class="flex items-center gap-2 text-lg font-semibold"><FolderTree size={18} class="text-accent" /> Groups</h1>
      <p class="text-xs text-fg-muted">
        Hosts in a group (or any sub-group) use its defaults when they don't set their own: credential, jump host, proxy and
        environment. The nearest group wins. Assign hosts to groups in the host form or by dragging them in the host list.
      </p>
      <form class="flex gap-2" onsubmit={(e) => (e.preventDefault(), newPath.trim() && (edit(newPath.trim().replace(/\s*\/\s*/g, "/")), (newPath = "")))}>
        <input class="input flex-1" bind:value={newPath} placeholder="New group, e.g. Production/Databases" />
        <button class="btn-ghost border border-line" disabled={!newPath.trim()}><Plus size={14} /> Add</button>
      </form>

      <div class="divide-y divide-line rounded-xl border border-line bg-panel">
        {#each editing && !paths.includes(editing) ? [...paths, editing] : paths as path (path)}
          {@const rec = vaultStore.groupByPath.get(path)}
          <div class="px-4 py-2.5">
            <div class="flex items-center gap-3">
              <span class="h-2 w-2 shrink-0 rounded-full" style:background={rec?.data?.color ?? "transparent"}></span>
              <div class="min-w-0 flex-1">
                <div class="truncate text-sm" style:padding-left="{(path.split('/').length - 1) * 12}px">{path.split("/").at(-1)} <span class="text-xs text-fg-muted">({hostCount(path)})</span></div>
                <div class="truncate text-xs text-fg-muted" style:padding-left="{(path.split('/').length - 1) * 12}px">{summary(rec?.data)}</div>
              </div>
              <button class="icon-btn h-7 w-7" title="Edit defaults" onclick={() => edit(path)}><Pencil size={13} /></button>
              {#if rec}
                <button class="icon-btn h-7 w-7 hover:text-danger" title="Remove defaults" onclick={() => clearGroup(path)}><Trash2 size={13} /></button>
              {/if}
            </div>
            {#if editing === path}
              <form class="mt-3 grid grid-cols-2 gap-3 rounded-lg border border-line bg-base/40 p-3" onsubmit={saveGroup}>
                <div>
                  <label class="label" for="g-ident">Default credential</label>
                  <select id="g-ident" class="input" bind:value={draft.default_identity_id}>
                    <option value={undefined}>None</option>
                    {#each vaultStore.identities.filter((i) => !i.data?.for_host) as i (i.id)}
                      <option value={i.id}>{i.data?.label} ({i.data?.username})</option>
                    {/each}
                  </select>
                </div>
                <div>
                  <label class="label" for="g-jump">Default jump host / bastion</label>
                  <select id="g-jump" class="input" bind:value={draft.default_jump_host_id}>
                    <option value={undefined}>None</option>
                    {#each vaultStore.hosts as h (h.id)}
                      <option value={h.id}>{h.data?.label}</option>
                    {/each}
                  </select>
                </div>
                <div>
                  <label class="label" for="g-proxy">Proxy</label>
                  <select id="g-proxy" class="input" bind:value={draft.proxy_id}>
                    <option value={undefined}>None</option>
                    {#each vaultStore.proxies as p (p.id)}
                      <option value={p.id}>{p.data?.name}</option>
                    {/each}
                  </select>
                </div>
                <div>
                  <label class="label" for="g-env">Environment</label>
                  <input id="g-env" class="input" list="g-envs" bind:value={draft.environment} placeholder="production, staging, …" />
                  <datalist id="g-envs">
                    {#each ENVIRONMENTS.filter((e) => e.value) as e (e.value)}<option value={e.value}></option>{/each}
                  </datalist>
                </div>
                <div class="col-span-2">
                  <label class="label" for="g-notes">Notes</label>
                  <input id="g-notes" class="input" bind:value={draft.notes} />
                </div>
                {#if error}<p class="col-span-2 text-sm text-danger">{error}</p>{/if}
                <div class="col-span-2 flex gap-2">
                  <button class="btn-primary" disabled={busy}>Save</button>
                  <button class="btn-ghost" type="button" onclick={() => (editing = null)}>Cancel</button>
                </div>
              </form>
            {/if}
          </div>
        {:else}
          <p class="px-4 py-8 text-center text-sm text-fg-muted">No groups yet. Give hosts a group in the host form, or add one above.</p>
        {/each}
      </div>
    </section>

    <section class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="flex items-center gap-2 text-lg font-semibold"><Network size={18} class="text-accent" /> Proxies</h2>
        <button class="btn-ghost border border-line" onclick={() => editProxy("new")}><Plus size={14} /> Add proxy</button>
      </div>
      <p class="text-xs text-fg-muted">
        A SOCKS5 or HTTP proxy, or an OpenSSH <code>ProxyCommand</code>, used to reach a host (or the first hop of its jump
        chain). Proxies resolve host names themselves, so no local DNS lookup reveals where you connect.
      </p>
      <div class="divide-y divide-line rounded-xl border border-line bg-panel">
        {#each vaultStore.proxies as p (p.id)}
          {@const d = p.data!}
          <div class="flex items-center gap-3 px-4 py-2.5">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-1.5 text-sm">
                {d.name}
                {#if d.spec.kind === "command" && !d.spec.approved}
                  <span class="rounded bg-warning/15 px-1 text-[9px] font-bold text-warning">NOT APPROVED</span>
                {/if}
              </div>
              <div class="truncate font-mono text-[11px] text-fg-muted">{describe(d)}</div>
            </div>
            <span class="text-xs text-fg-muted">{proxyUsers(p.id)} using</span>
            <button class="icon-btn h-7 w-7" title="Edit" onclick={() => editProxy(p.id)}><Pencil size={13} /></button>
            <button class="icon-btn h-7 w-7 hover:text-danger" title="Delete" onclick={() => removeProxy(p.id)}><Trash2 size={13} /></button>
          </div>
        {:else}
          {#if proxyEditing !== "new"}<p class="px-4 py-6 text-center text-sm text-fg-muted">No proxies.</p>{/if}
        {/each}

        {#if proxyEditing}
          <form class="grid grid-cols-2 gap-3 bg-base/40 p-4" onsubmit={saveProxy}>
            <div>
              <label class="label" for="p-name">Name</label>
              <input id="p-name" class="input" bind:value={proxyName} required placeholder="office SOCKS" />
            </div>
            <div>
              <label class="label" for="p-kind">Type</label>
              <select id="p-kind" class="input" bind:value={proxyKind}>
                <option value="socks5">SOCKS5</option>
                <option value="http">HTTP CONNECT</option>
                <option value="command">ProxyCommand</option>
              </select>
            </div>
            {#if proxyKind === "command"}
              <div class="col-span-2">
                <label class="label" for="p-cmd">Command <span class="font-normal text-fg-muted">(%h host, %p port, %r user)</span></label>
                <input id="p-cmd" class="input font-mono text-xs" bind:value={proxyCommand} required placeholder="ssh -W %h:%p bastion" spellcheck="false" />
              </div>
              <label class="col-span-2 flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 p-3 text-xs">
                <input type="checkbox" class="mt-0.5 accent-[#7b61ff]" bind:checked={proxyApproved} />
                <span>
                  <span class="flex items-center gap-1 font-semibold text-warning"><ShieldAlert size={12} /> Allow this command to run on my computers</span>
                  It runs as you on every device that uses this vault, whenever a host using this proxy connects. Only approve
                  commands you've read and understand, especially ones that were imported.
                </span>
              </label>
            {:else}
              <div>
                <label class="label" for="p-host">Proxy address</label>
                <input id="p-host" class="input font-mono" bind:value={proxyHost} required placeholder="proxy.example.com" />
              </div>
              <div>
                <label class="label" for="p-port">Port</label>
                <input id="p-port" class="input font-mono" type="number" min="1" max="65535" bind:value={proxyPort} required />
              </div>
              <div>
                <label class="label" for="p-user">Username (optional)</label>
                <input id="p-user" class="input" bind:value={proxyUser} autocomplete="off" />
              </div>
              <div>
                <label class="label" for="p-pass">Password</label>
                <input id="p-pass" class="input" type="password" bind:value={proxyPass} autocomplete="off" placeholder={proxyHadPassword ? "Saved. Leave empty to keep it." : ""} disabled={!proxyUser.trim()} />
              </div>
            {/if}
            {#if error}<p class="col-span-2 text-sm text-danger">{error}</p>{/if}
            <div class="col-span-2 flex gap-2">
              <button class="btn-primary" disabled={busy}>Save proxy</button>
              <button class="btn-ghost" type="button" onclick={() => (proxyEditing = null)}>Cancel</button>
            </div>
          </form>
        {/if}
      </div>
    </section>
  </div>
</div>
