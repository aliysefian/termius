<script lang="ts">
  import { untrack } from "svelte";
  import { CHANNEL_NAMES, SMTP_PORTS, botProxyChoices, emptyChannels, type BotConfig, type ChannelKind, type NotifyChannels, type SmtpSecurity } from "$lib/monitoring";
  import { monitoring } from "$lib/stores/monitoring.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  let draft = $state<NotifyChannels>(emptyChannels());
  let loaded = $state(false);
  let busy = $state(false);
  let msg = $state<{ ok: boolean; text: string } | null>(null);

  // Take the stored settings once they arrive (the secrets in them are blank; only whether one is saved is known).
  $effect(() => {
    const saved = monitoring.channels;
    if (saved && !loaded) {
      untrack(() => {
        draft = structuredClone($state.snapshot(saved.channels));
        loaded = true;
      });
    }
  });

  const dirty = $derived(!!monitoring.channels && JSON.stringify($state.snapshot(draft)) !== JSON.stringify(monitoring.channels.channels));
  const anyOn = $derived(draft.telegram.enabled || draft.bale.enabled || draft.email.enabled);

  async function save() {
    busy = true;
    msg = null;
    try {
      await monitoring.saveChannels($state.snapshot(draft));
      draft = structuredClone($state.snapshot(monitoring.channels!.channels));
      msg = { ok: true, text: "Saved in the encrypted vault. Tokens and the password are not shown again." };
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
    } finally {
      busy = false;
    }
  }

  async function test(kind: ChannelKind) {
    busy = true;
    msg = null;
    try {
      await monitoring.testChannel(kind);
      msg = { ok: true, text: `A test message was sent on ${CHANNEL_NAMES[kind]}. Check that it arrived.` };
    } catch (e) {
      msg = { ok: false, text: errorMessage(e) };
    } finally {
      busy = false;
    }
  }

  function setSecurity(s: SmtpSecurity) {
    const wasDefault = draft.email.port === SMTP_PORTS[draft.email.security];
    draft.email.security = s;
    if (wasDefault) draft.email.port = SMTP_PORTS[s];
  }

  const bots: { kind: "telegram" | "bale"; hint: string; base: string }[] = [
    { kind: "telegram", hint: "Create a bot with @BotFather, then send it a message (or add it to a group) to get a chat ID.", base: "https://api.telegram.org" },
    { kind: "bale", hint: "Create a bot with the Bale bot father, then use its token and your chat ID. Bale's API is Telegram's.", base: "https://tapi.bale.ai" },
  ];
  const bot = (k: "telegram" | "bale"): BotConfig => draft[k];

  const proxyChoices = $derived(botProxyChoices(vaultStore.proxies));
  // A choice whose proxy was deleted (or changed to a command) stays visible, so it can be seen and replaced.
  const proxyMissing = (id: string | null) => !!id && !proxyChoices.some((p) => p.id === id);
</script>

