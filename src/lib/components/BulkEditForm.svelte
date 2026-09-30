<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import { groupOptions } from "$lib/pickeroptions";
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { ENVIRONMENTS, errorMessage, type Host, type Uuid } from "$lib/types";

  // Every field starts as "leave unchanged".
  const KEEP = "\u0000keep";
  const ids = [...ui.selectedHosts];
  let group = $state(KEEP);
  let env = $state(KEEP);
  let identity = $state(KEEP);
  let jump = $state(KEEP);
  let proxy = $state(KEEP);
  let addTags = $state("");
  let removeTags = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  const hosts = $derived(ids.map((id) => vaultStore.hostById.get(id)).filter((h): h is NonNullable<typeof h> => !!h?.data));
  const groupChoices = $derived(groupOptions(vaultStore.hosts, vaultStore.groups));

  async function apply(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = null;
    const add = addTags.split(",").map((t) => t.trim()).filter(Boolean);
    const del = new Set(removeTags.split(",").map((t) => t.trim()).filter(Boolean));
    let done = 0;
    try {
      for (const rec of hosts) {
        const h: Host = structuredClone($state.snapshot(rec.data!));
        if (group !== KEEP) h.group = group;
        if (env !== KEEP) h.environment = env || undefined;
        if (identity !== KEEP) h.identity_id = identity || undefined;
        if (jump !== KEEP && jump !== rec.id) h.jump_host_id = jump || undefined;
        if (proxy !== KEEP) h.proxy_id = proxy || undefined;
        h.tags = [...new Set([...h.tags.filter((t) => !del.has(t)), ...add])];
        await vaultStore.saveHost(rec.id, h, rec.rev);
        done++;
      }
      ui.notify("info", `Updated ${done} host${done === 1 ? "" : "s"}.`);
      ui.selectedHosts = new Set();
      ui.modal = null;
    } catch (err) {
      error = `${errorMessage(err)} (${done} of ${hosts.length} updated)`;
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Edit {hosts.length} host{hosts.length === 1 ? '' : 's'}" onclose={() => (ui.modal = null)}>
  <form id="bulk-form" class="space-y-3" onsubmit={apply}>
    <p class="text-xs text-fg-muted">Only the fields you change are applied. {hosts.map((h) => h.data!.label).slice(0, 6).join(", ")}{hosts.length > 6 ? ", …" : ""}</p>
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="b-group">Group</label>
        <Combobox
          id="b-group"
          options={groupChoices}
          bind:value={() => (group === KEEP ? "" : group), (v) => (group = v === "" ? KEEP : v)}
          creatable
          clearable
          createLabel={(q) => `New group “${q}”`}
          placeholder="(unchanged)"
        />
        {#if group !== KEEP && !group.trim()}<p class="mt-1 text-[11px] text-fg-muted">Empty moves them to the top level.</p>{/if}
      </div>
      <div>
        <label class="label" for="b-env">Environment</label>
        <select id="b-env" class="input" bind:value={env}>
          <option value={KEEP}>(unchanged)</option>
          {#each ENVIRONMENTS as e (e.value)}<option value={e.value}>{e.label}</option>{/each}
        </select>
      </div>
      <div>
        <label class="label" for="b-ident">Credential</label>
        <select id="b-ident" class="input" bind:value={identity}>
          <option value={KEEP}>(unchanged)</option>
          <option value="">None (use group default or ask)</option>
          {#each vaultStore.identities.filter((i) => !i.data?.for_host) as i (i.id)}<option value={i.id}>{i.data?.label} ({i.data?.username})</option>{/each}
        </select>
      </div>
      <div>
        <label class="label" for="b-jump">Jump host</label>
        <select id="b-jump" class="input" bind:value={jump}>
          <option value={KEEP}>(unchanged)</option>
          <option value="">None</option>
          {#each vaultStore.hosts.filter((h) => !ids.includes(h.id)) as h (h.id)}<option value={h.id}>{h.data?.label}</option>{/each}
        </select>
      </div>
      <div>
        <label class="label" for="b-proxy">Proxy</label>
        <select id="b-proxy" class="input" bind:value={proxy}>
          <option value={KEEP}>(unchanged)</option>
          <option value="">None</option>
          {#each vaultStore.proxies as p (p.id)}<option value={p.id}>{p.data?.name}</option>{/each}
        </select>
      </div>
      <div>
        <label class="label" for="b-add">Add tags</label>
        <input id="b-add" class="input" bind:value={addTags} placeholder="db, eu-west" />
      </div>
      <div class="col-span-2">
        <label class="label" for="b-del">Remove tags</label>
        <input id="b-del" class="input" bind:value={removeTags} placeholder="legacy" />
      </div>
    </div>
    {#if error}<p class="text-sm text-danger">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="bulk-form" disabled={busy || hosts.length === 0}>{busy ? "Saving…" : "Apply"}</button>
  {/snippet}
</Modal>
