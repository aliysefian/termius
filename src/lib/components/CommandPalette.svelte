<script lang="ts">
  import {
    Activity, FileOutput, Keyboard, SquareTerminal, ArrowLeftRight, ServerCog, Code, FileInput, FolderSync, KeyRound, Lock, Play, Plus, Server, Settings, SquareSplitHorizontal, SquareSplitVertical, Zap,
  } from "lucide-svelte";
  import * as api from "$lib/api";
  import { fuzzyScore } from "$lib/fuzzy";
  import { errorMessage } from "$lib/types";
  import { parseAdhoc } from "$lib/ssh";
  import { settings } from "$lib/stores/settings.svelte";
  import { runSnippet } from "$lib/runsnippet";
  import { adhocLabel, ui, type View } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";

  interface Item {
    id: string;
    label: string;
    hint?: string;
    group: string;
    icon: typeof Server;
    run: () => void;
  }

  let query = $state("");
  let selected = $state(0);
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLDivElement>();

  const go = (view: View) => () => (ui.view = view);

  const items = $derived.by<Item[]>(() => {
    const out: Item[] = [];
    const adhoc = parseAdhoc(query);
    if (adhoc) {
      out.push({
        id: "adhoc",
        label: `Connect to ${adhocLabel(adhoc)}`,
        hint: "Quick connect, not saved",
        group: "Quick connect",
        icon: Zap,
        run: () => (ui.modal = { kind: "quick-connect", initial: query.trim() }),
      });
    }
    // Recent hosts first, then the rest alphabetically.
    const rank = new Map(settings.recent.map((id, i) => [id, i]));
    const hosts = [...vaultStore.hosts].sort((a, b) => {
      const ra = rank.get(a.id) ?? Infinity;
      const rb = rank.get(b.id) ?? Infinity;
      return ra !== rb ? ra - rb : (a.data?.label ?? "").localeCompare(b.data?.label ?? "");
    });
    for (const h of hosts) {
      const d = h.data!;
      out.push({
        id: `host-${h.id}`,
        label: d.label,
        hint: `${d.hostname}${d.group ? ` · ${d.group}` : ""}`,
        group: rank.has(h.id) ? "Recent" : "Hosts",
        icon: Server,
        run: () => ui.openTerminal(h.id, d.label),
      });
    }
    for (const s of vaultStore.snippets) {
      const d = s.data!;
      out.push({
        id: `snip-${s.id}`,
        label: `Run: ${d.label}`,
        hint: d.command.split("\n")[0],
        group: "Snippets",
        icon: Play,
        run: () => void runSnippet(d.command, { execute: true, scope: "pane" }),
      });
      out.push({
        id: `snip-hosts-${s.id}`,
        label: `Run on hosts: ${d.label}`,
        hint: "Several hosts, in the background",
        group: "Snippets",
        icon: ServerCog,
        run: () => (ui.modal = { kind: "run-on-hosts", command: d.command }),
      });
    }
    const actions: [string, typeof Server, () => void, string?][] = [
      ["Quick connect…", Zap, () => (ui.modal = { kind: "quick-connect" }), "Ctrl+Shift+T"],
      ["New host", Plus, () => (ui.modal = { kind: "host", id: null })],
      ["New identity", KeyRound, () => (ui.modal = { kind: "identity", id: null })],
      ["New snippet", Code, () => (ui.modal = { kind: "snippet", id: null })],
      ["New port-forwarding rule", ArrowLeftRight, () => (ui.modal = { kind: "forward", id: null })],
      ["Import hosts from ~/.ssh/config", FileInput, () => (ui.modal = { kind: "import-ssh-config" })],
      ["Run a command on several hosts…", ServerCog, () => (ui.modal = { kind: "run-on-hosts" })],
      ["New local terminal", SquareTerminal, () => ui.openLocal(), "Ctrl+Shift+`"],
      ["Check which hosts are reachable", Activity, () => void vaultStore.checkHealth()],
      ["Type into all panes in this tab (toggle)", Keyboard, () => ui.syncRequest++, "Ctrl+Shift+B"],
      ["Export hosts as ~/.ssh/config…", FileOutput, () => void exportConfig()],
      ["Import hosts from an Ansible inventory", FileInput, () => (ui.modal = { kind: "import-ssh-config" })],
      ["Split right", SquareSplitHorizontal, () => ui.splitActive("vertical"), "Ctrl+Shift+D"],
      ["Split down", SquareSplitVertical, () => ui.splitActive("horizontal"), "Ctrl+Shift+E"],
      ["Go to Hosts", Server, go("hosts")],
      ["Go to Keychain", KeyRound, go("keychain")],
      ["Go to SFTP", FolderSync, go("sftp")],
      ["Go to Port Forwarding", ArrowLeftRight, go("forwarding")],
      ["Go to Snippets", Code, go("snippets")],
      ["Open Settings", Settings, go("settings")],
      ["Lock vault", Lock, () => void vaultStore.lock(), "Ctrl+Shift+L"],
    ];
    for (const [label, icon, run, hint] of actions) {
      out.push({ id: `act-${label}`, label, hint, group: "Actions", icon, run });
    }
    return out;
  });

  const results = $derived.by(() => {
    if (!query.trim()) return items.slice(0, 60);
    return items
      .map((it) => ({ it, s: fuzzyScore(query, `${it.label} ${it.hint ?? ""}`) }))
      .filter((x): x is { it: Item; s: number } => x.s !== null)
      .sort((a, b) => (a.it.id === "adhoc" ? -1 : b.it.id === "adhoc" ? 1 : b.s - a.s))
      .slice(0, 60)
      .map((x) => x.it);
  });

  $effect(() => {
    void query;
    selected = 0;
  });

  $effect(() => {
    input?.focus();
  });

  async function exportConfig() {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ title: "Export hosts as SSH config", defaultPath: "sshvault.config" });
    if (!path) return;
    try {
      await api.exportSshConfig(path);
      ui.notify("info", `Exported. Add "Include ${path}" to ~/.ssh/config to use these names with ssh.`);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function close() {
    ui.paletteOpen = false;
  }

  function run(it: Item | undefined) {
    if (!it) return;
    close();
    it.run();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(results[selected]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
    queueMicrotask(() => list?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" }));
  }
</script>

<div class="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[12vh]" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="w-full max-w-xl overflow-hidden rounded-xl border border-line bg-panel shadow-2xl" role="dialog" aria-label="Command palette">
    <input
      bind:this={input}
      class="w-full border-b border-line bg-transparent px-4 py-3 text-sm outline-none placeholder:text-fg-muted/60"
      placeholder="Search hosts, snippets and actions, or type user@host"
      bind:value={query}
      {onkeydown}
    />
    <div bind:this={list} class="max-h-[50vh] overflow-y-auto py-1">
      {#each results as it, i (it.id)}
        {#if i === 0 || results[i - 1].group !== it.group}
          <div class="px-4 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wide text-fg-muted">{it.group}</div>
        {/if}
        <button
          data-index={i}
          class="flex w-full items-center gap-3 px-4 py-2 text-left text-sm {i === selected ? 'bg-accent/20' : 'hover:bg-panel-hover'}"
          onmousemove={() => (selected = i)}
          onclick={() => run(it)}
        >
          <it.icon size={15} class="shrink-0 {i === selected ? 'text-accent' : 'text-fg-muted'}" />
          <span class="truncate">{it.label}</span>
          {#if it.hint}
            <span class="ml-auto truncate pl-4 text-xs text-fg-muted">{it.hint}</span>
          {/if}
        </button>
      {:else}
        <p class="px-4 py-6 text-center text-sm text-fg-muted">No matches.</p>
      {/each}
    </div>
    <div class="flex gap-4 border-t border-line px-4 py-2 text-[11px] text-fg-muted">
      <span>↑↓ navigate</span><span>Enter run</span><span>Esc close</span>
    </div>
  </div>
</div>
