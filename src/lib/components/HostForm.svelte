<script lang="ts">
  import * as api from "$lib/api";
  import { RDP_SIZES, formatFingerprint } from "$lib/rdp";
  import Combobox from "./Combobox.svelte";
  import { groupOptions, hostOptions } from "$lib/pickeroptions";
  import { Bot, Check, Copy, Eye, EyeOff, FileKey, HelpCircle, KeyRound, Loader2, Lock, Sparkles, Users, Wifi, X } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Modal, { DISCARD } from "./Modal.svelte";
  import { revealIdentity } from "$lib/secrets.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { ENVIRONMENTS, emptyFtp, emptyHost, emptyRdp, errorMessage, type FtpTls, type Host, type HostCredentials, type InlineAuth, type Uuid } from "$lib/types";

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

  // -- tabs -------------------------------------------------------------------

  type Tab = "connection" | "route" | "organise" | "automation";
  const TABS: { id: Tab; label: string }[] = [
    { id: "connection", label: "Connection" },
    { id: "route", label: "Route" },
    { id: "organise", label: "Organise" },
    { id: "automation", label: "Automation" },
  ];
  let activeTab = $state<Tab>("connection");

  // Problems worth a glance before switching tabs: shown under the tab strip
  // regardless of which one is open, in addition to where the field itself is.
  const warnings = $derived([jumpMissingIdentity ? `"${jumpMissingIdentity}" has no saved credentials.` : null].filter((w): w is string => !!w));

  // -- test connection ---------------------------------------------------------
  // Reads the saved host record, so it needs this host saved already; testing
  // unsaved address/port changes would need a new backend command this
  // environment has no way to compile (no webkit/gtk dev libs here).
  let testing = $state(false);
  let moshInstalled = $state<boolean | null>(null);
  api.mosh.available().then((ok) => (moshInstalled = ok), () => (moshInstalled = null));

  async function testConnection() {
    if (!id) return;
    testing = true;
    try {
      await vaultStore.checkHealth([id]);
    } finally {
      testing = false;
    }
  }
  const health = $derived(id ? vaultStore.health[id] : undefined);

  // -- tunnels that start automatically with this host --------------------
  const hostForwards = $derived(id ? vaultStore.forwards.filter((f) => f.data?.host_id === id) : []);

  async function toggleAutoStart(f: (typeof hostForwards)[number]) {
    if (!f.data) return;
    await vaultStore.saveForward(f.id, { ...f.data, auto_start: !f.data.auto_start }, f.rev);
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    busy = true;
    try {
      form.tags = tags.split(",").map((t) => t.trim()).filter(Boolean);
      form.jump_host_id = form.jump_host_id || undefined;
      form.environment = form.environment || undefined;
      form.startup_command = form.startup_command?.trim() || undefined;
      if (form.protocol === "rdp") {
        form.rdp = { ...(form.rdp ?? emptyRdp()), domain: form.rdp?.domain?.trim() || undefined };
      } else {
        form.rdp = undefined;
      }
      form.mosh = form.protocol === "rdp" || form.protocol === "ftp" ? undefined : form.mosh;
      if (form.protocol === "rdp" || form.protocol === "telnet" || form.protocol === "ftp") form.file_protocol = undefined;
      form.ftp = form.protocol === "ftp" ? (form.ftp ?? emptyFtp()) : undefined;
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
      activeTab = "connection";
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
      <div class="-mb-1 flex overflow-hidden rounded-md border border-line text-xs" role="tablist">
        {#each TABS as t (t.id)}
          <button
            type="button"
            role="tab"
            aria-selected={activeTab === t.id}
            class="flex-1 py-2 {activeTab === t.id ? 'bg-accent text-white' : 'text-fg-muted hover:bg-panel-hover'}"
            onclick={() => (activeTab = t.id)}
          >
            {t.label}
          </button>
        {/each}
      </div>

      {#if error}
        <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
      {/if}
      {#each warnings as w (w)}
        <p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning">{w}</p>
      {/each}

      {#if activeTab === "connection"}
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
            value={form.protocol === "telnet" ? "telnet" : form.protocol === "rdp" ? "rdp" : form.protocol === "ftp" ? "ftp" : "ssh"}
            onchange={(e) => {
              const v = e.currentTarget.value;
              form.protocol = v === "ssh" ? undefined : v;
              // Move the port only if it is still the old protocol's default.
              const defaults: Record<string, number> = { ssh: 22, telnet: 23, rdp: 3389, ftp: 21 };
              if (Object.values(defaults).includes(form.port)) form.port = defaults[v];
              if (v === "rdp") form.rdp ??= emptyRdp();
              if (v === "ftp") form.ftp ??= emptyFtp();
            }}
          >
            <option value="ssh">SSH</option>
            <option value="rdp">Remote Desktop (RDP)</option>
            <option value="ftp">FTP / FTPS (files only)</option>
            <option value="telnet">Telnet (unencrypted, for network gear)</option>
          </select>
          {#if form.protocol !== "telnet" && form.protocol !== "rdp" && form.protocol !== "ftp"}
            <div class="mt-2 flex items-center gap-2 text-xs">
              <label class="shrink-0 font-medium" for="h-files">Files</label>
              <select id="h-files" class="input w-auto py-1 text-xs" value={form.file_protocol ?? ""} onchange={(e) => (form.file_protocol = e.currentTarget.value === "scp" ? "scp" : undefined)}>
                <option value="">SFTP (default)</option>
                <option value="scp">SCP (the server has no SFTP)</option>
              </select>
            </div>
            <label class="mt-2 flex items-start gap-2 text-xs">
              <input type="checkbox" class="mt-0.5" checked={!!form.mosh} onchange={(e) => (form.mosh = e.currentTarget.checked || undefined)} />
              <span>
                <span class="font-medium">Use Mosh</span>
                <span class="block text-fg-muted">
                  Keeps the session alive across network changes and sleep. Needs <code>mosh-server</code> on the host and
                  <code>mosh-client</code> on this computer, and UDP ports 60000–61000 open to the host.
                  {#if moshInstalled === false}<span class="text-warning">mosh-client wasn't found on this computer.</span>{/if}
                </span>
              </span>
            </label>
          {/if}
          {#if form.protocol === "ftp" && form.ftp}
            <div class="mt-3 grid grid-cols-2 gap-3 rounded-lg border border-line bg-base/40 p-3">
              <div>
                <label class="label" for="h-ftp-tls">Encryption</label>
                <select
                  id="h-ftp-tls"
                  class="input text-xs"
                  value={form.ftp.tls}
                  onchange={(e) => {
                    const v = e.currentTarget.value as FtpTls;
                    if (!form.ftp) return;
                    // Implicit FTPS conventionally lives on 990; follow the port only while it is still a default.
                    if (v === "implicit" && form.port === 21) form.port = 990;
                    else if (v !== "implicit" && form.port === 990) form.port = 21;
                    form.ftp.tls = v;
                  }}
                >
                  <option value="explicit">Explicit TLS (FTPES, recommended)</option>
                  <option value="implicit">Implicit TLS (FTPS, port 990)</option>
                  <option value="none">None (UNENCRYPTED)</option>
                </select>
              </div>
              <label class="flex items-center gap-2 self-end pb-2 text-xs">
                <input type="checkbox" class="accent-input" checked={form.ftp.anonymous} onchange={(e) => form.ftp && (form.ftp.anonymous = e.currentTarget.checked)} />
                <span>Anonymous (no user name or password)</span>
              </label>
              {#if form.ftp.tls === "none"}
                <p class="col-span-2 text-xs text-warning">
                  Plain FTP sends everything, including your password and the files, readable by anyone on the path between you and the server.
                  Use it only on a network you trust.
                </p>
              {/if}
              <div class="col-span-2 text-xs text-fg-muted">
                {#if form.ftp.tls !== "none"}
                  {#if form.ftp.cert_sha256}
                    <div>Server certificate trusted: <span class="font-mono break-all">{formatFingerprint(form.ftp.cert_sha256)}</span></div>
                    <button type="button" class="mt-1 text-accent hover:underline" onclick={() => form.ftp && (form.ftp.cert_sha256 = undefined)}>Forget it (you'll be asked again next time)</button>
                  {:else}
                    The server's certificate is shown the first time you connect, and trusted from then on. A different one later is refused until you say otherwise.
                  {/if}
                {/if}
                <div class="mt-1">Credentials below must be a user name and password (unless anonymous). Jump hosts and proxies aren't used for FTP. Open it from the Files view or by double-clicking the host.</div>
              </div>
            </div>
          {/if}
          {#if form.protocol === "rdp" && form.rdp}
            <div class="mt-3 grid grid-cols-2 gap-3 rounded-lg border border-line bg-base/40 p-3">
              <div>
                <label class="label" for="h-rdp-domain">Domain <span class="font-normal text-fg-muted">(optional)</span></label>
                <input id="h-rdp-domain" class="input font-mono text-xs" bind:value={form.rdp.domain} placeholder="CORP" spellcheck="false" />
              </div>
              <div>
                <label class="label" for="h-rdp-size">Screen size</label>
                <select
                  id="h-rdp-size"
                  class="input text-xs"
                  value={`${form.rdp.width}x${form.rdp.height}`}
                  onchange={(e) => {
                    const [w, h] = e.currentTarget.value.split("x").map(Number);
                    if (form.rdp) [form.rdp.width, form.rdp.height] = [w, h];
                  }}
                >
                  {#each RDP_SIZES as s (s.label)}<option value={`${s.width}x${s.height}`}>{s.label}</option>{/each}
                </select>
              </div>
              <div>
                <label class="label" for="h-rdp-depth">Colours</label>
                <select id="h-rdp-depth" class="input text-xs" value={String(form.rdp.color_depth)} onchange={(e) => form.rdp && (form.rdp.color_depth = e.currentTarget.value === "16" ? 16 : 32)}>
                  <option value="32">True colour (32-bit)</option>
                  <option value="16">High colour (16-bit, less data)</option>
                </select>
              </div>
              <div>
                <label class="label" for="h-rdp-sec">Sign-in security</label>
                <select id="h-rdp-sec" class="input text-xs" bind:value={form.rdp.security}>
                  <option value="auto">Automatic (recommended)</option>
                  <option value="nla">Require NLA</option>
                  <option value="tls">TLS only</option>
                </select>
              </div>
              <div class="col-span-2 text-xs text-fg-muted">
                {#if form.rdp.cert_sha256}
                  <div>Server certificate trusted: <span class="font-mono break-all">{formatFingerprint(form.rdp.cert_sha256)}</span></div>
                  <button type="button" class="mt-1 text-accent hover:underline" onclick={() => form.rdp && (form.rdp.cert_sha256 = undefined)}>Forget it (you'll be asked again next time)</button>
                {:else}
                  The server's certificate is shown the first time you connect, and trusted from then on. A different one later is refused until you say otherwise.
                {/if}
                <div class="mt-1">Credentials below must be a user name and password. Jump hosts and proxies aren't used for Remote Desktop.</div>
              </div>
            </div>
          {/if}
          {#if form.protocol === "telnet"}
            <p class="mt-1 text-xs text-warning">
              Telnet sends everything, including passwords, in clear text. You log in inside the terminal; nothing is saved
              for it. Credentials, jump hosts, proxies and SFTP don't apply.
            </p>
          {/if}
        </div>
      </div>

      <div class="flex items-center gap-2">
        <button type="button" class="btn-secondary py-1 text-xs" disabled={!id || testing} title={id ? undefined : "Save the host first"} onclick={testConnection}>
          {#if testing}<Loader2 size={13} class="animate-spin" /> Testing…{:else}<Wifi size={13} /> Test connection{/if}
        </button>
        {#if !id}
          <span class="text-xs text-fg-muted">Available once the host is saved.</span>
        {:else if health}
          <span class="text-xs {health.state === 'up' ? 'text-success' : health.state === 'down' ? 'text-danger' : 'text-fg-muted'}">
            {#if health.state === "up"}Reachable in {health.latency_ms} ms{#if health.banner} · {health.banner}{/if}
            {:else if health.state === "down"}Unreachable: {health.reason}
            {:else}Behind a jump host; the jump itself wasn't probed{/if}
          </span>
        {/if}
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

      {/if}

      {#if activeTab === "route"}
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
      </div>
      {/if}

      {#if activeTab === "organise"}
      <div class="grid grid-cols-2 gap-3">
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
        <label class="flex items-end gap-2 pb-2 text-sm">
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
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="flex h-6 w-6 items-center justify-center rounded-full border border-line {!form.color ? 'ring-2 ring-fg' : ''}"
              onclick={() => (form.color = undefined)}
              title="No colour"
              aria-label="No colour"
            >
              {#if !form.color}<X size={12} class="text-fg-muted" />{/if}
            </button>
            {#each colors as c (c)}
              <button
                type="button"
                class="h-6 w-6 rounded-full ring-offset-2 ring-offset-panel {form.color === c ? 'ring-2 ring-fg' : ''}"
                style:background={c}
                onclick={() => (form.color = c)}
                aria-label="Colour {c}"
              ></button>
            {/each}
            <input
              type="color"
              class="h-6 w-6 shrink-0 cursor-pointer rounded-full border border-line bg-base p-0"
              value={form.color ?? "#7b61ff"}
              oninput={(e) => (form.color = e.currentTarget.value)}
              aria-label="Custom colour"
              title="Custom colour"
            />
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
      {/if}

      {#if activeTab === "automation"}
      <div class="space-y-4">
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
        <div>
          <span class="label">Tunnels that start when the vault unlocks</span>
          {#if !id}
            <p class="text-xs text-fg-muted">Save the host first to add tunnels for it, under <strong>Tunnels</strong>.</p>
          {:else if hostForwards.length === 0}
            <p class="text-xs text-fg-muted">No tunnels for this host yet. Add one under <strong>Tunnels</strong>.</p>
          {:else}
            <div class="space-y-1">
              {#each hostForwards as f (f.id)}
                <label class="flex items-center gap-2 rounded-md border border-line px-2.5 py-1.5 text-sm">
                  <input type="checkbox" class="accent-input" checked={!!f.data?.auto_start} onchange={() => toggleAutoStart(f)} />
                  <span class="min-w-0 flex-1 truncate">{f.data?.label}</span>
                </label>
              {/each}
            </div>
          {/if}
        </div>
      </div>
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
