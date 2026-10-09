<script lang="ts">
  import { CheckCircle2, Loader2 } from "lucide-svelte";
  import Combobox from "./Combobox.svelte";
  import Modal, { DISCARD } from "./Modal.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { cleanOptions, engineInfo } from "$lib/dbengines";
  import { DB_ENGINES, ENVIRONMENTS, emptyDbConnection, errorMessage, type DbConnection, type DbTls, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();

  // Remounted via {#key} when id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existingRec = id ? vaultStore.dbConnections.find((c) => c.id === id) : undefined;
  const baseRev = existingRec?.rev ?? null;
  const base: DbConnection = existingRec?.data ? structuredClone($state.snapshot(existingRec.data)) : emptyDbConnection();
  /** A password is stored when the list says so (it arrives blanked). */
  const hasStoredPassword = base.password === "";

  let name = $state(base.name);
  let engine = $state(base.engine);
  let host = $state(base.host);
  let port = $state(base.port);
  let username = $state(base.username);
  let password = $state("");
  let database = $state(base.database);
  let tls = $state<DbTls>(base.tls);
  let sshHostId = $state(base.ssh_host_id ?? "");
  let environment = $state(base.environment ?? "");
  let group = $state(base.group);
  let notes = $state(base.notes);
  /** Settings only this engine has (see dbengines.ts). */
  let options = $state<Record<string, string>>({ ...(base.options ?? {}) });
  const info = $derived(engineInfo(engine));

  let error = $state<string | null>(null);
  let busy = $state(false);
  let testing = $state(false);
  let testResult = $state<string | null>(null);

  const hostChoices = $derived(hostOptions(vaultStore.hosts));
  const sshHost = $derived(sshHostId ? vaultStore.hostById.get(sshHostId) : undefined);
  const sshHostHasIdentity = $derived(!!vaultStore.effectiveIdentity(sshHost?.data));

  const fingerprint = () => JSON.stringify([name, engine, host, port, username, password, database, tls, sshHostId, environment, group, notes, options]);
  const initial = fingerprint();
  const dirty = $derived(fingerprint() !== initial);

  function changeEngine(value: string) {
    const was = DB_ENGINES.find((e) => e.value === engine);
    engine = value;
    // Another engine's settings mean nothing here.
    options = {};
    // Follow the default port only if the user hasn't typed their own.
    if (port === was?.port) port = DB_ENGINES.find((e) => e.value === value)?.port ?? port;
  }

  function build(): DbConnection {
    return {
      name: name.trim(),
      engine,
      host: host.trim(),
      port,
      username,
      // Blank keeps the stored one (on a new connection, no password).
      password: password === "" ? (hasStoredPassword ? "" : undefined) : password,
      database: info.database ? database.trim() : "",
      tls,
      ssh_host_id: sshHostId || undefined,
      group: group.trim(),
      environment: environment || undefined,
      notes,
      options: cleanOptions(engine, $state.snapshot(options)),
    };
  }

  async function test() {
    testResult = null;
    error = null;
    testing = true;
    try {
      testResult = await api.db.test(id, build());
    } catch (err) {
      error = errorMessage(err);
    } finally {
      testing = false;
    }
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      await vaultStore.saveDbConnection(id, build(), baseRev);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit database connection" : "New database connection"} onclose={() => (ui.modal = null)} confirmClose={dirty ? DISCARD : null} width="max-w-xl">
  <form id="db-form" onsubmit={save} class="space-y-4">
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="db-name">Name</label>
        <input id="db-name" class="input" bind:value={name} required placeholder="Orders (production)" />
      </div>
      <div>
        <label class="label" for="db-engine">Type</label>
        <select id="db-engine" class="input" value={engine} onchange={(e) => changeEngine(e.currentTarget.value)}>
          {#each DB_ENGINES as d (d.value)}<option value={d.value}>{d.label}</option>{/each}
        </select>
      </div>
      <div>
        <label class="label" for="db-host">Server</label>
        <input id="db-host" class="input font-mono" bind:value={host} required placeholder="db.internal" autocomplete="off" spellcheck="false" />
      </div>
      <div>
        <label class="label" for="db-port">Port</label>
        <input id="db-port" class="input font-mono" type="number" min="1" max="65535" bind:value={port} required />
      </div>
      <div>
        <label class="label" for="db-user">{info.user?.label ?? "User"}{#if !info.user?.required}<span class="ml-1 font-normal text-fg-muted">(optional)</span>{/if}</label>
        <input id="db-user" class="input" bind:value={username} autocomplete="off" spellcheck="false" required={info.user?.required} />
      </div>
      <div>
        <label class="label" for="db-pass">{info.password.label}</label>
        <input
          id="db-pass"
          class="input"
          type="password"
          bind:value={password}
          autocomplete="new-password"
          placeholder={hasStoredPassword ? "Saved. Leave blank to keep it" : ""}
        />
      </div>
      {#if info.database}
        <div class="col-span-2">
          <label class="label" for="db-db">{info.database.label} <span class="font-normal text-fg-muted">{info.database.help ?? ""}</span></label>
          <input id="db-db" class="input font-mono" bind:value={database} autocomplete="off" spellcheck="false" placeholder={info.database.placeholder ?? ""} />
        </div>
      {/if}
      {#each info.options.filter((o) => !o.onlyWhenVerifying || tls === "verify_full") as o (o.key)}
        <div class="col-span-2" data-testid="db-option-{o.key}">
          <label class="label" for="db-opt-{o.key}">{o.label}</label>
          {#if o.choices}
            <select id="db-opt-{o.key}" class="input" value={options[o.key] ?? o.choices[0].value} onchange={(e) => (options[o.key] = e.currentTarget.value)}>
              {#each o.choices as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
            </select>
          {:else}
            <input id="db-opt-{o.key}" class="input font-mono" value={options[o.key] ?? ""} oninput={(e) => (options[o.key] = e.currentTarget.value)} placeholder={o.placeholder ?? ""} autocomplete="off" spellcheck="false" />
          {/if}
          {#if o.help}<p class="mt-1 text-xs text-fg-muted">{o.help}</p>{/if}
        </div>
      {/each}
      {#if info.note}
        <p class="col-span-2 text-xs text-fg-muted" data-testid="db-engine-note">{info.note}</p>
      {/if}
    </div>

    <div class="grid grid-cols-2 gap-3">
      <div class="col-span-2">
        <label class="label" for="db-ssh">Connect through SSH host <span class="font-normal text-fg-muted">(optional)</span></label>
        <Combobox id="db-ssh" options={hostChoices} bind:value={sshHostId} clearable placeholder="Direct connection" emptyText="No host matches" />
        {#if sshHostId}
          <p class="mt-1 text-xs text-fg-muted">The server address above is then as seen from that host (often <code>127.0.0.1</code>). The database port is never opened on this computer for anything else to use.</p>
          {#if !sshHost}
            <p class="mt-1 text-xs text-danger">That host no longer exists. Pick another.</p>
          {:else if !sshHostHasIdentity}
            <p class="mt-1 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">This host has no identity. Connecting runs unattended, so attach an identity to the host first.</p>
          {/if}
        {/if}
      </div>
      <div>
        <label class="label" for="db-tls">Encryption</label>
        <select id="db-tls" class="input" bind:value={tls}>
          <option value="verify_full">TLS, verify the certificate</option>
          <option value="require">TLS, don't verify the certificate</option>
          <option value="disable">None</option>
        </select>
      </div>
      <div>
        <label class="label" for="db-env">Environment</label>
        <select id="db-env" class="input" bind:value={environment}>
          {#each ENVIRONMENTS as e (e.value)}<option value={e.value}>{e.label}</option>{/each}
        </select>
      </div>
    </div>
    {#if tls === "disable" && !sshHostId}
      <p class="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">UNENCRYPTED: the password and every row cross the network in clear text. Use TLS, or connect through an SSH host.</p>
    {:else if tls === "require"}
      <p class="text-xs text-fg-muted">The connection is encrypted, but anyone on the path could impersonate the server.</p>
    {/if}
    {#if environment === "production"}
      <p class="text-xs text-fg-muted">On production, destructive statements and row edits ask you to type this connection's name first.</p>
    {/if}

    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="db-group">Group <span class="font-normal text-fg-muted">(optional)</span></label>
        <input id="db-group" class="input" bind:value={group} placeholder="Shop" />
      </div>
      <div class="col-span-2">
        <label class="label" for="db-notes">Notes</label>
        <textarea id="db-notes" class="input min-h-16" bind:value={notes}></textarea>
      </div>
    </div>

    {#if testResult}
      <p class="flex items-center gap-2 rounded-md border border-success/30 bg-success/10 px-3 py-2 text-sm text-success"><CheckCircle2 size={15} /> Connected. Server version {testResult}</p>
    {/if}
    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger" role="alert">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-secondary mr-auto" type="button" onclick={test} disabled={testing || busy || !host.trim()}>
      {#if testing}<Loader2 size={14} class="animate-spin" /> Testing…{:else}Test connection{/if}
    </button>
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="db-form" disabled={busy || testing}>{busy ? "Saving…" : "Save"}</button>
  {/snippet}
</Modal>
