<script lang="ts">
  import { Download, ExternalLink, Loader2, X } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { formatSize, installHint, installUpdate, updates } from "$lib/stores/updates.svelte";

  const u = $derived(updates.available);
</script>

{#if u && (updates.showBanner || updates.installing)}
  <div class="fixed bottom-4 right-4 z-50 w-80 rounded-xl border border-accent/40 bg-panel p-4 text-sm shadow-2xl" role="status">
    <div class="flex items-start gap-3">
      <Download size={18} class="mt-0.5 shrink-0 text-accent" />
      <div class="min-w-0 flex-1">
        <div class="font-medium">SSHVault {u.version} is available</div>
        <div class="text-xs text-fg-muted">
          You have {updates.info?.version ?? "an older version"}.{u.size ? ` Download ${formatSize(u.size)}.` : ""}
        </div>
        {#if updates.installing}
          <div class="mt-2 h-1.5 overflow-hidden rounded bg-base">
            <div
              class="h-full bg-accent transition-[width] {updates.progress === null ? 'w-1/3 animate-pulse' : ''}"
              style:width={updates.progress === null ? undefined : `${Math.round(updates.progress * 100)}%`}
            ></div>
          </div>
          <div class="mt-1 flex items-center gap-1.5 text-xs text-fg-muted">
            <Loader2 size={12} class="animate-spin" />
            {updates.stage}{updates.progress !== null && updates.progress < 1 ? ` ${Math.round(updates.progress * 100)}%` : ""}
          </div>
        {:else}
          <p class="mt-1 text-[11px] text-fg-muted">{installHint(u.install)}</p>
          {#if updates.error}<p class="mt-2 text-xs text-danger">{updates.error}</p>{/if}
          <div class="mt-3 flex flex-wrap gap-2">
            <button class="btn-primary py-1 text-xs" onclick={() => void installUpdate()}>
              {#if updates.canInstall}<Download size={12} /> Update now{:else}<ExternalLink size={12} /> Download{/if}
            </button>
            <button class="btn-ghost py-1 text-xs" onclick={() => openUrl(u.url)}>What's new</button>
          </div>
        {/if}
      </div>
      {#if !updates.installing}
        <button class="icon-btn h-6 w-6" title="Later" aria-label="Dismiss" onclick={() => (updates.dismissed = u.version)}><X size={13} /></button>
      {/if}
    </div>
  </div>
{/if}
