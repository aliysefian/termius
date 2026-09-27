<script lang="ts">
  import { onMount } from "svelte";
  import {
    Archive,
    CheckCircle2,
    FolderInput,
    GitMerge,
    KeyRound,
    Laptop,
    Lock,
    RefreshCw,
    RotateCcw,
    ShieldCheck,
    Stethoscope,
    TriangleAlert,
  } from "lucide-svelte";
  import StrengthMeter from "./StrengthMeter.svelte";
  import { ask } from "$lib/dialogs.svelte";
  import * as api from "$lib/api";
  import { pickFolder } from "$lib/api";
  import { withMasterPassword } from "$lib/secrets.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage, type BackupInfo, type ConflictInfo, type IntegrityReport, type VaultInfo } from "$lib/types";

  let info = $state<VaultInfo | null>(null);
  let backups = $state<BackupInfo[]>([]);
  let conflicts = $state<ConflictInfo[]>([]);
  let integrity = $state<IntegrityReport | null>(null);
  let busy = $state<string | null>(null);

  async function load() {
    try {
      [info, backups, conflicts] = await Promise.all([api.vault.info(), api.vault.listBackups(), api.vault.listConflicts()]);
      vaultStore.openConflicts = conflicts.length;
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }
  onMount(load);

  async function act(name: string, f: () => Promise<unknown>) {
    busy = name;
    try {
      await f();
    } catch (e) {
      ui.notify("error", errorMessage(e));
    } finally {
      busy = null;
    }
  }

  const when = (ms: number | null | undefined) => (ms ? new Date(ms).toLocaleString() : "never");
  const folderName = $derived(info?.path.split(/[\\/]/).filter(Boolean).at(-1) ?? "");

  // -- password -------------------------------------------------------------
  let current = $state("");
  let next = $state("");
  let confirmNext = $state("");

  async function changePassword(e: SubmitEvent) {
    e.preventDefault();
    if (next !== confirmNext) return ui.notify("error", "The new passwords don't match.");
    await act("password", async () => {
      await api.vault.changePassword(current, next);
      current = next = confirmNext = "";
      ui.notify("info", "Master password changed. Other devices need the new password next time they unlock.");
    });
  }

  // -- recovery -------------------------------------------------------------
  async function newRecoveryKey() {
    if (info?.has_recovery && !await ask("Replace the recovery key? The old one stops working.")) return;
    await act("recovery", async () => {
      const key = await withMasterPassword("Create a new recovery key", "Create", (pw) => api.vault.setRecoveryKey(pw));
      if (key) {
        vaultStore.pendingRecoveryKey = key;
        await load();
      }
    });
  }

  async function removeRecoveryKey() {
    if (!await ask("Remove the recovery key? If you then forget the master password, the vault can't be opened.")) return;
    await act("recovery", async () => {
      const ok = await withMasterPassword("Remove the recovery key", "Remove", (pw) => api.vault.removeRecoveryKey(pw));
      if (ok !== null) await load();
    });
  }

  // -- device ---------------------------------------------------------------
  async function toggleRemember() {
    await act("remember", async () => {
      vaultStore.status = info?.remembered ? await api.vault.forgetDevice() : await api.vault.rememberDevice();
      await load();
    });
  }

  // -- backups --------------------------------------------------------------
  async function backupNow() {
    await act("backup", async () => {
      const b = await api.vault.createBackup();
      ui.notify("info", `Encrypted backup ${b.file_name} created.`);
      await load();
    });
  }

  async function restore(b: BackupInfo) {
    if (
      !await ask(
        `Restore the backup from ${when(b.created_at)} (${b.records} records)?\n\nA backup of the current state is taken first, so this can be undone. Other devices receive the restored data as a normal change.`,
      )
    )
      return;
    await act("restore", async () => {
      const r = await api.vault.restoreBackup(b.file_name);
      ui.notify("info", `Restored: ${r.restored} changed, ${r.removed} removed, ${r.unchanged} unchanged. Undo with backup ${r.safety_backup}.`);
      await Promise.all([load(), vaultStore.reloadAll()]);
    });
  }

  // -- integrity & conflicts ------------------------------------------------
  async function verify() {
    await act("verify", async () => {
      integrity = await api.vault.verifyIntegrity();
    });
  }

  async function resolve(c: ConflictInfo, keep: "current" | "other") {
    await act("conflict", async () => {
      await api.vault.resolveConflict(c.collection, c.id, c.file_name, keep);
      await Promise.all([load(), vaultStore.reloadAll()]);
    });
  }

  const show = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));
  const deviceName = (id: string) => info?.devices.find((d) => d.id === id)?.data?.name ?? id.slice(0, 8);

  // -- move -----------------------------------------------------------------
  async function move() {
    const dest = await pickFolder("Choose an EMPTY folder to move the vault to");
    if (!dest) return;
    if (!await ask(`Copy the vault to ${dest}, verify the copy, then use it from now on?\n\nThe old folder is left as it is; delete it yourself once other devices have switched.`)) return;
    await act("move", async () => {
      vaultStore.status = await api.vault.move(dest);
      await load();
      ui.notify("info", "Vault moved. On other devices, choose Open Existing Vault and pick the new folder.");
    });
  }
