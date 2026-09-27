<script lang="ts">
  import { Download, Loader2, X } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { RELEASES_URL, updates } from "$lib/stores/updates.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { ask } from "$lib/dialogs.svelte";

  const u = $derived(updates.available);
  const sessions = $derived(ui.tabs.reduce((n, t) => n + t.panes.length, 0));

  async function install() {
    if (
      updates.info?.can_install &&
      sessions > 0 &&
      !await ask(`Installing restarts SSHVault and closes ${sessions} open session${sessions === 1 ? "" : "s"}. Continue?`)
    )
      return;
    void updates.install();
  }
</script>

{#if u && updates.showBanner}
  <div class="fixed bottom-4 right-4 z-50 w-80 rounded-xl border border-accent/40 bg-panel p-4 text-sm shadow-2xl" role="status">
    <div class="flex items-start gap-3">
      <Download size={18} class="mt-0.5 shrink-0 text-accent" />
      <div class="min-w-0 flex-1">
        <div class="font-medium">SSHVault {u.version} is available</div>
        <div class="text-xs text-fg-muted">You have {updates.info?.version}.</div>
        {#if updates.installing}
          <div class="mt-2 h-1.5 overflow-hidden rounded bg-base">
            <div class="h-full bg-accent transition-[width] {updates.progress === null ? 'w-1/3 animate-pulse' : ''}" style:width={updates.progress === null ? undefined : `${Math.round(updates.progress * 100)}%`}></div>
          </div>
          <div class="mt-1 text-xs text-fg-muted">{updates.progress === 1 ? "Installing…" : "Downloading and checking the signature…"}</div>
        {/if}
        {#if updates.error}<p class="mt-2 text-xs text-danger">{updates.error}</p>{/if}
        {#if !updates.installing}
          <div class="mt-3 flex flex-wrap gap-2">
            <button class="btn-primary py-1 text-xs" onclick={install}>
              {#if updates.info?.can_install}Install and restart{:else}Download{/if}
            </button>
            <button class="btn-ghost py-1 text-xs" onclick={() => openUrl(RELEASES_URL)}>What's new</button>
          </div>
          {#if !updates.info?.can_install}
            <p class="mt-2 text-[11px] text-fg-muted">This copy was installed from a .deb or .rpm package, so install the new package the same way.</p>
          {/if}
        {:else}
          <div class="mt-2 flex items-center gap-1.5 text-xs text-fg-muted"><Loader2 size={12} class="animate-spin" /> Don't close the app.</div>
        {/if}
      </div>
      {#if !updates.installing}
        <button class="icon-btn h-6 w-6" title="Later" aria-label="Dismiss" onclick={() => (updates.dismissed = u.version)}><X size={13} /></button>
      {/if}
    </div>
  </div>
{/if}