<div class="space-y-5" data-testid="notify-channels">
  <p class="text-xs text-fg-muted">
    When a channel is switched on, an alert message is sent to that service, with the host's name and what was read (CPU 95%, port closed). Nothing is sent while all are off.
    The tokens and the password are kept in the encrypted vault and are never shown again after saving.
  </p>

  {#each bots as b (b.kind)}
    <fieldset class="space-y-3 rounded-lg border border-line p-3">
      <legend class="px-1 text-xs font-medium text-fg-muted">{CHANNEL_NAMES[b.kind]}</legend>
      <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" bind:checked={draft[b.kind].enabled} /> Send alerts on {CHANNEL_NAMES[b.kind]}</label>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="n-{b.kind}-token">Bot token</label>
          <input id="n-{b.kind}-token" class="input font-mono text-xs" type="password" autocomplete="off" spellcheck="false" bind:value={draft[b.kind].bot_token} placeholder={bot(b.kind).token_set ? "saved; type a new one to replace it" : "123456:ABC…"} />
        </div>
        <div>
          <label class="label" for="n-{b.kind}-chat">Chat ID</label>
          <input id="n-{b.kind}-chat" class="input font-mono text-xs" bind:value={draft[b.kind].chat_id} placeholder="-1001234567890 or @channel" spellcheck="false" />
        </div>
        <div class="col-span-2">
          <label class="label" for="n-{b.kind}-base">Bot address (only if you reach it through a mirror or relay of your own)</label>
          <input id="n-{b.kind}-base" class="input font-mono text-xs" value={draft[b.kind].base_url ?? ""} oninput={(e) => (draft[b.kind].base_url = e.currentTarget.value.trim() || null)} placeholder={b.base} spellcheck="false" />
        </div>
        <div class="col-span-2">
          <label class="label" for="n-{b.kind}-proxy">Proxy (if {CHANNEL_NAMES[b.kind]} is blocked on your network)</label>
          <select id="n-{b.kind}-proxy" class="input text-xs" value={draft[b.kind].proxy_id ?? ""} onchange={(e) => (draft[b.kind].proxy_id = e.currentTarget.value || null)}>
            <option value="">None, connect directly</option>
            {#each proxyChoices as p (p.id)}
              <option value={p.id}>{p.label}</option>
            {/each}
            {#if proxyMissing(draft[b.kind].proxy_id)}
              <option value={draft[b.kind].proxy_id}>A proxy that is no longer saved (or is not SOCKS5 or HTTP)</option>
            {/if}
          </select>
          <p class="mt-1 text-xs text-fg-muted">
            {#if proxyChoices.length === 0}
              No SOCKS5 or HTTP proxy is saved yet. Add one under Groups and Proxies, then choose it here.
            {:else}
              Alerts go out through it, and are not sent at all if it can't be reached. SOCKS5 and HTTP proxies are listed; they are kept under Groups and Proxies.
            {/if}
          </p>
        </div>
      </div>
      <p class="text-xs text-fg-muted">{b.hint}</p>
      <button class="btn-secondary py-1 text-xs" disabled={busy || dirty} title={dirty ? "Save first: the test uses the saved settings" : ""} onclick={() => void test(b.kind)}>Send test message</button>
    </fieldset>
  {/each}

  <fieldset class="space-y-3 rounded-lg border border-line p-3">
    <legend class="px-1 text-xs font-medium text-fg-muted">Email (SMTP)</legend>
    <label class="flex items-center gap-2 text-sm"><input type="checkbox" class="accent-input" bind:checked={draft.email.enabled} /> Send alerts by email</label>
    <div class="grid grid-cols-3 gap-3">
      <div class="col-span-2">
        <label class="label" for="n-smtp-server">Server</label>
        <input id="n-smtp-server" class="input font-mono text-xs" bind:value={draft.email.server} placeholder="smtp.example.com" spellcheck="false" />
      </div>
      <div>
        <label class="label" for="n-smtp-port">Port</label>
        <input id="n-smtp-port" class="input font-mono text-xs" type="number" min="1" max="65535" bind:value={draft.email.port} />
      </div>
      <div>
        <label class="label" for="n-smtp-sec">Security</label>
        <select id="n-smtp-sec" class="input text-xs" value={draft.email.security} onchange={(e) => setSecurity(e.currentTarget.value as SmtpSecurity)}>
          <option value="start_tls">STARTTLS (port 587)</option>
          <option value="tls">TLS (port 465)</option>
          <option value="plain">None (no encryption)</option>
        </select>
      </div>
      <div>
        <label class="label" for="n-smtp-user">User name</label>
        <input id="n-smtp-user" class="input text-xs" autocomplete="off" bind:value={draft.email.username} spellcheck="false" />
      </div>
      <div>
        <label class="label" for="n-smtp-pass">Password</label>
        <input id="n-smtp-pass" class="input text-xs" type="password" autocomplete="off" bind:value={draft.email.password} placeholder={draft.email.password_set ? "saved; type a new one" : ""} />
      </div>
      <div class="col-span-3 grid grid-cols-2 gap-3">
        <div>
          <label class="label" for="n-smtp-from">From (optional)</label>
          <input id="n-smtp-from" class="input text-xs" bind:value={draft.email.from} placeholder="alerts@example.com" spellcheck="false" />
        </div>
        <div>
          <label class="label" for="n-smtp-to">Send alerts to</label>
          <input id="n-smtp-to" class="input text-xs" bind:value={draft.email.to} placeholder="you@example.com, oncall@example.com" spellcheck="false" />
        </div>
      </div>
    </div>
    <p class="text-xs text-fg-muted">The password is only sent over an encrypted connection (or to this computer). With STARTTLS, a server that does not offer it is refused.</p>
    <button class="btn-secondary py-1 text-xs" disabled={busy || dirty} title={dirty ? "Save first: the test uses the saved settings" : ""} onclick={() => void test("email")}>Send test message</button>
  </fieldset>

  <label class="flex items-start gap-2 text-sm">
    <input type="checkbox" class="mt-0.5 accent-input" bind:checked={draft.include_address} />
    <span>
      Include the host's address in messages
      <span class="block text-xs text-fg-muted">Off: messages carry only the name you gave the host.</span>
    </span>
  </label>

  <div class="flex items-center gap-3">
    <button class="btn-primary" disabled={busy || !dirty || !monitoring.channels} onclick={() => void save()}>{busy ? "Working…" : "Save"}</button>
    {#if anyOn}<span class="text-xs text-warning">Alerts will be sent to the services switched on above.</span>{/if}
  </div>
  {#if msg}
    <p class="rounded-md border px-3 py-2 text-sm {msg.ok ? 'border-success/30 bg-success/10 text-success' : 'border-danger/30 bg-danger/10 text-danger'}">{msg.text}</p>
  {/if}
</div>
