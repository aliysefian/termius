<script lang="ts">
  import { Bot, Check, Copy, Eye, EyeOff, FileKey, HelpCircle, KeyRound, Lock, Sparkles, Users } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal from "./Modal.svelte";
  import { revealIdentity } from "$lib/secrets.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { emptyHost, errorMessage, type Host, type HostCredentials, type InlineAuth, type Uuid } from "$lib/types";

  let { id, group }: { id: Uuid | null; group?: string } = $props();

  // The form is remounted via {#key} when the id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existing = id ? vaultStore.hostById.get(id)?.data : undefined;
  // svelte-ignore state_referenced_locally
  let form = $state<Host>(existing ? structuredClone($state.snapshot(existing)) : { ...emptyHost(), group: group ?? "" });
  let tags = $state(form.tags.join(", "));
  let error = $state<string | null>(null);
  let busy = $state(false);

  // -- credentials ---------------------------------------------------------

  type Mode = "password" | "key" | "keychain" | "ask";
  type KeySource = "keep" | "generate" | "file" | "paste" | "agent";

  // The identity this host uses now, as listed (secrets redacted).
  const current = existing?.identity_id ? vaultStore.identityById.get(existing.identity_id)?.data : undefined;
  // svelte-ignore state_referenced_locally
  const owned = !!current && !!id && current.for_host === id;

  let mode = $state<Mode>(
    !existing ? "password" : !current ? "ask" : !owned ? "keychain" : current.auth.type === "password" ? "password" : "key",
  );
  let username = $state(owned ? current!.username : "");
  let password = $state("");
  let showPassword = $state(false);
  let keySource = $state<KeySource>(
    owned && current!.auth.type === "private_key" ? "keep" : owned && current!.auth.type === "agent" ? "agent" : "generate",
  );
  let keyText = $state("");
  let keyFile = $state<string | null>(null);
  let passphrase = $state("");
  let identityId = $state<Uuid | "">(!owned && existing?.identity_id ? existing.identity_id : "");
  let saveToKeychain = $state(false);
  let keychainLabel = $state("");

  const hasSavedPassword = owned && current?.auth.type === "password";
  const hasSavedKey = owned && current?.auth.type === "private_key";

  // Keychain choices: shared identities, plus this host's own if it has one.
  const keychainChoices = $derived(vaultStore.identities.filter((i) => !i.data?.for_host || i.data.for_host === id));

  let generatedKey = $state<string | null>(null);
  let copied = $state(false);

  async function revealSaved() {
    if (!existing?.identity_id) return;
    const s = await revealIdentity(existing.identity_id, `Show the saved password for "${form.label}"`);
    if (s?.password != null) {
      password = s.password;
      showPassword = true;
    }
  }

  async function chooseKeyFile() {
    const f = await open({ multiple: false, directory: false, title: "Choose a private key file" });
    if (typeof f === "string") keyFile = f;
  }

  function credentials(): HostCredentials {
    if (mode === "ask") return { mode: "ask" };
    if (mode === "keychain") {
      if (!identityId) throw new Error("Choose an identity from the Keychain");
      return { mode: "identity", identity_id: identityId };
    }
    let auth: InlineAuth;
    if (mode === "password") {
      auth = { type: "password", password: password || null };
    } else {
      switch (keySource) {
        case "keep":
          auth = { type: "private_key", private_key: null, passphrase: passphrase || null };
          break;
        case "generate":
          auth = { type: "generate_key" };
          break;
        case "file":
          if (!keyFile) throw new Error("Choose a key file");
          auth = { type: "key_file", path: keyFile, passphrase: passphrase || null };
          break;
        case "paste":
          auth = { type: "private_key", private_key: keyText || null, passphrase: passphrase || null };
          break;
        case "agent":
          auth = { type: "agent" };
          break;
      }
    }
    return {
      mode: "inline",
      username,
      auth,
      save_to_keychain: saveToKeychain ? keychainLabel.trim() || `${username}@${form.label}` : null,
    };
  }

  // -- jump hosts ------------------------------------------------------------

  // Hosts whose own chain passes through this one would create a loop.
  function reachesThis(startId: Uuid): boolean {
    const seen = new Set<Uuid>();
    let cur: Uuid | undefined = startId;
    while (cur && !seen.has(cur)) {
      if (cur === id) return true;
      seen.add(cur);
      cur = vaultStore.hostById.get(cur)?.data?.jump_host_id;
    }
    return false;
  }
  const jumpCandidates = $derived(vaultStore.hosts.filter((h) => h.id !== id && !(id && reachesThis(h.id))));

  /** "jump2 → jump1 → this host", outermost first. */
  const chainLabel = $derived.by(() => {
    const names: string[] = [];
    const seen = new Set<Uuid>();
    let cur = form.jump_host_id;
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      const h = vaultStore.hostById.get(cur)?.data;
      if (!h) break;
      names.unshift(h.label);
      cur = h.jump_host_id;
    }
    return names.length ? `${names.join(" → ")} → ${form.label || "this host"}` : "";
  });

  const jumpMissingIdentity = $derived.by(() => {
    const seen = new Set<Uuid>();
    let cur = form.jump_host_id;
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      const h = vaultStore.hostById.get(cur)?.data;
      if (!h) return null;
      if (!h.identity_id) return h.label;
      cur = h.jump_host_id;
    }
    return null;
  });

  const colors = ["#7B61FF", "#3DDC84", "#FFB020", "#FF5C5C", "#38BDF8", "#F472B6"];

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      form.tags = tags.split(",").map((t) => t.trim()).filter(Boolean);
      form.jump_host_id = form.jump_host_id || undefined;
      const out = await vaultStore.saveHostWithCredentials(id, $state.snapshot(form), credentials());
      password = keyText = passphrase = "";
      if (out.public_key) generatedKey = out.public_key;
      else ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function copyKey() {
    if (!generatedKey) return;
    await writeText(generatedKey);
    copied = true;
  }

  const modes: { value: Mode; label: string; icon: typeof Lock }[] = [
    { value: "password", label: "Password", icon: Lock },
    { value: "key", label: "SSH key", icon: KeyRound },
    { value: "keychain", label: "Keychain", icon: Users },
    { value: "ask", label: "Ask", icon: HelpCircle },
  ];
  const sources: { value: KeySource; label: string; icon: typeof Lock; show: boolean }[] = [
    { value: "keep", label: "Saved key", icon: Check, show: hasSavedKey },
    { value: "generate", label: "Generate", icon: Sparkles, show: true },
    { value: "file", label: "From file", icon: FileKey, show: true },
    { value: "paste", label: "Paste", icon: KeyRound, show: true },
    { value: "agent", label: "ssh-agent", icon: Bot, show: true },
  ];
