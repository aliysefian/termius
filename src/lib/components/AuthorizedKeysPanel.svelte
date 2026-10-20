<script lang="ts">
  import { KeyRound, Loader2, RefreshCw, RotateCw, ShieldAlert, Trash2 } from "lucide-svelte";
  import { checkPublicKeyLine, isHardwareKey, keyRisk, type AuthorizedKey } from "$lib/security/authkeys";
  import type { AuthorizedKeysManager } from "$lib/security/useAuthorizedKeysManager.svelte";

  let { mgr }: { mgr: AuthorizedKeysManager } = $props();

  let pickedVault = $state("");
  let pasted = $state("");

  const typeTone = (k: AuthorizedKey) => {
    if (isHardwareKey(k.type)) return "bg-accent/10 text-accent";
    const risk = keyRisk(k);
    if (risk !== "ok") return "bg-warning/10 text-warning";
    return k.type === "ssh-ed25519" ? "bg-success/10 text-success" : "bg-fg-muted/10 text-fg-muted";
  };
  const typeLabel = (k: AuthorizedKey) => `${k.type.replace("@openssh.com", "")}${k.bits ? ` ${k.bits}` : ""}`;
  const optionTone = (o: string) => (o.startsWith("command=") || o.startsWith("from=") ? "text-accent" : "text-fg-muted");

  async function addVault() {
    const k = mgr.vaultKeys.find((x) => x.id === pickedVault);
    if (k && (await mgr.addFromVault(k))) pickedVault = "";
  }
  async function addPasted() {
    if (await mgr.add(pasted)) pasted = "";
  }
</script>

<div class="space-y-3">
  <div class="flex flex-wrap items-end gap-3 rounded-lg border border-line bg-panel/60 p-3 text-xs">
    <div>
      <label class="label" for="ak-who">File</label>
      <select id="ak-who" class="input py-1 text-xs" bind:value={mgr.who} onchange={() => void mgr.refresh()}>
        <option value="me">~/.ssh/authorized_keys</option>
        <option value="root">/root/.ssh/authorized_keys (sudo)</option>
      </select>
    </div>
    <div class="min-w-48 flex-1">
      <label class="label" for="ak-vault">Add key from Vault</label>
      <div class="flex gap-2">
        <select id="ak-vault" class="input py-1 text-xs" bind:value={pickedVault}>
          <option value="">Choose a key…</option>
          {#each mgr.vaultKeys as k (k.id)}<option value={k.id}>{k.data?.name} · {k.data?.algorithm}</option>{/each}
        </select>
        <button class="btn-primary px-3 py-1 text-xs" disabled={!pickedVault || !!mgr.busy} onclick={addVault}><KeyRound size={12} /> Add</button>
      </div>
    </div>
    <div class="min-w-64 flex-1">
      <label class="label" for="ak-paste">Or paste a public key</label>
      <div class="flex gap-2">
        <input id="ak-paste" class="input py-1 font-mono text-xs" bind:value={pasted} placeholder="ssh-ed25519 AAAA… comment" />
        <button class="btn-secondary px-3 py-1 text-xs" disabled={!pasted.trim() || checkPublicKeyLine(pasted) !== null || !!mgr.busy} onclick={addPasted}>Add</button>
      </div>
    </div>
    <button class="btn-secondary px-2 py-1 text-xs" disabled={mgr.loading} onclick={() => void mgr.refresh()}>
      {#if mgr.loading}<Loader2 size={12} class="animate-spin" />{:else}<RefreshCw size={12} />{/if} Reload
    </button>
  </div>
  {#if mgr.error}<p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{mgr.error}</p>{/if}

  <div class="overflow-x-auto rounded-lg border border-line">
    <table class="w-full text-left text-xs">
      <thead class="bg-panel/60 text-fg-muted"><tr class="[&>th]:px-3 [&>th]:py-2 [&>th]:font-medium"><th>Type</th><th>Comment / Identity</th><th>SHA256 fingerprint</th><th>Options</th><th class="w-24"></th></tr></thead>
      <tbody class="divide-y divide-line/80">
        {#each mgr.keys as k (k.line)}
          {@const risk = keyRisk(k)}
          <tr class="group align-top transition-colors hover:bg-panel-hover/50 [&>td]:px-3 [&>td]:py-2">
            <td class="whitespace-nowrap">
              <span class="rounded-sm px-1.5 py-0.5 font-mono tabular-nums {typeTone(k)}">{typeLabel(k)}</span>
              {#if risk !== "ok"}<span class="ml-1 inline-flex items-center gap-1 text-warning" title={risk === "legacy" ? "DSA keys are obsolete" : "RSA under 3072 bits is deprecated"}><ShieldAlert size={12} /></span>{/if}
            </td>
            <td>
              {k.comment || "—"}
              {#if mgr.isActive(k)}<span class="ml-1.5 rounded-sm bg-accent/10 px-1 text-[10px] text-accent" title="This is the key this host's saved login uses">in use here</span>{/if}
            </td>
            <td class="font-mono tabular-nums text-fg-muted">{k.fingerprint}</td>
            <td>
              <div class="flex flex-wrap gap-1">
                {#each k.options as o (o)}<span class="max-w-64 truncate rounded-sm border border-line px-1.5 py-0.5 font-mono {optionTone(o)}" title={o}>{o}</span>{:else}<span class="text-fg-muted">—</span>{/each}
              </div>
            </td>
            <td class="whitespace-nowrap text-right">
              <span class="opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                <button class="icon-btn h-6 w-6" title="Rotate key" aria-label="Rotate key" disabled={!!mgr.busy || mgr.who === "root"} onclick={() => void mgr.rotate(k)}><RotateCw size={13} /></button>
                <button class="icon-btn h-6 w-6 hover:text-danger" title="Revoke / Remove" aria-label="Revoke key" disabled={!!mgr.busy} onclick={() => void mgr.revoke(k)}><Trash2 size={13} /></button>
              </span>
            </td>
          </tr>
        {:else}
          <tr><td class="px-3 py-6 text-center text-fg-muted" colspan="5">{mgr.loading ? "Reading the file…" : mgr.loaded ? "No keys in this file." : "Not loaded yet."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
  {#if mgr.busy}<p class="flex items-center gap-2 px-1 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> {mgr.busy}…</p>{/if}
</div>
