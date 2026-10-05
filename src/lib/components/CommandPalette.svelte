<script lang="ts">
  import {
    Activity, Container, Database, FileOutput, History, Keyboard, SquareTerminal, ArrowLeftRight, ServerCog, Code, FileInput, FolderSync, KeyRound, Lock, Play, Plus, Server, Settings, ShieldAlert, SquareSplitHorizontal, SquareSplitVertical, Zap,
  } from "lucide-svelte";
  import * as api from "$lib/api";
  import { fuzzyScore } from "$lib/fuzzy";
  import { ask } from "$lib/dialogs.svelte";
  import { describeForward, errorMessage } from "$lib/types";
  import { parseAdhoc } from "$lib/ssh";
  import { settings } from "$lib/stores/settings.svelte";
  import { runSnippet } from "$lib/runsnippet";
  import { adhocLabel, ui, type View, type WorkspaceTab } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { databases } from "$lib/stores/databases.svelte";
  import { containers, LOCAL } from "$lib/stores/containers.svelte";

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
    const adhoc = parseAdhoc(query, (n, p, u) => vaultStore.findHop(n, p, u));
    if (adhoc) {
      out.push({
        id: "adhoc",
        label: `Connect to ${adhocLabel(adhoc)}${adhoc.jumps?.length ? ` via ${adhoc.jumps.length} jump host${adhoc.jumps.length > 1 ? "s" : ""}` : ""}`,
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
    // Recent commands on the active pane's host, then on recent hosts.
    const activeHost = ui.activeTab?.panes.find((p) => p.id === ui.activeTab?.activePaneId)?.target;
    const historyHosts = [activeHost?.kind === "host" ? activeHost.hostId : null, ...settings.recent].filter((x): x is string => !!x);
    const seenCmd = new Set<string>();
    for (const hid of [...new Set(historyHosts)].slice(0, 5)) {
      const h = vaultStore.hostById.get(hid)?.data;
      if (!h) continue;
      for (const e of (settings.history[hid] ?? []).slice(0, 15)) {
        const key = `${hid}\n${e.command}`;
        if (seenCmd.has(key)) continue;
        seenCmd.add(key);
        out.push({
          id: `hist-${key}`,
          label: e.command.split("\n")[0],
          hint: `on ${h.label}${e.exit ? ` · last exit ${e.exit}` : ""}`,
          group: "History",
          icon: Play,
          run: () => {
            const pane = ui.activeTab?.panes.find((p) => p.id === ui.activeTab?.activePaneId);
            if (pane?.target.kind === "host" && pane.target.hostId === hid && ui.paneInfo[pane.id]?.status === "connected") {
              void runSnippet(e.command, { execute: true, scope: "pane" });
            } else {
              ui.openTerminal(hid, h.label, e.command);
            }
          },
        });
      }
    }
    // Tunnels: found by name, host, address or port.
    for (const f of vaultStore.forwards) {
      const d = f.data;
      if (!d) continue;
      const h = vaultStore.hostById.get(d.host_id)?.data;
      const st = vaultStore.forwardStatus[f.id];
      const running = st?.state === "active" || st?.state === "starting";
      out.push({
        id: `fwd-${f.id}`,
        label: `${running ? "Stop" : "Start"} tunnel: ${d.label}`,
        hint: `${describeForward(d)} · ${h ? `${h.label} ${h.hostname}` : "missing host"}`,
        group: "Tunnels",
        icon: ArrowLeftRight,
        run: () => void (running ? vaultStore.stopForward(f.id) : vaultStore.startForward(f.id)),
      });
    }
    for (const w of vaultStore.workspaces) {
      const d = w.data!;
      out.push({
        id: `ws-${w.id}`,
        label: `Open workspace: ${d.name}`,
        hint: `${d.tabs.length} tab(s)`,
        group: "Workspaces",
        icon: SquareSplitHorizontal,
        run: () => ui.openWorkspace(d.tabs as WorkspaceTab[]),
      });
      out.push({
        id: `ws-del-${w.id}`,
        label: `Delete workspace: ${d.name}`,
        group: "Workspaces",
        icon: SquareSplitHorizontal,
        run: () =>
          void ask(`Delete workspace "${d.name}"?`).then((ok) => {
            if (ok) void vaultStore.deleteWorkspace(w.id);
          }),
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
      ["New credential", KeyRound, () => (ui.modal = { kind: "identity", id: null })],
      ["New snippet", Code, () => (ui.modal = { kind: "snippet", id: null })],
      ["New port-forwarding rule", ArrowLeftRight, () => (ui.modal = { kind: "forward", id: null })],
      ["Import hosts from ~/.ssh/config", FileInput, () => (ui.modal = { kind: "import-ssh-config" })],
      ["Run a command on several hosts…", ServerCog, () => (ui.modal = { kind: "run-on-hosts" })],
      ["New local terminal", SquareTerminal, () => ui.openLocal(), "Ctrl+Shift+`"],
      ["Open a serial console…", SquareTerminal, () => (ui.modal = { kind: "serial" })],
      ["Check which hosts are reachable", Activity, () => void vaultStore.checkHealth()],
      ["Type into all panes in this tab (toggle)", Keyboard, () => ui.syncRequest++, "Ctrl+Shift+B"],
      ["Export hosts as ~/.ssh/config…", FileOutput, () => void exportConfig()],
      ["Import hosts from an Ansible inventory", FileInput, () => (ui.modal = { kind: "import-ssh-config" })],
      ["Save open tabs as a workspace…", SquareSplitHorizontal, () => (ui.modal = { kind: "save-workspace" })],
      ["Hide or show the list panel", SquareSplitHorizontal, () => ui.toggleSidebar(), "Ctrl+Shift+H"],
      ["Maximize or restore the pane", SquareSplitHorizontal, () => ui.toggleZoomActive(), "Ctrl+Shift+Enter"],
      ["Keyboard shortcuts", Keyboard, () => (ui.modal = { kind: "shortcuts" }), "Ctrl+Shift+/"],
      ["Focus mode (toggle)", Keyboard, () => (settings.prefs.focusMode = !settings.prefs.focusMode), "Ctrl+Shift+U"],
      ["Import PuTTY sessions or a CSV of hosts…", FileInput, () => (ui.modal = { kind: "import-ssh-config" })],
      ["Split right", SquareSplitHorizontal, () => ui.splitActive("vertical"), "Ctrl+Shift+D"],
      ["Split down", SquareSplitVertical, () => ui.splitActive("horizontal"), "Ctrl+Shift+E"],
      ["Go to Hosts", Server, go("hosts")],
      ["Go to Favorites", Server, go("favorites")],
      ["Go to Groups and Proxies", Server, go("groups")],
      ["Go to Keys", KeyRound, go("keys")],
      ["Go to Credentials", KeyRound, go("keychain")],
      ["Go to SFTP", FolderSync, go("sftp")],
      ["Go to Tunnels", ArrowLeftRight, go("forwarding")],
      ["Go to Snippets", Code, go("snippets")],
      ["Go to Known Hosts", Settings, go("knownhosts")],
      ["Go to Vault (backups, recovery, integrity)", Lock, go("vault")],
      ["Go to Security review", ShieldAlert, go("security-review")],
      ["Go to Fleet", Activity, go("fleet")],
      ["Go to Databases", Database, go("databases")],
      ["Go to Containers", Container, go("containers")],
      ["Containers on this computer", Container, () => { ui.view = "containers"; void containers.open(LOCAL); }],
      ["New database connection", Database, () => (ui.modal = { kind: "db-connection", id: null })],
      ["View changelog", History, go("changelog")],
      ["Open Settings", Settings, go("settings")],
      ["Lock vault", Lock, () => void vaultStore.lock(), "Ctrl+Shift+L"],
    ];
    for (const c of vaultStore.dbConnections) {
      if (!c.data) continue;
      out.push({
        id: `db-${c.id}`,
        label: `Open database: ${c.data.name}`,
        hint: `${c.data.host}:${c.data.port}`,
        group: "Databases",
        icon: Database,
        run: () => {
          ui.view = "databases";
          void databases.connect(c.id).then((s) => s && databases.newTab(c.id));
        },
      });
    }
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
      e.stopPropagation();
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
