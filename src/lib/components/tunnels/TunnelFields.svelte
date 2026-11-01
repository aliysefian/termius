<script lang="ts">
  // The fields of a tunnel, shared by the Create dialog and the Configuration tab. Only the fields that apply to the
  // chosen type are shown. The checks here are the same the backend makes; it has the last word.
  import Combobox from "../Combobox.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import type { Draft, FieldErrors } from "$lib/tunnels";

  let { draft = $bindable(), errors, idPrefix, showType = true }: { draft: Draft; errors: FieldErrors; idPrefix: string; showType?: boolean } = $props();

  const hostChoices = $derived(hostOptions(vaultStore.hosts));
  const hostHasIdentity = $derived(!!vaultStore.effectiveIdentity(vaultStore.hostById.get(draft.host_id)?.data));
  const remote = $derived(draft.kind === "remote");
  const id = (s: string) => `${idPrefix}-${s}`;
  const invalid = (e?: string) => (e ? "true" : undefined);
</script>

<div class="space-y-4">
  <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
    <div>
      <label class="label" for={id("name")}>Name</label>
      <input id={id("name")} class="input" bind:value={draft.label} placeholder="Postgres tunnel" aria-invalid={invalid(errors.label)} aria-describedby={errors.label ? id("name-err") : undefined} />
      {#if errors.label}<p id={id("name-err")} class="mt-1 text-xs text-danger" role="alert">{errors.label}</p>{/if}
    </div>
    <div>
      <label class="label" for={id("host")}>SSH host</label>
      <Combobox id={id("host")} options={hostChoices} bind:value={draft.host_id} placeholder="Search hosts…" emptyText="No host matches" />
      {#if errors.host_id}<p class="mt-1 text-xs text-danger" role="alert">{errors.host_id}</p>{/if}
    </div>
  </div>
  {#if draft.host_id && !hostHasIdentity}
    <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">This host has no saved credential. A tunnel runs unattended, so attach an identity to the host before starting it.</p>
  {/if}

  <div>
    <label class="label" for={id("desc")}>Description <span class="font-normal text-fg-muted">(optional)</span></label>
    <input id={id("desc")} class="input" bind:value={draft.description} placeholder="What this tunnel is for" />
  </div>

  {#if showType}
    <fieldset>
      <legend class="label">Type</legend>
      <div class="flex overflow-hidden rounded-lg border border-line text-sm" role="radiogroup" aria-label="Tunnel type">
        {#each [["local", "Local"], ["remote", "Remote"], ["dynamic", "Dynamic (SOCKS)"]] as [value, text] (value)}
          <button type="button" role="radio" aria-checked={draft.kind === value} class="flex-1 py-1.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent {draft.kind === value ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}" onclick={() => (draft.kind = value as Draft["kind"])}>{text}</button>
        {/each}
      </div>
    </fieldset>
  {/if}

  <div class="grid grid-cols-3 gap-3">
    <div class="col-span-2">
      <label class="label" for={id("baddr")}>{remote ? "Bind address on the host" : "Local bind address"}</label>
      <input id={id("baddr")} class="input font-mono" bind:value={draft.bind_addr} placeholder={remote ? "empty = all interfaces" : "127.0.0.1"} spellcheck="false" autocomplete="off" aria-invalid={invalid(errors.bind_addr)} />
      {#if errors.bind_addr}<p class="mt-1 text-xs text-danger" role="alert">{errors.bind_addr}</p>{/if}
    </div>
    <div>
      <label class="label" for={id("bport")}>Port</label>
      <input id={id("bport")} class="input font-mono tabular-nums" type="number" min="0" max="65535" bind:value={draft.bind_port} aria-invalid={invalid(errors.bind_port)} />
    </div>
    {#if errors.bind_port}<p class="col-span-3 -mt-2 text-xs text-danger" role="alert">{errors.bind_port}</p>{/if}
    {#if draft.kind !== "dynamic"}
      <div class="col-span-2">
        <label class="label" for={id("dhost")}>{remote ? "Local destination host" : "Destination host"}</label>
        <input id={id("dhost")} class="input font-mono" title={remote ? "Reached from this computer" : "Reached from the SSH host, so 127.0.0.1 means the host itself"} bind:value={draft.dest_host} spellcheck="false" autocomplete="off" aria-invalid={invalid(errors.dest_host)} />
        {#if errors.dest_host}<p class="mt-1 text-xs text-danger" role="alert">{errors.dest_host}</p>{/if}
      </div>
      <div>
        <label class="label" for={id("dport")}>Port</label>
        <input id={id("dport")} class="input font-mono tabular-nums" type="number" min="1" max="65535" bind:value={draft.dest_port} aria-invalid={invalid(errors.dest_port)} />
      </div>
      {#if errors.dest_port}<p class="col-span-3 -mt-2 text-xs text-danger" role="alert">{errors.dest_port}</p>{/if}
    {/if}
  </div>
  <p class="text-xs text-fg-muted">Port 0 picks a free port each time the tunnel starts.</p>

  <label class="flex items-start gap-2.5 text-sm">
    <input type="checkbox" class="mt-0.5 accent-input" bind:checked={draft.auto_start} />
    <span>
      Start when the vault is unlocked
      <span class="block text-xs text-fg-muted">Applies on every computer that syncs this vault.</span>
    </span>
  </label>
</div>
