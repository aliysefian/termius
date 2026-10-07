<script lang="ts">
  import { KeyRound, Lock, ShieldAlert, ShieldCheck } from "lucide-svelte";
  import Badge from "./Badge.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { checkKeyHygiene, type Finding } from "$lib/hygiene";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  const PASSWORD_MAX_AGE_YEARS = 1;
  const YEAR_MS = 365 * 24 * 60 * 60 * 1000;

  const keyFindings = $derived(checkKeyHygiene(vaultStore.keys).map((f) => ({ ...f, go: () => (ui.view = "keys") })));

  const hasUsableKey = $derived(vaultStore.keys.some((k) => k.data?.private_key !== undefined));

  /** Hosts using password auth while a Key Manager key is available to switch to. */
  const passwordHostFindings = $derived.by(() => {
    if (!hasUsableKey) return [];
    const out: (Finding & { go: () => void })[] = [];
    for (const rec of vaultStore.hosts) {
      const d = rec.data;
      if (!d) continue;
      const identId = vaultStore.effectiveIdentity(d);
      const ident = identId ? vaultStore.identityById.get(identId)?.data : undefined;
      if (ident?.auth.type === "password") {
        out.push({
          id: `host:${rec.id}:password`,
          severity: "warning",
          subjectId: rec.id,
          subjectLabel: d.label,
          message: `${d.label}: connects with a password, though a key is available in the Key Manager.`,
          go: () => (ui.modal = { kind: "host", id: rec.id }),
        });
      }
    }
    return out;
  });

  /** Password identities whose record hasn't been touched in over a year — a proxy for "not rotated", since the vault only timestamps the whole record, not the password specifically. */
  const stalePasswordFindings = $derived.by(() => {
    const now = Date.now();
    const out: (Finding & { go: () => void })[] = [];
    for (const rec of vaultStore.identities) {
      const d = rec.data;
      if (!d || d.auth.type !== "password") continue;
      if (now - rec.updated_at < PASSWORD_MAX_AGE_YEARS * YEAR_MS) continue;
      const label = d.for_host ? (vaultStore.hostById.get(d.for_host)?.data?.label ?? d.label) : d.label;
      out.push({
        id: `identity:${rec.id}:stale`,
        severity: "warning",
        subjectId: rec.id,
        subjectLabel: label,
        message: `${label}: this password hasn't been changed in over a year, as far as the vault can tell.`,
        go: () => (ui.modal = d.for_host ? { kind: "host", id: d.for_host } : { kind: "identity", id: rec.id }),
      });
    }
    return out;
  });

  const all = $derived([...keyFindings, ...passwordHostFindings, ...stalePasswordFindings]);
  const dangers = $derived(all.filter((f) => f.severity === "danger"));
  const warnings = $derived(all.filter((f) => f.severity === "warning"));
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-3xl space-y-6">
    <div>
      <h1 class="flex items-center gap-2 text-lg font-semibold"><ShieldAlert size={20} class="text-accent" /> Security review</h1>
      <p class="mt-1 text-sm text-fg-muted">
        Certificate expiry, weak or ageing keys, and credentials worth a second look. Checked locally against what's
        already in the vault; nothing here is sent anywhere.
      </p>
    </div>

    {#if all.length === 0}
      <div class="rounded-xl border border-line bg-panel p-8">
        <EmptyState art="security" text="Nothing to flag right now." />
      </div>
    {:else}
      {#if dangers.length}
        <section class="rounded-xl border border-danger/40 bg-panel p-5">
          <h2 class="mb-3 flex items-center gap-2 text-sm font-semibold text-danger"><ShieldAlert size={15} /> Needs attention ({dangers.length})</h2>
          <ul class="divide-y divide-line rounded-md border border-line">
            {#each dangers as f (f.id)}
              <li class="flex items-center justify-between gap-3 px-3 py-2 text-sm">
                <span class="min-w-0 flex-1">{f.message}</span>
                <button class="btn-secondary shrink-0 py-1 text-xs" onclick={f.go}>Review</button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if warnings.length}
        <section class="rounded-xl border border-line bg-panel p-5">
          <h2 class="mb-3 flex items-center gap-2 text-sm font-semibold"><Badge tone="warning">{warnings.length}</Badge> Worth a look</h2>
          <ul class="divide-y divide-line rounded-md border border-line">
            {#each warnings as f (f.id)}
              <li class="flex items-center justify-between gap-3 px-3 py-2 text-sm">
                <span class="min-w-0 flex-1">{f.message}</span>
                <button class="btn-secondary shrink-0 py-1 text-xs" onclick={f.go}>Review</button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}

    <section class="rounded-xl border border-line bg-panel p-5 text-xs text-fg-muted">
      <h2 class="mb-2 flex items-center gap-2 text-sm font-semibold text-fg"><KeyRound size={14} class="text-accent" /> What's checked</h2>
      <ul class="list-inside list-disc space-y-1">
        <li>An OpenSSH certificate within 14 days of expiring, or already expired.</li>
        <li>An RSA key under 3072 bits, or a DSA key (deprecated by most servers).</li>
        <li>A key created more than 5 years ago.</li>
        <li>A host using a saved password while the Key Manager has a key it could use instead.</li>
        <li>A saved password whose record hasn't changed in over a year (a proxy for "not rotated": the vault timestamps the whole record, not the password specifically).</li>
      </ul>
      <p class="mt-2 flex items-center gap-1.5"><Lock size={12} /> Everything above is computed from what's already decrypted in this session; nothing leaves this computer.</p>
    </section>
  </div>
</div>
