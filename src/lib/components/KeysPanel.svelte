<script lang="ts">
  import { BadgeCheck, ChevronDown, ChevronRight, Copy, Download, FileKey, KeyRound, Lock, Pencil, Plus, Sparkles, Trash2, Upload, Users } from "lucide-svelte";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { withMasterPassword } from "$lib/secrets.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type KeyAlgorithm, type KeyUsage, type SshKey, type Uuid, type VaultRecord } from "$lib/types";

  type Dialog = null | "generate" | "import" | "public";
  let dialog = $state<Dialog>(null);
  let expanded = $state<Uuid | null>(null);
  let usage = $state<Record<Uuid, KeyUsage>>({});
  let busy = $state(false);
  let error = $state<string | null>(null);
  let filter = $state("");

  // Dialog fields.
  let name = $state("");
  let algorithm = $state<KeyAlgorithm>("ed25519");
  let comment = $state("");
  let passphrase = $state("");
  let passphrase2 = $state("");
  let savePassphrase = $state(true);
  let keyText = $state("");
  let keyPath = $state<string | null>(null);
  let publicText = $state("");

  // Per-key edits.
  let renaming = $state<Uuid | null>(null);
  let newName = $state("");
  let certFor = $state<Uuid | null>(null);
  let certText = $state("");
  let ppFor = $state<Uuid | null>(null);
  let ppCurrent = $state("");
  let ppNew = $state("");
  let ppNew2 = $state("");
  let ppSave = $state(true);

  const shown = $derived(
    vaultStore.keys
      .filter((k) => k.data)
      .filter((k) => {
        const q = filter.trim().toLowerCase();
        return !q || [k.data!.name, k.data!.algorithm, k.data!.fingerprint, k.data!.comment].some((s) => s.toLowerCase().includes(q));
      })
      .sort((a, b) => a.data!.name.localeCompare(b.data!.name)),
  );

  function openDialog(d: Dialog) {
    dialog = d;
    error = null;
    name = comment = passphrase = passphrase2 = keyText = publicText = "";
    keyPath = null;
    algorithm = "ed25519";
    savePassphrase = true;
  }

  function closeDialog() {
    dialog = null;
    passphrase = passphrase2 = keyText = "";
  }

  async function run<T>(f: () => Promise<T>): Promise<T | undefined> {
    busy = true;
    error = null;
    try {
      return await f();
    } catch (e) {
      error = errorMessage(e);
      ui.notify("error", error);
    } finally {
      busy = false;
    }
  }

  async function submitGenerate(e: SubmitEvent) {
    e.preventDefault();
    if (passphrase !== passphrase2) return void (error = "The passphrases don't match");
    const rec = await run(() => api.keys.create(name, algorithm, comment || name || "sshvault", passphrase || null, savePassphrase));
    if (rec) {
      vaultStore.putKey(rec);
      expanded = rec.id;
      closeDialog();
      ui.notify("info", `Key "${rec.data?.name}" created. Copy its public key to your servers.`);
    }
  }

  async function chooseFile() {
    const f = await open({ multiple: false, directory: false, title: "Choose a private key file" });
    if (typeof f === "string") keyPath = f;
  }

  async function submitImport(e: SubmitEvent) {
    e.preventDefault();
    const pp = passphrase || null;
    const rec = await run(() =>
      keyPath && !keyText.trim()
        ? api.keys.importPrivateFile(name, keyPath, pp, savePassphrase)
        : api.keys.importPrivate(name, keyText, pp, savePassphrase),
    );
    if (rec) {
      vaultStore.putKey(rec);
      expanded = rec.id;
      closeDialog();
    }
  }

  async function submitPublic(e: SubmitEvent) {
    e.preventDefault();
    const rec = await run(() => api.keys.importPublic(name, publicText));
    if (rec) {
      vaultStore.putKey(rec);
      closeDialog();
    }
  }

  async function toggle(k: VaultRecord<SshKey>) {
    expanded = expanded === k.id ? null : k.id;
    renaming = certFor = ppFor = null;
    if (expanded) {
      try {
        usage[k.id] = await api.keys.usage(k.id);
      } catch {
        // Usage is informational only.
      }
    }
  }

  async function copyPublic(k: SshKey) {
    await writeText(k.public_key);
    ui.notify("info", "Public key copied. Add it to ~/.ssh/authorized_keys on the server.");
  }

  async function rename(k: VaultRecord<SshKey>) {
    const rec = await run(() => api.keys.update(k.id, k.rev, newName, k.data!.certificate ?? null));
    if (rec) {
      vaultStore.putKey(rec);
      renaming = null;
    }
  }

  async function setCertificate(k: VaultRecord<SshKey>, cert: string | null) {
    const rec = await run(() => api.keys.update(k.id, k.rev, k.data!.name, cert));
    if (rec) {
      vaultStore.putKey(rec);
      certFor = null;
      ui.notify("info", cert ? "Certificate saved; it's used automatically when connecting." : "Certificate removed.");
    }
  }

  async function changePassphrase(k: VaultRecord<SshKey>) {
    if (ppNew !== ppNew2) return void (error = "The new passphrases don't match");
    // Empty "current" means "use the passphrase saved in the vault".
    const removing = !ppNew;
    const rec = await run(() => api.keys.changePassphrase(k.id, k.rev, ppCurrent || null, ppNew || null, ppSave));
    if (rec) {
      vaultStore.putKey(rec);
      ppFor = null;
      ppCurrent = ppNew = ppNew2 = "";
      ui.notify("info", removing ? "Passphrase removed; the key is protected by the vault's encryption only." : "Passphrase changed.");
    }
  }

  async function exportPrivate(k: VaultRecord<SshKey>) {
    if (
      !confirm(
        `Export the PRIVATE key "${k.data!.name}" to a file?\n\nAnyone who gets that file can log in wherever this key is trusted${k.data!.encrypted ? " (if they also know its passphrase)" : ""}. Keep it off shared or synced folders and delete it when done.`,
      )
    )
      return;
    const dest = await save({ title: "Save private key as", defaultPath: k.data!.name.replace(/[^\w.-]+/g, "_") || "id_key" });
    if (!dest) return;
    try {
      const done = await withMasterPassword(`Export the private key "${k.data!.name}"`, "Export", (pw) => api.keys.exportPrivate(k.id, pw, dest));
      if (done !== null) ui.notify("info", `Private key written to ${dest} (readable only by you).`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function remove(k: VaultRecord<SshKey>) {
    if (!confirm(`Delete the key "${k.data!.name}" from the vault? Servers that trust it are not changed.`)) return;
    try {
      await vaultStore.deleteKey(k.id, k.rev);
      if (expanded === k.id) expanded = null;
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  const algorithms: { value: KeyAlgorithm; label: string }[] = [
    { value: "ed25519", label: "Ed25519 (recommended)" },
    { value: "ecdsa_p256", label: "ECDSA P-256" },
    { value: "ecdsa_p384", label: "ECDSA P-384" },
    { value: "rsa3072", label: "RSA 3072" },
    { value: "rsa4096", label: "RSA 4096 (slow to generate)" },
  ];
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-3xl space-y-4">
    <div class="flex items-center justify-between">
      <h1 class="flex items-center gap-2 text-lg font-semibold"><KeyRound size={18} class="text-accent" /> Keys</h1>
      <div class="flex gap-2">
        <button class="btn-ghost border border-line" onclick={() => openDialog("public")}><Users size={14} /> Add public key</button>
        <button class="btn-ghost border border-line" onclick={() => openDialog("import")}><Upload size={14} /> Import</button>
        <button class="btn-primary" onclick={() => openDialog("generate")}><Sparkles size={14} /> Generate</button>
      </div>
    </div>
    <p class="text-xs text-fg-muted">
      Private keys are stored encrypted in the vault and never leave it unless you export one. Credentials use a key by
      reference, so replacing or re-encrypting a key here applies everywhere it's used.
    </p>
    {#if vaultStore.keys.length > 6}
      <input class="input py-1.5 text-sm" placeholder="Filter keys…" bind:value={filter} />
    {/if}

    <div class="divide-y divide-line rounded-xl border border-line bg-panel">
      {#each shown as k (k.id)}
        {@const d = k.data!}
        {@const pub = d.private_key === undefined}
        <div>
          <button class="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-panel-hover" onclick={() => toggle(k)}>
            {#if expanded === k.id}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
            <KeyRound size={16} class={pub ? "text-fg-muted" : "text-accent"} />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-1.5 text-sm">
                <span class="truncate">{d.name}</span>
                {#if pub}<span class="rounded bg-fg-muted/15 px-1 text-[9px] font-bold text-fg-muted">PUBLIC ONLY</span>{/if}
                {#if d.encrypted}<span class="rounded bg-success/15 px-1 text-[9px] font-bold text-success" title="The private key is passphrase-protected">PASSPHRASE</span>{/if}
                {#if d.certificate}<span class="rounded bg-accent/15 px-1 text-[9px] font-bold text-accent">CERT</span>{/if}
              </div>
              <div class="truncate font-mono text-[11px] text-fg-muted">{d.algorithm} · {d.fingerprint}</div>
            </div>
          </button>

          {#if expanded === k.id}
            <div class="space-y-3 border-t border-line bg-base/40 px-4 py-3 text-sm">
              <div>
                <div class="mb-1 flex items-center justify-between">
                  <span class="text-xs font-medium">Public key</span>
                  <button class="btn-ghost py-0.5 text-xs" onclick={() => copyPublic(d)}><Copy size={12} /> Copy</button>
                </div>
                <p class="rounded-md border border-line bg-base p-2 font-mono text-[11px] break-all text-fg-muted select-all">{d.public_key}</p>
              </div>

              {#if usage[k.id]}
                {@const u = usage[k.id]}
                <p class="text-xs text-fg-muted">
                  {#if u.identities.length === 0}
                    Not used by any credential.
                  {:else}
                    Used by {u.identities.map((i) => i.label).join(", ")}{u.hosts.length ? ` → ${u.hosts.length} host(s): ${u.hosts.map((h) => h.label).slice(0, 8).join(", ")}${u.hosts.length > 8 ? "…" : ""}` : ""}.
                  {/if}
                </p>
              {/if}

              {#if renaming === k.id}
                <form class="flex gap-2" onsubmit={(e) => (e.preventDefault(), rename(k))}>
                  <input class="input flex-1" bind:value={newName} required aria-label="Key name" />
                  <button class="btn-primary" disabled={busy}>Save</button>
                  <button class="btn-ghost" type="button" onclick={() => (renaming = null)}>Cancel</button>
                </form>
              {/if}

              {#if certFor === k.id}
                <form class="space-y-2" onsubmit={(e) => (e.preventDefault(), setCertificate(k, certText.trim() || null))}>
                  <textarea class="input font-mono text-[11px]" rows="3" bind:value={certText} placeholder="ssh-ed25519-cert-v01@openssh.com AAAA…" spellcheck="false"></textarea>
                  <div class="flex gap-2">
                    <button class="btn-primary" disabled={busy || !certText.trim()}>Save certificate</button>
                    {#if d.certificate}<button class="btn-ghost text-danger" type="button" onclick={() => setCertificate(k, null)}>Remove</button>{/if}
                    <button class="btn-ghost" type="button" onclick={() => (certFor = null)}>Cancel</button>
                  </div>
                </form>
              {/if}

              {#if ppFor === k.id}
                <form class="grid grid-cols-2 gap-2" onsubmit={(e) => (e.preventDefault(), changePassphrase(k))}>
                  {#if d.encrypted}
                    <input class="input col-span-2" type="password" bind:value={ppCurrent} placeholder={d.passphrase !== undefined ? "Current passphrase (saved; leave empty)" : "Current passphrase"} autocomplete="off" />
                  {/if}
                  <input class="input" type="password" bind:value={ppNew} placeholder="New passphrase (empty = none)" autocomplete="off" />
                  <input class="input" type="password" bind:value={ppNew2} placeholder="Confirm" autocomplete="off" />
                  <label class="col-span-2 flex items-center gap-2 text-xs text-fg-muted">
                    <input type="checkbox" class="accent-[#7b61ff]" bind:checked={ppSave} /> Save the passphrase in the vault (no prompt when connecting)
                  </label>
                  <div class="col-span-2 flex gap-2">
                    <button class="btn-primary" disabled={busy}>Change passphrase</button>
                    <button class="btn-ghost" type="button" onclick={() => (ppFor = null)}>Cancel</button>
                  </div>
                </form>
              {/if}

              {#if error && (renaming === k.id || certFor === k.id || ppFor === k.id)}
                <p class="text-xs text-danger">{error}</p>
              {/if}

              <div class="flex flex-wrap gap-1.5">
                <button class="btn-ghost border border-line py-1 text-xs" onclick={() => ((renaming = k.id), (newName = d.name))}><Pencil size={12} /> Rename</button>
                {#if !pub}
                  <button class="btn-ghost border border-line py-1 text-xs" onclick={() => ((certFor = k.id), (certText = d.certificate ?? ""))}><BadgeCheck size={12} /> Certificate</button>
                  <button class="btn-ghost border border-line py-1 text-xs" onclick={() => ((ppFor = k.id), (ppCurrent = ppNew = ppNew2 = ""), (ppSave = d.passphrase !== undefined || !d.encrypted))}><Lock size={12} /> Passphrase</button>
                  <button class="btn-ghost border border-line py-1 text-xs" onclick={() => exportPrivate(k)}><Download size={12} /> Export private key…</button>
                {/if}
                <button class="btn-ghost border border-line py-1 text-xs hover:text-danger" onclick={() => remove(k)}><Trash2 size={12} /> Delete</button>
              </div>
            </div>
          {/if}
        </div>
      {:else}
        <div class="px-4 py-10 text-center">
          <KeyRound size={28} class="mx-auto mb-3 text-fg-muted/50" />
          <p class="text-sm text-fg-muted">{vaultStore.keys.length ? "No matches." : "No keys yet. Generate one or import an existing key."}</p>
        </div>
      {/each}
    </div>
  </div>
</div>

{#if dialog === "generate"}
  <Modal title="Generate a key" onclose={closeDialog}>
    <form id="k-gen" class="space-y-3" onsubmit={submitGenerate}>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="k-name">Name</label>
          <input id="k-name" class="input" bind:value={name} required placeholder="work laptop" />
        </div>
        <div>
          <label class="label" for="k-alg">Type</label>
          <select id="k-alg" class="input" bind:value={algorithm}>
            {#each algorithms as a (a.value)}<option value={a.value}>{a.label}</option>{/each}
          </select>
        </div>
      </div>
      <div>
        <label class="label" for="k-comment">Comment</label>
        <input id="k-comment" class="input" bind:value={comment} placeholder="you@example.com" />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="k-pp">Passphrase (optional)</label>
          <input id="k-pp" class="input" type="password" bind:value={passphrase} autocomplete="new-password" />
        </div>
        <div>
          <label class="label" for="k-pp2">Confirm</label>
          <input id="k-pp2" class="input" type="password" bind:value={passphrase2} autocomplete="new-password" />
        </div>
      </div>
      {#if passphrase}
        <label class="flex items-center gap-2 text-xs text-fg-muted">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={savePassphrase} /> Save the passphrase in the vault
        </label>
      {/if}
      {#if error}<p class="text-sm text-danger">{error}</p>{/if}
    </form>
    {#snippet footer()}
      <button class="btn-ghost" onclick={closeDialog}>Cancel</button>
      <button class="btn-primary" type="submit" form="k-gen" disabled={busy}>{busy ? "Generating…" : "Generate"}</button>
    {/snippet}
  </Modal>
{:else if dialog === "import"}
  <Modal title="Import a private key" onclose={closeDialog}>
    <form id="k-imp" class="space-y-3" onsubmit={submitImport}>
      <div>
        <label class="label" for="k-iname">Name <span class="font-normal text-fg-muted">(defaults to the key's comment)</span></label>
        <input id="k-iname" class="input" bind:value={name} />
      </div>
      <div>
        <div class="mb-1 flex items-end justify-between">
          <label class="label mb-0" for="k-text">Private key (OpenSSH, PEM, PKCS#8 or PuTTY)</label>
          <button type="button" class="btn-ghost py-0.5 text-xs" onclick={chooseFile}><FileKey size={12} /> From file…</button>
        </div>
        {#if keyPath && !keyText}
          <div class="input truncate font-mono text-xs" title={keyPath}>{keyPath}</div>
        {:else}
          <textarea id="k-text" class="input font-mono text-xs" rows="6" bind:value={keyText} placeholder="-----BEGIN OPENSSH PRIVATE KEY-----" spellcheck="false"></textarea>
        {/if}
        <p class="mt-1 text-xs text-fg-muted">The key is copied into the encrypted vault; the original file is left alone.</p>
      </div>
      <div>
        <label class="label" for="k-ipp">Passphrase (if the key has one)</label>
        <input id="k-ipp" class="input" type="password" bind:value={passphrase} autocomplete="off" />
      </div>
      {#if passphrase}
        <label class="flex items-center gap-2 text-xs text-fg-muted">
          <input type="checkbox" class="accent-[#7b61ff]" bind:checked={savePassphrase} /> Save the passphrase in the vault
        </label>
      {/if}
      {#if error}<p class="text-sm text-danger">{error}</p>{/if}
    </form>
    {#snippet footer()}
      <button class="btn-ghost" onclick={closeDialog}>Cancel</button>
      <button class="btn-primary" type="submit" form="k-imp" disabled={busy || (!keyText.trim() && !keyPath)}>{busy ? "Checking…" : "Import"}</button>
    {/snippet}
  </Modal>
{:else if dialog === "public"}
  <Modal title="Add a public key" onclose={closeDialog}>
    <form id="k-pub" class="space-y-3" onsubmit={submitPublic}>
      <div>
        <label class="label" for="k-pname">Name</label>
        <input id="k-pname" class="input" bind:value={name} placeholder="alice's laptop" />
      </div>
      <div>
        <label class="label" for="k-ptext">Public key</label>
        <textarea id="k-ptext" class="input font-mono text-xs" rows="3" bind:value={publicText} required placeholder="ssh-ed25519 AAAA… alice@laptop" spellcheck="false"></textarea>
      </div>
      <p class="text-xs text-fg-muted">Public keys can't log in; keep them here to copy them onto servers.</p>
      {#if error}<p class="text-sm text-danger">{error}</p>{/if}
    </form>
    {#snippet footer()}
      <button class="btn-ghost" onclick={closeDialog}>Cancel</button>
      <button class="btn-primary" type="submit" form="k-pub" disabled={busy}>Add</button>
    {/snippet}
  </Modal>
{/if}