</script>

<Modal title={generatedKey ? "Install your new key" : id ? "Edit host" : "New host"} onclose={() => (ui.modal = null)} width="max-w-xl">
  {#if generatedKey}
    <div class="space-y-3 text-sm">
      <p>
        A new Ed25519 key was created and saved for <strong>{form.label}</strong>. Add this public key to
        <code class="rounded bg-base px-1">~/.ssh/authorized_keys</code> on the server, then connect.
      </p>
      <div class="rounded-md border border-line bg-base p-3 font-mono text-xs break-all">{generatedKey}</div>
      <p class="text-xs text-fg-muted">
        For example, log in to the server another way and run:
        <code class="mt-1 block rounded bg-base px-2 py-1 font-mono break-all">echo '{generatedKey}' &gt;&gt; ~/.ssh/authorized_keys</code>
      </p>
    </div>
  {:else}
    <form id="host-form" onsubmit={save} class="space-y-5">
      <!-- Address -->
      <div class="grid grid-cols-6 gap-3">
        <div class="col-span-6">
          <label class="label" for="h-label">Label</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="h-label" class="input" bind:value={form.label} required placeholder="prod-db-01" autofocus={!id} />
        </div>
        <div class="col-span-4">
          <label class="label" for="h-host">Hostname or IP</label>
          <input id="h-host" class="input font-mono" bind:value={form.hostname} required placeholder="10.0.0.5" spellcheck="false" />
        </div>
        <div class="col-span-2">
          <label class="label" for="h-port">Port</label>
          <input id="h-port" class="input font-mono" type="number" min="1" max="65535" bind:value={form.port} required />
        </div>
      </div>

      <!-- Credentials -->
      <div class="rounded-lg border border-line bg-base/40 p-4">
        <div class="mb-3 flex items-center justify-between">
          <span class="text-sm font-semibold">Credentials</span>
        </div>
        <div class="mb-4 grid grid-cols-4 overflow-hidden rounded-md border border-line text-xs" role="tablist">
          {#each modes as m (m.value)}
            <button
              type="button"
              role="tab"
              aria-selected={mode === m.value}
              class="flex items-center justify-center gap-1.5 py-2 {mode === m.value ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}"
              onclick={() => (mode = m.value)}
            >
              <m.icon size={13} />
              {m.label}
            </button>
          {/each}
        </div>

        {#if mode === "password" || mode === "key"}
          <div class="space-y-3">
            <div>
              <label class="label" for="h-user">Username</label>
              <input id="h-user" class="input font-mono" bind:value={username} required placeholder="root" autocomplete="off" spellcheck="false" />
            </div>

            {#if mode === "password"}
              <div>
                <div class="mb-1 flex items-end justify-between">
                  <label class="label mb-0" for="h-pw">Password</label>
                  {#if hasSavedPassword && !password}
                    <button type="button" class="btn-ghost py-0.5 text-xs" onclick={revealSaved}><Eye size={12} /> Reveal saved</button>
                  {/if}
                </div>
                <div class="relative">
                  <input
                    id="h-pw"
                    class="input pr-10"
                    type={showPassword ? "text" : "password"}
                    bind:value={password}
                    autocomplete="new-password"
                    required={!hasSavedPassword}
                    placeholder={hasSavedPassword ? "Saved. Leave empty to keep it." : ""}
                  />
                  <button
                    type="button"
                    class="absolute right-2 top-1/2 -translate-y-1/2 text-fg-muted hover:text-fg"
                    onclick={() => (showPassword = !showPassword)}
                    aria-label={showPassword ? "Hide password" : "Show password"}
                  >
                    {#if showPassword}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
                  </button>
                </div>
              </div>
            {:else}
              <div>
                <span class="label">Key</span>
                <div class="flex flex-wrap gap-1.5">
                  {#each sources.filter((s) => s.show) as s (s.value)}
                    <button
                      type="button"
                      class="flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-xs {keySource === s.value ? 'border-accent bg-accent/15 text-fg' : 'border-line text-fg-muted hover:bg-panel-hover'}"
                      onclick={() => (keySource = s.value)}
                    >
                      <s.icon size={12} />
                      {s.label}
                    </button>
                  {/each}
                </div>
              </div>

              {#if keySource === "keep"}
                <p class="text-xs text-fg-muted">The saved private key stays as it is.</p>
              {:else if keySource === "generate"}
                <p class="text-xs text-fg-muted">
                  A new Ed25519 key is created when you save. You'll get its public key to add to the server.
                </p>
              {:else if keySource === "file"}
                <div class="flex items-center gap-2">
                  <div class="input flex-1 truncate font-mono text-xs {keyFile ? '' : 'text-fg-muted/60'}" title={keyFile ?? ""}>
                    {keyFile ?? "No file chosen, for example ~/.ssh/id_ed25519"}
                  </div>
                  <button type="button" class="btn-ghost border border-line" onclick={chooseKeyFile}><FileKey size={14} /> Choose…</button>
                </div>
                <p class="text-xs text-fg-muted">The key is copied into the encrypted vault, so it syncs to your other computers.</p>
              {:else if keySource === "paste"}
                <textarea
                  class="input font-mono text-xs"
                  rows="5"
                  bind:value={keyText}
                  placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                  spellcheck="false"
                  required
                ></textarea>
              {:else}
                <p class="text-xs text-fg-muted">Keys come from the ssh-agent running on each computer.</p>
              {/if}

              {#if keySource === "keep" || keySource === "file" || keySource === "paste"}
                <div>
                  <label class="label" for="h-pass">Key passphrase {keySource === "keep" ? "(leave empty to keep)" : "(if any)"}</label>
                  <input id="h-pass" class="input" type="password" bind:value={passphrase} autocomplete="off" />
                </div>
              {/if}
            {/if}

            <label class="flex items-center gap-2 text-xs text-fg-muted">
              <input type="checkbox" class="accent-[#7b61ff]" bind:checked={saveToKeychain} />
              Also save to Keychain so other hosts can use these credentials
            </label>
            {#if saveToKeychain}
              <input class="input text-sm" bind:value={keychainLabel} placeholder={`${username || "user"}@${form.label || "host"}`} aria-label="Keychain label" />
            {/if}
          </div>
        {:else if mode === "keychain"}
          {#if keychainChoices.length === 0}
            <p class="text-sm text-fg-muted">
              The Keychain is empty. Choose <strong>Password</strong> or <strong>SSH key</strong> above, and tick "Also save to
              Keychain" to add one.
            </p>
          {:else}
            <label class="label" for="h-identity">Identity</label>
            <select id="h-identity" class="input" bind:value={identityId} required>
              <option value="" disabled>Choose an identity…</option>
              {#each keychainChoices as ident (ident.id)}
                {@const d = ident.data!}
                <option value={ident.id}>
                  {d.label} · {d.username} · {d.auth.type === "password" ? "password" : d.auth.type === "private_key" ? "key" : "agent"}
                </option>
              {/each}
            </select>
          {/if}
        {:else}
          <p class="text-sm text-fg-muted">Nothing is saved. You'll be asked for a username and password each time you connect.</p>
        {/if}
      </div>

      <!-- Organise -->
      <div class="grid grid-cols-2 gap-3">
        <div class="col-span-2">
          <label class="label" for="h-jump">Jump host</label>
          <select id="h-jump" class="input" bind:value={form.jump_host_id}>
            <option value={undefined}>None, connect directly</option>
            {#each jumpCandidates as h (h.id)}
              <option value={h.id}>{h.data?.label} ({h.data?.hostname})</option>
            {/each}
          </select>
          {#if chainLabel}
            <p class="mt-1 text-xs text-fg-muted">Route: {chainLabel}</p>
          {/if}
          {#if jumpMissingIdentity}
            <p class="mt-1 text-xs text-danger">"{jumpMissingIdentity}" has no saved credentials. Jump hosts need them to connect.</p>
          {/if}
        </div>
        <label class="col-span-2 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-[#7b61ff]" bind:checked={form.forward_agent} />
          <span>
            Forward ssh-agent
            <span class="block text-xs text-fg-muted">
              Lets this host use your local keys, for example to reach another server or git. Anyone with root on the
              host can use them while you're connected, so only enable it for hosts you trust.
            </span>
          </span>
        </label>
        <label class="col-span-2 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-[#7b61ff]" bind:checked={form.forward_x11} />
          <span>
            Forward X11
            <span class="block text-xs text-fg-muted">
              Show the host's graphical programs on this computer. Needs an X server here (built into most Linux
              desktops, XQuartz on macOS, VcXsrv or X410 on Windows) and <code>xauth</code> on the host.
            </span>
          </span>
        </label>
        <div>
          <label class="label" for="h-group">Group</label>
          <input id="h-group" class="input" bind:value={form.group} placeholder="Production/Databases" />
        </div>
        <div>
          <label class="label" for="h-tags">Tags</label>
          <input id="h-tags" class="input" bind:value={tags} placeholder="db, eu-west" />
        </div>
        <div class="col-span-2">
          <span class="label">Color</span>
          <div class="flex gap-2">
            {#each colors as c (c)}
              <button
                type="button"
                class="h-6 w-6 rounded-full ring-offset-2 ring-offset-panel {form.color === c ? 'ring-2 ring-fg' : ''}"
                style:background={c}
                onclick={() => (form.color = c)}
                aria-label="Color {c}"
              ></button>
            {/each}
          </div>
        </div>
        <div class="col-span-2">
          <label class="label" for="h-notes">Notes</label>
          <textarea id="h-notes" class="input" rows="2" bind:value={form.notes}></textarea>
        </div>
      </div>

      {#if error}
        <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
      {/if}
    </form>
  {/if}
  {#snippet footer()}
    {#if generatedKey}
      <button class="btn-ghost border border-line" onclick={copyKey}>
        {#if copied}<Check size={14} /> Copied{:else}<Copy size={14} /> Copy public key{/if}
      </button>
      <button class="btn-primary" onclick={() => (ui.modal = null)}>Done</button>
    {:else}
      <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-primary" type="submit" form="host-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
    {/if}
  {/snippet}
</Modal>
