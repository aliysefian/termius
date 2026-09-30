<script lang="ts">
  import { Zap } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import { parseAdhoc, type AdhocHop } from "$lib/ssh";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  let { initial = "" }: { initial?: string } = $props();

  // svelte-ignore state_referenced_locally
  let address = $state(initial);
  let password = $state("");
  const parsed = $derived(parseAdhoc(address, (n, p, u) => vaultStore.findHop(n, p, u)));
  const hopName = (h: AdhocHop) =>
    "host_id" in h ? (vaultStore.hostById.get(h.host_id)?.data?.label ?? "saved host") : `${h.username}@${h.hostname}${h.port !== 22 ? `:${h.port}` : ""} (ssh-agent)`;
  /** "bastion → gw (ssh-agent) → user@host". */
  const route = $derived(parsed?.jumps?.length ? [...parsed.jumps.map(hopName), `${parsed.username}@${parsed.hostname}`].join(" → ") : "");
  /** "telnet://host[:port]" or "telnet host [port]". */
  const telnet = $derived.by(() => {
    const m = /^telnet(?::\/\/|\s+)(\[[^\]]+\]|[^\s:/]+)(?:[:\s](\d{1,5}))?\/?$/i.exec(address.trim());
    if (!m) return null;
    const port = m[2] ? Number(m[2]) : 23;
    return port > 0 && port < 65536 ? { host: m[1].replace(/^\[|\]$/g, ""), port } : null;
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (telnet) {
      ui.modal = null;
      ui.openTelnet(telnet.host, telnet.port);
      return;
    }
    if (!parsed) return;
    ui.modal = null;
    ui.openAdhoc({ ...parsed, password: password || undefined });
    password = "";
  }
</script>

<Modal title="Quick connect" onclose={() => (ui.modal = null)} width="max-w-md">
  <form id="quick-form" onsubmit={submit} class="space-y-4">
    <div>
      <label class="label" for="q-addr">Address</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input id="q-addr" class="input font-mono" placeholder="user@host:22 or ssh -J bastion user@host" bind:value={address} autofocus required spellcheck="false" />
      {#if telnet}
        <p class="mt-1 text-xs text-warning">Telnet to {telnet.host}:{telnet.port}. Everything, including passwords, is sent unencrypted.</p>
      {:else if route}
        <p class="mt-1 text-xs text-fg-muted">Route: {route}</p>
      {:else if address && !parsed}
        <p class="mt-1 text-xs text-danger">
          Use user@host[:port], ssh -J jump user@host, or telnet://host[:port]. A jump host without a user must be a saved host.
        </p>
      {/if}
    </div>
    <div class={telnet ? "hidden" : ""}>
      <label class="label" for="q-pw">Password</label>
      <input id="q-pw" class="input" type="password" bind:value={password} autocomplete="off" placeholder="Leave empty to use ssh-agent" />
      {#if route}<p class="mt-1 text-xs text-fg-muted">For the last host only. Jump hosts use their saved credentials or ssh-agent.</p>{/if}
    </div>
    <p class="text-xs text-fg-muted">Nothing is saved. To keep this server, add it as a host instead.</p>
  </form>
  {#snippet footer()}
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-ghost" type="button" onclick={() => (ui.modal = { kind: "serial" })}>Serial console…</button>
    <button class="btn-primary" type="submit" form="quick-form" disabled={!parsed && !telnet}><Zap size={14} /> Connect</button>
  {/snippet}
</Modal>
