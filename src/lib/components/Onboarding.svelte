<script lang="ts">
  import { Command, FileInput, Keyboard, Plus, Server, Zap } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { t } from "$lib/i18n/index.svelte";

</script>

<Modal title="Welcome to SSHVault" onclose={() => (ui.modal = null)}>
  <div class="space-y-4 text-sm">
    <p class="text-fg-muted">Your vault is ready and empty. Start however suits you:</p>
    <div class="space-y-2">
      <button class="flex w-full items-center gap-3 rounded-lg border border-line p-3 text-left hover:bg-panel-hover" onclick={() => (ui.modal = { kind: "import-ssh-config" })}>
        <FileInput size={18} class="shrink-0 text-accent" />
        <span>
          <span class="block font-medium">Import your hosts</span>
          <span class="text-xs text-fg-muted">From <code>~/.ssh/config</code>, an Ansible inventory, PuTTY, MobaXterm or a CSV export</span>
        </span>
      </button>
      <button class="flex w-full items-center gap-3 rounded-lg border border-line p-3 text-left hover:bg-panel-hover" onclick={() => (ui.modal = { kind: "host", id: null })}>
        <Server size={18} class="shrink-0 text-accent" />
        <span>
          <span class="block font-medium">Add a host by hand</span>
          <span class="text-xs text-fg-muted">One server at a time, with its own credentials and settings</span>
        </span>
      </button>
      <button class="flex w-full items-center gap-3 rounded-lg border border-line p-3 text-left hover:bg-panel-hover" onclick={() => (ui.modal = { kind: "quick-connect" })}>
        <Zap size={18} class="shrink-0 text-accent" />
        <span>
          <span class="block font-medium">Quick connect</span>
          <span class="text-xs text-fg-muted">Just <code>user@host</code>, without saving anything yet</span>
        </span>
      </button>
    </div>
    <div class="rounded-lg border border-line bg-base/60 p-3 text-xs text-fg-muted">
      <p class="mb-1.5 font-medium text-fg">Finding your way around</p>
      <ul class="space-y-1">
        <li class="flex items-center gap-2"><Server size={12} class="shrink-0" /> The icons on the left switch between hosts, keys, credentials, tunnels and settings.</li>
        <li class="flex items-center gap-2"><Command size={12} class="shrink-0" /> <strong class="font-mono">Ctrl+Shift+P</strong> opens the command palette: connect, run a snippet, jump anywhere.</li>
        <li class="flex items-center gap-2"><Keyboard size={12} class="shrink-0" /> <strong class="font-mono">Ctrl+Shift+/</strong> lists every shortcut, and lets you rebind them.</li>
      </ul>
    </div>
  </div>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>{t("tour.skip")}</button>
    <button class="btn-secondary" onclick={() => { ui.modal = null; ui.tour = true; }}>{t("tour.start")}</button>
  {/snippet}
</Modal>