</script>

<div class="flex-1 overflow-y-auto bg-base p-8">
  <div class="mx-auto max-w-3xl space-y-6">
    <div class="flex items-center justify-between">
      <h1 class="flex items-center gap-2 text-lg font-semibold"><ShieldCheck size={18} class="text-accent" /> Vault</h1>
      <div class="flex gap-2">
        <button class="icon-btn" title="Refresh" onclick={load}><RefreshCw size={14} /></button>
        <button class="btn-ghost border border-line" onclick={() => vaultStore.lock()}><Lock size={14} /> Lock now</button>
      </div>
    </div>

    {#if info}
      <section class="rounded-xl border border-line bg-panel p-5">
        <dl class="grid grid-cols-[10rem_1fr] gap-x-4 gap-y-1.5 text-sm">
          <dt class="text-fg-muted">Name</dt><dd>{folderName}</dd>
          <dt class="text-fg-muted">Vault ID</dt><dd class="font-mono text-xs">{info.vault_id}</dd>
          <dt class="text-fg-muted">Location</dt><dd class="truncate font-mono text-xs" title={info.path}>{info.path}</dd>
          <dt class="text-fg-muted">Encryption</dt><dd>{info.cipher}; records use a random vault key that only your password or recovery key can unwrap</dd>
          <dt class="text-fg-muted">Key derivation</dt><dd>{info.kdf}</dd>
          <dt class="text-fg-muted">Format</dt><dd>version {info.format_version}</dd>
          <dt class="text-fg-muted">Revision</dt><dd><span class="font-mono text-xs">{info.state_hash}</span> · {info.records} records <span class="text-xs text-fg-muted">(same value on two devices = in sync)</span></dd>
          <dt class="text-fg-muted">Last save</dt><dd>{when(info.last_change)}</dd>
          <dt class="text-fg-muted">This device</dt><dd>{info.device_name} {#if info.remembered}<span class="text-xs text-success">· remembered</span>{/if}</dd>
          <dt class="text-fg-muted">Backups</dt><dd>{backups.length} kept, last {when(info.last_backup)}</dd>
          <dt class="text-fg-muted">Recovery key</dt><dd>{info.has_recovery ? "set up" : "none"}</dd>
        </dl>
        {#if info.active_sessions.length}
          <p class="mt-3 flex items-center gap-1.5 text-xs text-fg-muted">
            <Laptop size={12} /> Also open on {info.active_sessions.map((s) => s.device_name).join(", ")}. Edits sync; simultaneous edits to the same item are merged or flagged, never silently overwritten.
          </p>
        {/if}
      </section>

      {#if conflicts.length}
        <section class="rounded-xl border border-warning/40 bg-panel p-5">
          <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold text-warning"><GitMerge size={15} /> Sync conflicts ({conflicts.length})</h2>
          <p class="mb-3 text-xs text-fg-muted">
            Two devices changed the same item at the same time and the changes couldn't be combined. Choose which version to
            keep; the other is removed. Secrets are hidden here.
          </p>
          <div class="space-y-3">
            {#each conflicts as c (c.file_name)}
              <div class="rounded-md border border-line p-3 text-xs">
                <div class="mb-2 font-medium">
                  {c.collection} · {show(c.current.data?.label ?? c.current.data?.name ?? c.id)}
                  <span class="text-fg-muted">· differs in {c.fields.join(", ") || "content"}</span>
                </div>
                <div class="grid grid-cols-2 gap-2">
                  {#each [["current", c.current], ["other", c.other]] as const as [side, rec] (side)}
                    <div class="rounded border border-line bg-base p-2">
                      <div class="mb-1 text-[10px] uppercase tracking-wide text-fg-muted">
                        {side === "current" ? "Current" : "Other"} · {deviceName(rec.device_id)} · {when(rec.updated_at)}
                      </div>
                      {#each c.fields as f (f)}
                        <div class="truncate font-mono"><span class="text-fg-muted">{f}:</span> {show(rec.data?.[f] ?? "—")}</div>
                      {/each}
                      <button class="btn-ghost mt-2 border border-line py-0.5 text-xs" disabled={!!busy} onclick={() => resolve(c, side)}>Keep this</button>
                    </div>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        </section>
      {/if}

      <section class="rounded-xl border border-line bg-panel p-5">
        <h2 class="mb-1 text-sm font-semibold">Master password</h2>
        <p class="mb-3 text-xs text-fg-muted">
          Only the key slot is re-encrypted, so this is instant and safe to interrupt. Other devices need the new password
          next time they unlock; devices that remember the vault keep working.
        </p>
        <form onsubmit={changePassword} class="grid max-w-sm gap-3">
          <input class="input" type="password" placeholder="Current password" bind:value={current} required autocomplete="current-password" />
          <div>
            <input class="input" type="password" placeholder="New password (min 8 characters)" bind:value={next} required minlength="8" autocomplete="new-password" />
            <StrengthMeter password={next} />
          </div>
          <input class="input" type="password" placeholder="Confirm new password" bind:value={confirmNext} required autocomplete="new-password" />
          <button class="btn-primary" type="submit" disabled={busy === "password"}>{busy === "password" ? "Changing…" : "Change master password"}</button>
        </form>
      </section>

      <section class="rounded-xl border border-line bg-panel p-5">
        <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><KeyRound size={15} class="text-accent" /> Recovery and this device</h2>
        <p class="mb-3 text-xs text-fg-muted">
          {info.has_recovery
            ? "A recovery key can reset the master password. Replacing it invalidates the old one."
            : "No recovery key: if the master password is forgotten, the vault can't be opened by anyone."}
        </p>
        <div class="flex flex-wrap gap-2">
          <button class="btn-ghost border border-line" disabled={!!busy} onclick={newRecoveryKey}>{info.has_recovery ? "Replace recovery key" : "Create recovery key"}</button>
          {#if info.has_recovery}
            <button class="btn-ghost border border-line hover:text-danger" disabled={!!busy} onclick={removeRecoveryKey}>Remove recovery key</button>
          {/if}
          <button class="btn-ghost border border-line" disabled={!!busy} onclick={toggleRemember}>
            {info.remembered ? "Forget on this device" : "Remember on this device"}
          </button>
        </div>
      </section>

      <section class="rounded-xl border border-line bg-panel p-5">
        <div class="mb-1 flex items-center justify-between">
          <h2 class="flex items-center gap-2 text-sm font-semibold"><Archive size={15} class="text-accent" /> Backups</h2>
          <button class="btn-ghost border border-line py-1 text-xs" disabled={!!busy} onclick={backupNow}>{busy === "backup" ? "Backing up…" : "Back up now"}</button>
        </div>
        <p class="mb-3 text-xs text-fg-muted">
          Encrypted with the vault key and kept in <code>backups/</code> inside the vault folder. One is taken automatically
          each day and before every restore.
        </p>
        <div class="max-h-72 divide-y divide-line overflow-y-auto rounded-md border border-line">
          {#each backups as b (b.file_name)}
            <div class="flex items-center gap-3 px-3 py-2 text-xs">
              {#if b.error}<TriangleAlert size={13} class="text-danger" />{:else}<CheckCircle2 size={13} class="text-success" />{/if}
              <div class="min-w-0 flex-1">
                <div>{when(b.created_at)} · {b.reason ?? "?"} · {b.device_name ?? "unknown device"}</div>
                <div class="text-fg-muted">{b.error ? `Unusable: ${b.error}` : `${b.records} records`}</div>
              </div>
              <button class="btn-ghost py-0.5 text-xs" disabled={!!busy || !!b.error} onclick={() => restore(b)}><RotateCcw size={12} /> Restore</button>
            </div>
          {:else}
            <p class="px-3 py-4 text-center text-xs text-fg-muted">No backups yet.</p>
          {/each}
        </div>
      </section>

      <section class="rounded-xl border border-line bg-panel p-5">
        <div class="mb-1 flex items-center justify-between">
          <h2 class="flex items-center gap-2 text-sm font-semibold"><Stethoscope size={15} class="text-accent" /> Verify integrity</h2>
          <button class="btn-ghost border border-line py-1 text-xs" disabled={!!busy} onclick={verify}>{busy === "verify" ? "Checking…" : "Verify"}</button>
        </div>
        <p class="text-xs text-fg-muted">Decrypts and checks every record, reference and backup. Read-only: nothing is changed.</p>
        {#if integrity}
          <div class="mt-3 rounded-md border p-3 text-xs {integrity.ok ? 'border-success/30 bg-success/5' : 'border-danger/30 bg-danger/5'}">
            <p class="font-medium {integrity.ok ? 'text-success' : 'text-danger'}">
              {integrity.ok ? "No problems found." : `${integrity.errors.length} problem(s) found.`}
              <span class="font-normal text-fg-muted">
                {integrity.records_checked} records, {integrity.backups_ok} good backups{integrity.backups_bad ? `, ${integrity.backups_bad} bad` : ""}{integrity.conflict_copies ? `, ${integrity.conflict_copies} conflict copies` : ""}.
              </span>
            </p>
            {#each integrity.errors as e (e)}<p class="mt-1 text-danger">{e}</p>{/each}
            {#each integrity.warnings as w (w)}<p class="mt-1 text-warning">{w}</p>{/each}
          </div>
        {/if}
      </section>

      <section class="rounded-xl border border-line bg-panel p-5">
        <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><Laptop size={15} class="text-accent" /> Devices</h2>
        <div class="mt-2 divide-y divide-line rounded-md border border-line text-xs">
          {#each info.devices as d (d.id)}
            <div class="flex items-center justify-between px-3 py-2">
              <span>{d.data?.name}{d.id === info.device_id ? " (this device)" : ""} <span class="text-fg-muted">· {d.data?.platform} · v{d.data?.app_version}</span></span>
              <span class="text-fg-muted">last seen {when(d.data?.last_seen)}</span>
            </div>
          {/each}
        </div>
      </section>

      <section class="rounded-xl border border-line bg-panel p-5">
        <h2 class="mb-1 flex items-center gap-2 text-sm font-semibold"><FolderInput size={15} class="text-accent" /> Move vault</h2>
        <p class="mb-3 text-xs text-fg-muted">Copies the vault to an empty folder, verifies the copy, then switches to it. The old folder is left in place.</p>
        <button class="btn-ghost border border-line" disabled={!!busy} onclick={move}>{busy === "move" ? "Moving…" : "Move to…"}</button>
      </section>
    {:else}
      <p class="text-sm text-fg-muted">Loading…</p>
    {/if}
  </div>
</div>
