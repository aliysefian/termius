<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import { groupOptions, hostOptions } from "$lib/pickeroptions";
  import { Bot, Check, Copy, Eye, EyeOff, FileKey, HelpCircle, KeyRound, Lock, Sparkles, Users } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal, { DISCARD } from "./Modal.svelte";
  import { revealIdentity } from "$lib/secrets.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { ENVIRONMENTS, emptyHost, errorMessage, type Host, type HostCredentials, type InlineAuth, type Uuid } from "$lib/types";

  let { id, group }: { id: Uuid | null; group?: string } = $props();

  // The form is remounted via {#key} when the id changes, so capturing the initial value is intended.
  // svelte-ignore state_referenced_locally
  const existing = id ? vaultStore.hostById.get(id)?.data : undefined;
  // The revision being edited: a change from another device meanwhile is merged or reported, never overwritten.
  // svelte-ignore state_referenced_locally
  const baseRev = id ? (vaultStore.hostById.get(id)?.rev ?? null) : null;
  // svelte-ignore state_referenced_locally
  let form = $state<Host>(existing ? structuredClone($state.snapshot(existing)) : { ...emptyHost(), group: group ?? "" });
  let tags = $state(form.tags.join(", "));
  const knownEnv = (v: string | undefined) => ENVIRONMENTS.some((e) => e.value === (v ?? ""));
  let envChoice = $state(knownEnv(form.environment) ? (form.environment ?? "") : "custom");
  let customEnv = $state(knownEnv(form.environment) ? "" : (form.environment ?? ""));
  let customFields = $state(
    Object.entries(form.custom ?? {})
      .map(([k, v]) => `${k}=${v}`)
      .join("\n"),
  );
  let error = $state<string | null>(null);
  let busy = $state(false);

  // -- credentials ---------------------------------------------------------

  type Mode = "password" | "key" | "keychain" | "ask";
  type KeySource = "keep" | "manager" | "generate" | "file" | "paste" | "agent";

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
    owned && current!.auth.type === "private_key"
      ? "keep"
      : owned && current!.auth.type === "agent"
        ? "agent"
        : owned && current!.auth.type === "key"
          ? "manager"
          : vaultStore.keys.some((k) => k.data?.private_key !== undefined)
            ? "manager"
            : "generate",
  );
  let managerKeyId = $state<Uuid | "">(owned && current!.auth.type === "key" ? current!.auth.key_id : "");
  const managerKeys = $derived(vaultStore.keys.filter((k) => k.data && k.data.private_key !== undefined));
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

  // Everything the user can edit, as one string: closing with changes asks first.
  const fingerprint = () =>
    JSON.stringify([
      $state.snapshot(form), tags, envChoice, customEnv, customFields, mode, username, password, keySource, managerKeyId,
      keyText, keyFile, passphrase, identityId, saveToKeychain, keychainLabel,
    ]);
  const initial = fingerprint();
  const dirty = $derived(fingerprint() !== initial);

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
    // Telnet logs in inside the terminal; nothing is stored for it.
    if (mode === "ask" || form.protocol === "telnet") return { mode: "ask" };
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
        case "manager":
          if (!managerKeyId) throw new Error("Choose a key from the Key Manager");
          auth = { type: "key", key_id: managerKeyId };
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
  /** What the host's groups would give it, shown so an empty field never hides a jump or proxy. */
  const groupJump = $derived.by(() => {
    const j = vaultStore.groupDefault(form, "default_jump_host_id") as Uuid | undefined;
    return j && j !== id ? j : undefined;
  });
  const groupJumpLabel = $derived(groupJump ? (vaultStore.hostById.get(groupJump)?.data?.label ?? "(missing)") : "");
  const groupProxy = $derived(vaultStore.groupDefault(form, "proxy_id") as Uuid | undefined);
  const groupProxyName = $derived(groupProxy ? (vaultStore.proxyById.get(groupProxy)?.data?.name ?? "(missing)") : "");
  const DIRECT = "direct";
  const jumpChoices = $derived([
    ...(groupJump ? [{ value: DIRECT, label: "None, connect directly", detail: `ignore the group's ${groupJumpLabel}` }] : []),
    ...hostOptions(vaultStore.hosts, (h) => h.id !== id && !(id && reachesThis(h.id))),
  ]);
  const jumpValue = () => form.jump_host_id ?? (form.no_group_jump && groupJump ? DIRECT : "");
  const setJump = (v: string) => {
    form.no_group_jump = v === DIRECT;
    form.jump_host_id = v && v !== DIRECT ? v : undefined;
  };
  const proxyValue = () => form.proxy_id ?? (form.no_group_proxy && groupProxy ? DIRECT : "");
  const setProxy = (v: string) => {
    form.no_group_proxy = v === DIRECT;
    form.proxy_id = v && v !== DIRECT ? v : undefined;
  };
  /** The jump in effect: the host's own, else its group's unless opted out. */
  const firstJump = $derived(form.jump_host_id ?? (form.no_group_jump ? undefined : groupJump));
  const groupChoices = $derived(groupOptions(vaultStore.hosts, vaultStore.groups));

  /** "jump2 → jump1 → this host", outermost first. */
  const chainLabel = $derived.by(() => {
    const names: string[] = [];
    const seen = new Set<Uuid>(id ? [id] : []);
    let cur = firstJump;
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      const h = vaultStore.hostById.get(cur)?.data;
      if (!h) break;
      names.unshift(h.label);
      cur = vaultStore.effectiveJump(h, cur);
    }
    return names.length ? `${names.join(" → ")} → ${form.label || "this host"}` : "";
  });

  const jumpMissingIdentity = $derived.by(() => {
    const seen = new Set<Uuid>(id ? [id] : []);
    let cur = firstJump;
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      const h = vaultStore.hostById.get(cur)?.data;
      if (!h) return null;
      if (!vaultStore.effectiveIdentity(h)) return h.label;
      cur = vaultStore.effectiveJump(h, cur);
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
      form.environment = form.environment || undefined;
      form.startup_command = form.startup_command?.trim() || undefined;
      form.environment = envChoice === "custom" ? customEnv.trim() || undefined : envChoice || undefined;
      form.proxy_id = form.proxy_id || undefined;
      form.no_group_jump = (!form.jump_host_id && form.no_group_jump) || undefined;
      form.no_group_proxy = (!form.proxy_id && form.no_group_proxy) || undefined;
      form.keepalive_secs = form.keepalive_secs == null || (form.keepalive_secs as unknown) === "" ? undefined : Number(form.keepalive_secs);
      const custom: Record<string, string> = {};
      for (const line of customFields.split("\n")) {
        const at = line.indexOf("=");
        if (at > 0) custom[line.slice(0, at).trim()] = line.slice(at + 1).trim();
      }
      form.custom = Object.keys(custom).length ? custom : undefined;
      const out = await vaultStore.saveHostWithCredentials(id, $state.snapshot(form), credentials(), baseRev);
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
    { value: "keychain", label: "Credential", icon: Users },
    { value: "ask", label: "Ask", icon: HelpCircle },
  ];
  const sources: { value: KeySource; label: string; icon: typeof Lock; show: boolean }[] = [
    { value: "keep", label: "Saved key", icon: Check, show: hasSavedKey },
    { value: "manager", label: "Key Manager", icon: KeyRound, show: true },
    { value: "generate", label: "Generate", icon: Sparkles, show: true },
    { value: "file", label: "From file", icon: FileKey, show: true },
    { value: "paste", label: "Paste", icon: KeyRound, show: true },
    { value: "agent", label: "ssh-agent", icon: Bot, show: true },
  ];
</script>

<Modal title={generatedKey ? "Install your new key" : id ? "Edit host" : "New host"} onclose={() => (ui.modal = null)} width="max-w-xl" confirmClose={dirty && !generatedKey ? DISCARD : null}>
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
        <div class="col-span-6">
          <label class="label" for="h-proto">Protocol</label>
          <select
            id="h-proto"
            class="input"
            value={form.protocol === "telnet" ? "telnet" : "ssh"}
            onchange={(e) => {
              const telnet = e.currentTarget.value === "telnet";
              form.protocol = telnet ? "telnet" : undefined;
              if (telnet && form.port === 22) form.port = 23;
              else if (!telnet && form.port === 23) form.port = 22;
            }}
          >
            <option value="ssh">SSH</option>
            <option value="telnet">Telnet (unencrypted, for network gear)</option>
          </select>
          {#if form.protocol === "telnet"}
            <p class="mt-1 text-xs text-warning">
              Telnet sends everything, including passwords, in clear text. You log in inside the terminal; nothing is saved
              for it. Credentials, jump hosts, proxies and SFTP don't apply.
            </p>
          {/if}
        </div>
      </div>

      {#if form.protocol !== "telnet"}
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
              {:else if keySource === "manager"}
                {#if managerKeys.length === 0}
                  <p class="text-xs text-fg-muted">The Key Manager has no private keys yet. Add one under <strong>Keys</strong>.</p>
                {:else}
                  <select class="input" bind:value={managerKeyId} required aria-label="Key">
                    <option value="" disabled>Choose a key…</option>
                    {#each managerKeys as k (k.id)}
                      <option value={k.id}>{k.data!.name} · {k.data!.algorithm} · {k.data!.fingerprint.slice(7, 19)}…</option>
                    {/each}
                  </select>
                {/if}
              {:else if keySource === "generate"}
                <p class="text-xs text-fg-muted">
                  A new Ed25519 key is created when you save. You'll get its public key to add to the server.
                </p>
              {:else if keySource === "file"}
                <div class="flex items-center gap-2">
                  <div class="input flex-1 truncate font-mono text-xs {keyFile ? '' : 'text-fg-muted/60'}" title={keyFile ?? ""}>
                    {keyFile ?? "No file chosen, for example ~/.ssh/id_ed25519"}
                  </div>
                  <button type="button" class="btn-secondary" onclick={chooseKeyFile}><FileKey size={14} /> Choose…</button>
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
              <input type="checkbox" class="accent-input" bind:checked={saveToKeychain} />
              Also save to Credentials so other hosts can use them
            </label>
            {#if saveToKeychain}
              <input class="input text-sm" bind:value={keychainLabel} placeholder={`${username || "user"}@${form.label || "host"}`} aria-label="Keychain label" />
            {/if}
          </div>
        {:else if mode === "keychain"}
          {#if keychainChoices.length === 0}
            <p class="text-sm text-fg-muted">
              The Keychain is empty. Choose <strong>Password</strong> or <strong>SSH key</strong> above, and tick "Also save to
              Credentials" to add one.
            </p>
          {:else}
            <label class="label" for="h-identity">Identity</label>
            <select id="h-identity" class="input" bind:value={identityId} required>
              <option value="" disabled>Choose an identity…</option>
              {#each keychainChoices as ident (ident.id)}
                {@const d = ident.data!}
                <option value={ident.id}>
                  {d.label} · {d.username} · {d.auth.type === "password" ? "password" : d.auth.type === "agent" ? "agent" : "key"}
                </option>
              {/each}
            </select>
          {/if}
        {:else}
          <p class="text-sm text-fg-muted">Nothing is saved. You'll be asked for a username and password each time you connect.</p>
        {/if}
      </div>
      {/if}

      <!-- Organise -->
      <div class="grid grid-cols-2 gap-3">
        <div class="col-span-2">
          <label class="label" for="h-jump">Jump host</label>
          <Combobox
            id="h-jump"
            options={jumpChoices}
            bind:value={jumpValue, setJump}
            clearable
            placeholder={groupJump && !form.no_group_jump ? `Group default: ${groupJumpLabel}` : "None, connect directly"}
            emptyText="No host matches"
          />
          {#if chainLabel}
            <p class="mt-1 text-xs text-fg-muted">
              Route: {chainLabel}{!form.jump_host_id ? ` (jump host from group "${form.group}"; pick "None, connect directly" to skip it)` : ""}
            </p>
          {/if}
          {#if jumpMissingIdentity}
            <p class="mt-1 text-xs text-danger">"{jumpMissingIdentity}" has no saved credentials. Jump hosts need them to connect.</p>
          {/if}
        </div>
        <label class="col-span-2 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" bind:checked={form.forward_agent} />
          <span>
            Forward ssh-agent
            <span class="block text-xs text-fg-muted">
              Lets this host use your local keys, for example to reach another server or git. Anyone with root on the
              host can use them while you're connected, so only enable it for hosts you trust.
            </span>
          </span>
        </label>
        <div>
          <label class="label" for="h-env">Environment</label>
          <select id="h-env" class="input" bind:value={envChoice}>
            {#each ENVIRONMENTS as e (e.value)}
              <option value={e.value}>{e.label}</option>
            {/each}
            <option value="custom">Custom…</option>
          </select>
          {#if envChoice === "custom"}
            <input class="input mt-1.5" bind:value={customEnv} placeholder="qa, dr, customer-x" aria-label="Custom environment" />
          {/if}
          {#if envChoice === "production"}
            <p class="mt-1 text-xs text-danger">Marked in red, and bulk actions ask before touching it.</p>
          {/if}
        </div>
        <div>
          <label class="label" for="h-startup">Run after connecting</label>
          <input
            id="h-startup"
            class="input font-mono text-xs"
            bind:value={form.startup_command}
            placeholder="sudo -i, cd /srv/app, tmux attach"
            spellcheck="false"
          />
        </div>
        <label class="col-span-2 flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-0.5 accent-input" bind:checked={form.forward_x11} />
          <span>
            Forward X11
            <span class="block text-xs text-fg-muted">
              Show the host's graphical programs on this computer. Needs an X server here (built into most Linux
              desktops, XQuartz on macOS, VcXsrv or X410 on Windows) and <code>xauth</code> on the host.
            </span>
          </span>
        </label>
        <div>
          <label class="label" for="h-proxy">Proxy</label>
          <select id="h-proxy" class="input" bind:value={proxyValue, setProxy}>
            <option value="">{groupProxy ? `Group default: ${groupProxyName}` : "None, connect directly"}</option>
            {#if groupProxy}<option value={DIRECT}>None, ignore the group's proxy</option>{/if}
            {#each vaultStore.proxies as p (p.id)}
              <option value={p.id}>{p.data?.name}{p.data?.spec.kind === "command" && !p.data.spec.approved ? " (not approved)" : ""}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="label" for="h-keepalive">Keep-alive (seconds)</label>
          <input id="h-keepalive" class="input font-mono" type="number" min="0" max="3600" bind:value={form.keepalive_secs} placeholder="30 (0 = off)" />
        </div>
        <label class="col-span-2 flex items-center gap-2 text-sm">
          <input type="checkbox" class="accent-input" bind:checked={form.favorite} />
          Favorite
        </label>
        <div>
          <label class="label" for="h-group">Group</label>
          <Combobox
            id="h-group"
            options={groupChoices}
            bind:value={form.group}
            creatable
            clearable
            createLabel={(q) => `New group “${q}”`}
            placeholder="No group (top level)"
            emptyText="No groups yet. Type a name to create one."
          />
          <p class="mt-1 text-[11px] text-fg-muted">Search or type a new name. Use / to nest, e.g. Production/Databases.</p>
        </div>
        <div>
          <label class="label" for="h-tags">Tags</label>
          <input id="h-tags" class="input" bind:value={tags} placeholder="db, eu-west" />
        </div>
        <div class="col-span-2">
          <span class="label">Colour</span>
          <div class="flex gap-2">
            {#each colors as c (c)}
              <button
                type="button"
                class="h-6 w-6 rounded-full ring-offset-2 ring-offset-panel {form.color === c ? 'ring-2 ring-fg' : ''}"
                style:background={c}
                onclick={() => (form.color = c)}
                aria-label="Colour {c}"
              ></button>
            {/each}
          </div>
        </div>
        <div class="col-span-2">
          <label class="label" for="h-notes">Notes</label>
          <textarea id="h-notes" class="input" rows="2" bind:value={form.notes}></textarea>
        </div>
        <div class="col-span-2">
          <label class="label" for="h-custom">Custom fields <span class="font-normal text-fg-muted">(one <code>key=value</code> per line)</span></label>
          <textarea id="h-custom" class="input font-mono text-xs" rows="2" bind:value={customFields} placeholder="owner=platform-team" spellcheck="false"></textarea>
        </div>
      </div>

      {#if error}
        <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
      {/if}
    </form>
  {/if}
  {#snippet footer()}
    {#if generatedKey}
      <button class="btn-secondary" onclick={copyKey}>
        {#if copied}<Check size={14} /> Copied{:else}<Copy size={14} /> Copy public key{/if}
      </button>
      <button class="btn-primary" onclick={() => (ui.modal = null)}>Done</button>
    {:else}
      <button class="btn-ghost" type="button" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-primary" type="submit" form="host-form" disabled={busy}>{busy ? "Saving…" : "Save"}</button>
    {/if}
  {/snippet}
</Modal>
