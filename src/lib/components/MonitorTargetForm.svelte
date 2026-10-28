<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import Modal, { DISCARD } from "./Modal.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import { checkTarget, emptyTarget, hostOf, parsePorts, splitList, type CheckSpec, type MonitorTarget } from "$lib/monitoring";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { id }: { id: Uuid | null } = $props();

  // Remounted via {#key} when id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existingRec = id ? monitoring.targets.find((t) => t.id === id) : undefined;
  const baseRev = existingRec?.rev ?? null;
  const base: MonitorTarget = existingRec?.data ? structuredClone($state.snapshot(existingRec.data)) : emptyTarget();
  const ping = base.checks.find((c): c is Extract<CheckSpec, { kind: "ping" }> => c.kind === "ping");

  let label = $state(base.label);
  let address = $state(base.address);
  let tagsText = $state(base.tags.join(", "));
  let interval = $state(base.interval_secs);
  let enabled = $state(base.enabled);
  let usePing = $state(!!ping);
  let pingCount = $state(ping?.count ?? 4);
  let portsText = $state(base.checks.flatMap((c) => (c.kind === "tcp" ? [String(c.port)] : [])).join(", "));
  const webChecks = base.checks.filter((c): c is Extract<CheckSpec, { kind: "http" }> => c.kind === "http");
  let webText = $state(webChecks.map((c) => c.url).join("\n"));
  let webExpect = $state(webChecks[0]?.expect_status ? String(webChecks[0].expect_status) : "");
  let webContains = $state(webChecks[0]?.contains ?? "");
  let useSsh = $state(base.checks.some((c) => c.kind === "ssh_metrics"));
  let sshHostId = $state(base.ssh_host_id ?? "");
  let notes = $state(base.notes);
  let copyFrom = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  const hostChoices = $derived(hostOptions(vaultStore.hosts));
  const fingerprint = () => JSON.stringify([label, address, tagsText, interval, enabled, usePing, pingCount, portsText, webText, webExpect, webContains, useSsh, sshHostId, notes]);
  const initial = fingerprint();
  const dirty = $derived(fingerprint() !== initial);

  /** Fill in the form from a host that is already saved. */
  function fillFrom(hostId: string) {
    const h = vaultStore.hostById.get(hostId)?.data;
    if (!h) return;
    label = h.label;
    address = h.hostname;
    tagsText = (h.tags ?? []).join(", ");
    portsText = String(h.port || 22);
    sshHostId = hostId;
    useSsh = !!vaultStore.effectiveIdentity(h);
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    const ports = parsePorts(portsText);
    if (ports === null) {
      error = "Ports are numbers from 1 to 65535, separated by commas.";
      return;
    }
    const checks: CheckSpec[] = [];
    if (usePing) checks.push({ kind: "ping", count: Number(pingCount), timeout_ms: ping?.timeout_ms ?? 1000 });
    for (const port of ports) checks.push({ kind: "tcp", port });
    const urls = webText.split("\n").map((u) => u.trim()).filter(Boolean);
    const expect = webExpect.trim() === "" ? 0 : Number(webExpect);
    for (const url of urls) checks.push({ kind: "http", url, expect_status: expect, timeout_ms: webChecks[0]?.timeout_ms ?? 10000, contains: webContains.trim() ? webContains : null });
    if (useSsh) checks.push({ kind: "ssh_metrics" });
    const target: MonitorTarget = {
      label: label.trim(),
      // A site on its own needs no separate address: take it from the first web address.
      address: address.trim() || hostOf(urls[0] ?? ""),
      tags: splitList(tagsText),
      enabled,
      interval_secs: Number(interval),
      checks,
      ssh_host_id: useSsh ? sshHostId || null : null,
      notes,
    };
    const problem = checkTarget(target);
    if (problem) {
      error = problem;
      return;
    }
    busy = true;
    try {
      await monitoring.saveTarget(id, baseRev, target);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={id ? "Edit watched host" : "Watch a host"} onclose={() => (ui.modal = null)} confirmClose={dirty ? DISCARD : null} width="max-w-xl">
  <form id="monitor-form" onsubmit={save} class="space-y-4">
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="m-label">Name</label>
        <input id="m-label" class="input" bind:value={label} required placeholder="web-01" />
      </div>
      <div>
        <label class="label" for="m-address">Address</label>
        <input id="m-address" class="input font-mono" bind:value={address} placeholder="10.0.0.5 or web.example.com (or leave empty for a website)" spellcheck="false" />
      </div>
      <div>
        <label class="label" for="m-tags">Tags</label>
        <input id="m-tags" class="input" bind:value={tagsText} placeholder="prod, web" />
      </div>
      <div>
        <label class="label" for="m-interval">Check every (seconds)</label>
        <input id="m-interval" class="input font-mono" type="number" min="10" max="86400" bind:value={interval} required />
      </div>
    </div>

    {#if !id && hostChoices.length}
      <div>
        <label class="label" for="m-copy">Fill in from a saved host (optional)</label>
        <Combobox id="m-copy" options={hostChoices} bind:value={copyFrom} onchange={fillFrom} placeholder="Search hosts…" emptyText="No host matches" />
      </div>
    {/if}

    <fieldset class="space-y-3 rounded-lg border border-line p-3">
      <legend class="px-1 text-xs font-medium text-fg-muted">Checks</legend>
      <label class="flex items-center gap-2 text-sm">
        <input type="checkbox" class="accent-input" bind:checked={usePing} />
        <span>Ping: is it up, how many packets are lost, how long they take</span>
        {#if usePing}<input class="input w-16 py-1 font-mono text-xs" type="number" min="1" max="10" bind:value={pingCount} aria-label="Packets per check" title="Packets per check" />{/if}
      </label>
      <div>
        <label class="label" for="m-ports">TCP ports that should be open</label>
        <input id="m-ports" class="input font-mono" bind:value={portsText} placeholder="22, 443" spellcheck="false" />
      </div>
      <div>
        <label class="label" for="m-web">Web pages (one address per line)</label>
        <textarea id="m-web" class="input min-h-14 font-mono text-xs" bind:value={webText} placeholder="https://example.com/&#10;https://example.com/health" spellcheck="false"></textarea>
        {#if webText.trim()}
          <div class="mt-2 grid grid-cols-2 gap-3">
            <div>
              <label class="label" for="m-web-status">Expected status</label>
              <input id="m-web-status" class="input font-mono text-xs" type="number" min="100" max="599" bind:value={webExpect} placeholder="any 2xx or 3xx" />
            </div>
            <div>
              <label class="label" for="m-web-text">Page must contain</label>
              <input id="m-web-text" class="input text-xs" bind:value={webContains} placeholder="optional text" />
            </div>
          </div>
          <p class="mt-1 text-xs text-fg-muted">Checks the status, how fast the page starts to answer, and for https how many days the certificate has left. Redirects are not followed. Up to 5 addresses; keep secrets out of them.</p>
        {/if}
      </div>
      <label class="flex items-start gap-2 text-sm">
        <input type="checkbox" class="mt-0.5 accent-input" bind:checked={useSsh} />
        <span>
          CPU, memory and disk use, read over SSH
          <span class="block text-xs text-fg-muted">Logs in with the saved host's credentials, unattended. A host key that isn't trusted yet is refused: connect to the host once first.</span>
        </span>
      </label>
      {#if useSsh}
        <div>
          <label class="label" for="m-ssh">Log in with this saved host</label>
          <Combobox id="m-ssh" options={hostChoices} bind:value={sshHostId} placeholder="Search hosts…" emptyText="No host matches" />
          {#if sshHostId && !vaultStore.effectiveIdentity(vaultStore.hostById.get(sshHostId)?.data)}
            <p class="mt-1 text-xs text-warning">This host has no saved credentials, so its readings will show as unknown.</p>
          {/if}
        </div>
      {/if}
    </fieldset>

    <div>
      <label class="label" for="m-notes">Notes</label>
      <textarea id="m-notes" class="input min-h-16" bind:value={notes}></textarea>
    </div>

    <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" bind:checked={enabled} /> Watch this host</label>

    {#if error}
      <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="monitor-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
  {/snippet}
</Modal>
