<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import Modal, { DISCARD } from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptyForward, errorMessage, type ForwardKind, type ForwardRule, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();

  // Remounted via {#key} when id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existingRec = id ? vaultStore.forwards.find((f) => f.id === id) : undefined;
  const existing = existingRec?.data;
  const baseRev = existingRec?.rev ?? null;
  const base = existing ? structuredClone($state.snapshot(existing)) : emptyForward(vaultStore.hosts[0]?.id);
  let label = $state(base.label);
  let autoStart = $state(base.auto_start ?? false);
  let hostId = $state(base.host_id);
  let kind = $state<ForwardKind["kind"]>(base.kind);
  let bindAddr = $state(base.bind_addr);
  let bindPort = $state(base.bind_port);
  let destHost = $state("dest_host" in base ? base.dest_host : "127.0.0.1");
  let destPort = $state("dest_port" in base ? base.dest_port : 80);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const hostChoices = $derived(hostOptions(vaultStore.hosts));

  const fingerprint = () => JSON.stringify([label, autoStart, hostId, kind, bindAddr, bindPort, destHost, destPort]);
  const initial = fingerprint();
  const dirty = $derived(fingerprint() !== initial);
  const hostHasIdentity = $derived(!!vaultStore.effectiveIdentity(vaultStore.hostById.get(hostId)?.data));

  const help: Record<ForwardKind["kind"], string> = {
    local: "Listen on this computer; connections are carried to the destination from the server (ssh -L).",
    remote: "The server listens; connections are carried back and dialled from this computer (ssh -R).",
    dynamic: "A SOCKS5 proxy on this computer that exits through the server (ssh -D).",
  };

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      const rule: ForwardRule =
        kind === "dynamic"
          ? { label, host_id: hostId, auto_start: autoStart, kind, bind_addr: bindAddr, bind_port: bindPort }
          : { label, host_id: hostId, auto_start: autoStart, kind, bind_addr: bindAddr, bind_port: bindPort, dest_host: destHost, dest_port: destPort };
      await vaultStore.saveForward(id, rule, baseRev);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit forwarding rule" : "New forwarding rule"} onclose={() => (ui.modal = null)} confirmClose={dirty ? DISCARD : null}>
  <form id="forward-form" onsubmit={save} class="space-y-4">
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="f-label">Label</label>
        <input id="f-label" class="input" bind:value={label} required placeholder="Postgres tunnel" />
      </div>
      <div>
        <label class="label" for="f-host">Via host</label>
        <Combobox id="f-host" options={hostChoices} bind:value={hostId} placeholder="Search hosts…" emptyText="No host matches" />
      </div>
    </div>
    {#if hostId && !hostHasIdentity}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">
        This host has no identity. Forwarding runs unattended, so attach an identity to the host first.
      </p>
    {/if}

    <div>
      <span class="label">Type</span>
      <div class="flex overflow-hidden rounded-md border border-line text-sm">
        {#each [["local", "Local"], ["remote", "Remote"], ["dynamic", "Dynamic (SOCKS)"]] as [value, text] (value)}
          <button
            type="button"
            class="flex-1 py-1.5 {kind === value ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}"
            onclick={() => (kind = value as ForwardKind["kind"])}>{text}</button
          >
        {/each}
      </div>
      <p class="mt-1.5 text-xs text-fg-muted">{help[kind]}</p>
    </div>

    <div class="grid grid-cols-3 gap-3">
      <div class="col-span-2">
        <label class="label" for="f-baddr">{kind === "remote" ? "Server bind address" : "Local bind address"}</label>
        <input id="f-baddr" class="input font-mono" bind:value={bindAddr} required />
      </div>
      <div>
        <label class="label" for="f-bport">Port</label>
        <input id="f-bport" class="input font-mono" type="number" min="0" max="65535" bind:value={bindPort} required />
      </div>
      {#if kind !== "dynamic"}
        <div class="col-span-2">
          <label class="label" for="f-dhost">{kind === "remote" ? "Local destination host" : "Destination host (from server)"}</label>
          <input id="f-dhost" class="input font-mono" bind:value={destHost} required />
        </div>
        <div>
          <label class="label" for="f-dport">Port</label>
          <input id="f-dport" class="input font-mono" type="number" min="1" max="65535" bind:value={destPort} required />
        </div>
      {/if}
    </div>
    <p class="text-xs text-fg-muted">Port 0 picks a free port automatically.</p>

    <label class="flex items-start gap-2 text-sm">
      <input type="checkbox" class="mt-0.5 accent-input" bind:checked={autoStart} />
      <span>
        Start automatically when the vault is unlocked
        <span class="block text-xs text-fg-muted">Applies on every computer that syncs this vault.</span>
      </span>
    </label>

    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="forward-form" disabled={busy || vaultStore.hosts.length === 0}>
      {busy ? "Saving…" : "Save"}
    </button>
  {/snippet}
</Modal>
