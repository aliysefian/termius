<script lang="ts">
  import Combobox from "./Combobox.svelte";
  import { hostOptions } from "$lib/pickeroptions";
  import { onDestroy, onMount, untrack } from "svelte";
  import { getCurrentWindow, ProgressBarStatus } from "@tauri-apps/api/window";
  import { ChevronDown, ChevronUp, CircleStop, FilePen, HardDrive, Link2, Loader2, Monitor, Pause, Play, RotateCcw, Server, Unplug, X } from "lucide-svelte";
  import { formatEta, nextSample, type SpeedSample } from "$lib/transferspeed";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import FilePane from "./FilePane.svelte";
  import Badge from "./Badge.svelte";
  import ConflictDialog from "./ConflictDialog.svelte";
  import RemoteEditor from "./RemoteEditor.svelte";
  import { sftp, type Conflict } from "$lib/sftp";
  import { backupName, planFromDecisions, type Decision, type SideInfo } from "$lib/conflict";
  import { cdCommand, followedPath, OSC7_BASH_ZSH, OSC7_FISH } from "$lib/cwdsync";
  import { choose } from "$lib/dialogs.svelte";
  import { PaneConn, THIS_COMPUTER } from "$lib/paneconn.svelte";
  import { writeToPane } from "$lib/terminalio";
  import { ui, type Pane } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { errorMessage, formatBytes, type FileEntry, type TransferProgress, type Uuid } from "$lib/types";

  interface Transfer {
    id: string;
    label: string;
    progress: TransferProgress;
    sources: string[];
    destDir: string;
    /** Continue partial files instead of starting over (retries). */
    resume: boolean;
    /** What to do with names already at the destination. */
    conflict: Conflict;
    /** Measured throughput while it runs. */
    speed?: SpeedSample;
    /** The backend ids of the two places, and what to call them. */
    fromId: string;
    toId: string;
    fromLabel: string;
    toLabel: string;
    /** Both places are hosts: the bytes pass through this app, straight from one host to the other, never to disk. */
    relay: boolean;
    /** Whether either place is this computer, for the arrow. */
    toLocal: boolean;
  }

  /** Transfers running at once; the rest wait their turn. */
  const MAX_RUNNING = 2;

  // The left pane starts on this computer, the right one waits for a host; either can be either.
  const left = new PaneConn("left", true);
  const right = new PaneConn("right", false);
  const panes = [left, right];
  const other = (c: PaneConn) => (c === left ? right : left);

  let transfers = $state<Transfer[]>([]);
  let paneRow = $state<HTMLDivElement>();

  function startSplitDrag(e: PointerEvent) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    el.focus();
    const box = paneRow!.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      const ratio = (ev.clientX - box.left) / box.width;
      settings.prefs.sftpSplitRatio = Math.min(0.85, Math.max(0.15, ratio));
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }

  // Taskbar/dock progress across every transfer, so the queue's state is
  // visible without switching back to this window.
  $effect(() => {
    let done = 0;
    let total = 0;
    let activeCount = 0;
    for (const t of transfers) {
      const p = t.progress;
      if (p.state === "progress" || p.state === "paused") {
        activeCount++;
        done += p.bytes;
        total += p.total_bytes;
      } else if (p.state === "started") {
        activeCount++;
        total += p.total_bytes;
      }
    }
    if (activeCount === 0) {
      void getCurrentWindow().setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
      return;
    }
    const progress = total > 0 ? Math.round((done / total) * 100) : 0;
    void getCurrentWindow().setProgressBar({ status: ProgressBarStatus.Normal, progress }).catch(() => {});
  });

  // Hosts with files to browse: SSH ones, and FTP ones. Not Remote Desktop or Telnet. This computer is first.
  const choices = $derived([
    { value: THIS_COMPUTER, label: "This computer", detail: "local disk" },
    ...hostOptions(vaultStore.hosts, (h) => !h.data?.protocol || h.data.protocol === "ssh" || h.data.protocol === "ftp"),
  ]);

  // -- editing a file in the local editor -----------------------------------

  interface Editing {
    id: string;
    name: string;
    remote: string;
    uploads: number;
    lastUpload?: Date;
    error?: string;
    conn: PaneConn;
  }
  let edits = $state<Editing[]>([]);
  let unlistenEdit: UnlistenFn | null = null;

  onMount(async () => {
    unlistenEdit = await api.remoteEdit.onEvent((e) => {
      const ed = edits.find((x) => x.id === e.edit_id);
      if (!ed) return;
      if (e.state === "uploaded") {
        ed.uploads++;
        ed.lastUpload = new Date();
        ed.error = undefined;
        ed.conn.refresh++;
      } else {
        ed.error = e.message;
      }
    });
  });

  async function edit(conn: PaneConn, entry: FileEntry) {
    if (edits.find((e) => e.remote === entry.path && e.conn === conn)) {
      ui.notify("info", `${entry.name} is already open for editing.`);
      return;
    }
    try {
      const started = await api.remoteEdit.start(conn.sessionId, entry.path);
      edits.push({ id: started.edit_id, name: entry.name, remote: entry.path, uploads: 0, conn });
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  async function stopEdit(id: string) {
    await api.remoteEdit.stop(id);
    edits = edits.filter((e) => e.id !== id);
  }

  async function stopAllEdits(of?: PaneConn) {
    const mine = edits.filter((e) => !of || e.conn === of);
    await Promise.all(mine.map((e) => api.remoteEdit.stop(e.id)));
    edits = edits.filter((e) => !mine.includes(e));
  }

  onDestroy(() => {
    unlistenEdit?.();
    void stopAllEdits();
    for (const c of panes) void c.dispose();
  });

  // -- editing a file in the app ---------------------------------------------

  let editor = $state<{ conn: PaneConn; path: string; hostId: Uuid | null } | null>(null);

  function openEditor(conn: PaneConn, entry: FileEntry) {
    editor = { conn, path: entry.path, hostId: conn.isLocal ? null : conn.connectedHost };
  }

  // -- the picker and what connecting needs ---------------------------------------

  async function disconnect(conn: PaneConn) {
    await stopAllEdits(conn);
    await conn.disconnect();
    conn.choice = "";
  }

  // "Browse here" from a terminal pane: show that host's folder in a pane that already has the host, else the right one.
  let handledRequest = 0;
  $effect(() => {
    const r = ui.sftpRequest;
    if (!r || r.n === handledRequest) return;
    handledRequest = r.n;
    void (async () => {
      const conn = panes.find((c) => !c.isLocal && c.connectedHost === r.hostId) ?? right;
      if (conn.connectedHost !== r.hostId) {
        if (conn.connectedHost) await disconnect(conn);
        await conn.pick(r.hostId);
      }
      if (conn.connectedHost === r.hostId && r.path) {
        conn.path = r.path;
        conn.refresh++;
      }
    })();
  });

  // Files dragged in from the OS file manager upload to a host's folder: the right pane if it shows one, else the left.
  let osDrop = $state(false);
  const dropTarget = $derived(!right.isLocal && right.connectedHost ? right : !left.isLocal && left.connectedHost ? left : null);
  $effect(() => {
    let off: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
      off = await getCurrentWebview().onDragDropEvent((ev) => {
        if (ui.view !== "sftp") return;
        const p = ev.payload;
        if (p.type === "enter" || p.type === "over") osDrop = !!dropTarget;
        else if (p.type === "leave") osDrop = false;
        else if (p.type === "drop") {
          osDrop = false;
          const target = dropTarget;
          if (target && p.paths.length) void upload(target, p.paths);
          else if (p.paths.length) ui.notify("error", "Open a host in one of the panes first, then drop files to upload them.");
        }
      });
    });
    return () => off?.();
  });

  /** Copy files that are on this computer (dropped from the OS) to `to`. */
  async function upload(to: PaneConn, paths: string[]) {
    const here = panes.find((c) => c.isLocal) ?? left;
    await copy(here, to, paths, here.isLocal ? undefined : "This computer");
  }

  function baseName(p: string) {
    return p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? p;
  }

  /** What copying from this pane to the other is called. */
  function verbFor(c: PaneConn): "Upload" | "Download" | "Copy" {
    const o = other(c);
    if (c.isLocal && !o.isLocal) return "Upload";
    if (!c.isLocal && o.isLocal) return "Download";
    return "Copy";
  }

  /** Copy `sources` from one pane to the other, asking about names that are already there. */
  async function start(from: PaneConn, to: PaneConn, sources: string[]) {
    if (sources.length === 0) return;
    if (!from.source || !to.source) {
      ui.notify("info", "Choose This computer or a host in the other pane first.");
      return;
    }
    await copy(from, to, sources);
  }

  async function copy(from: PaneConn, to: PaneConn, sources: string[], fromLabel?: string) {
    const src = from.source!;
    const dst = to.source!;
    const destDir = to.path;
    // The names already in the destination, and what the incoming ones are (for the comparison).
    let there: FileEntry[];
    try {
      there = await dst.list(destDir);
    } catch {
      // Can't tell; the transfer itself reports a problem with the folder.
      queue(from, to, sources, destDir, "overwrite", fromLabel);
      return;
    }
    const byName = new Map(there.map((e) => [e.name, e]));
    const clashes = sources.map(baseName).filter((n) => byName.has(n));
    if (clashes.length === 0) {
      queue(from, to, sources, destDir, "overwrite", fromLabel);
      return;
    }
    let here: FileEntry[] = [];
    try {
      here = await src.list(from.path);
    } catch {
      // The names alone are enough to ask with.
    }
    const incoming = new Map(here.map((e) => [e.path, e]));
    const info = (e: FileEntry | undefined, name: string, path: string): SideInfo => ({ name, path, size: e?.size ?? 0, modified: e?.modified ?? null, isDir: e?.is_dir ?? false });

    const decisions = new Map<string, Decision>();
    for (let i = 0; i < clashes.length; i++) {
      const name = clashes[i];
      if (decisions.has(name)) continue;
      const srcPath = sources.find((s) => baseName(s) === name)!;
      const existing = byName.get(name)!;
      const answer = await askConflict({
        src: info(incoming.get(srcPath), name, srcPath),
        dst: info(existing, name, existing.path),
        srcSession: from.id,
        dstSession: to.id,
        srcLabel: fromLabel ?? from.label,
        dstLabel: to.label,
        remaining: clashes.slice(i + 1).filter((n) => !decisions.has(n)).length,
      });
      if (!answer) return;
      decisions.set(name, answer.decision);
      if (answer.all) for (const n of clashes.slice(i + 1)) decisions.set(n, answer.decision);
    }

    const plan = planFromDecisions(sources, new Set(clashes), decisions, baseName);
    // "Backup & replace": what is there becomes name.bak first.
    const taken = new Set(byName.keys());
    for (const s of plan.backup) {
      const name = baseName(s);
      const bak = backupName(taken, name);
      taken.add(bak);
      try {
        await dst.rename(byName.get(name)!.path, bak);
      } catch (e) {
        ui.notify("error", `Could not back up ${name}: ${errorMessage(e)}. Nothing was copied.`);
        return;
      }
    }
    const main = [...plan.overwrite, ...plan.backup];
    if (main.length) queue(from, to, main, destDir, "overwrite", fromLabel);
    if (plan.rename.length) queue(from, to, plan.rename, destDir, "rename", fromLabel);
    if (!main.length && !plan.rename.length) ui.notify("info", "Nothing was copied: every name was skipped.");
  }

  // The overwrite dialog, asked one clash at a time.
  interface Asking {
    src: SideInfo;
    dst: SideInfo;
    srcSession: string;
    dstSession: string;
    srcLabel: string;
    dstLabel: string;
    remaining: number;
    resolve: (r: { decision: Decision; all: boolean } | null) => void;
  }
  let asking = $state<Asking | null>(null);
  function askConflict(c: Omit<Asking, "resolve">) {
    return new Promise<{ decision: Decision; all: boolean } | null>((resolve) => (asking = { ...c, resolve }));
  }
  function answered(r: { decision: Decision; all: boolean } | null) {
    const a = asking;
    asking = null;
    a?.resolve(r);
  }

  /** Add a transfer to the queue; it starts when a slot is free. */
  function queue(from: PaneConn, to: PaneConn, sources: string[], destDir: string, conflict: Conflict, fromLabel?: string) {
    const label = sources.length === 1 ? baseName(sources[0]) : `${sources.length} items`;
    transfers.push({
      id: crypto.randomUUID(),
      label,
      progress: { state: "queued" },
      sources,
      destDir,
      resume: false,
      conflict,
      fromId: from.id,
      toId: to.id,
      fromLabel: fromLabel ?? from.label,
      toLabel: to.label,
      relay: !from.isLocal && !to.isLocal,
      toLocal: to.isLocal,
    });
    transfersOpen = true;
    pump();
  }

  /** Start queued transfers, oldest first, up to the limit. */
  function pump() {
    let running = transfers.filter((t) => active(t.progress)).length;
    for (const t of transfers) {
      if (running >= MAX_RUNNING) break;
      if (t.progress.state === "queued") {
        running++;
        void run(t);
      }
    }
  }

  async function run(t: Transfer) {
    const id = t.id;
    t.progress = { state: "started", total_bytes: 0, total_files: 0 };
    const find = () => transfers.find((x) => x.id === id);
    try {
      await sftp.transfer(
        t.fromId,
        t.toId,
        id,
        t.sources,
        t.destDir,
        (p) => {
          const cur = find();
          if (!cur) return;
          cur.progress = p;
          // Speed is measured only while bytes are moving; a pause or an end leaves nothing to report.
          cur.speed = p.state === "progress" ? nextSample(cur.speed, p.bytes, Date.now()) : p.state === "paused" ? undefined : cur.speed;
          if (p.state === "done") for (const c of panes) if (c.id === cur.toId) c.refresh++;
          if (p.state === "done" || p.state === "failed" || p.state === "cancelled") pump();
        },
        { resume: t.resume, conflict: t.conflict },
      );
    } catch (e) {
      const cur = find();
      if (cur) cur.progress = { state: "failed", message: errorMessage(e) };
      pump();
    }
  }

  /** Try again, continuing partial files rather than starting over. */
  function retry(t: Transfer) {
    // Retries are appended with a fresh id so late events from the old run can't clobber them.
    transfers = transfers.filter((x) => x.id !== t.id);
    transfers.push({ ...t, id: crypto.randomUUID(), progress: { state: "queued" }, resume: true, speed: undefined });
    pump();
  }

  function cancel(t: Transfer) {
    if (t.progress.state === "queued") t.progress = { state: "cancelled" };
    else void sftp.cancel(t.id);
    pump();
  }

  function pct(p: TransferProgress) {
    if (p.state === "done") return 100;
    if ((p.state === "progress" || p.state === "paused") && p.total_bytes > 0) return Math.min(100, (p.bytes / p.total_bytes) * 100);
    return 0;
  }

  const active = (p: TransferProgress) => p.state === "started" || p.state === "progress" || p.state === "paused";
  const running = $derived(transfers.filter((t) => active(t.progress)).length);
  /** The queue can be folded to its header to give the file lists the room; a new transfer unfolds it. */
  let transfersOpen = $state(true);
  const moving = $derived(transfers.filter((t) => t.progress.state === "progress"));
  const totalBps = $derived(moving.reduce((n, t) => n + (t.speed?.bps ?? 0), 0));
  const overall = $derived.by(() => {
    let done = 0;
    let total = 0;
    for (const t of transfers) {
      const p = t.progress;
      if (p.state === "progress" || p.state === "paused") {
        done += p.bytes;
        total += p.total_bytes;
      }
    }
    return total > 0 ? Math.min(100, (done / total) * 100) : 0;
  });
  const speedText = (bps: number) => (bps > 0 ? `${formatBytes(bps)}/s` : "");
  const queued = $derived(transfers.filter((t) => t.progress.state === "queued").length);

  // -- the terminal and the browser ----------------------------------------------------------

  /** The terminal pane in front, and what it is on: a host's id, `"local"`, or nothing. */
  const frontTerminal = $derived.by<{ pane: Pane; on: Uuid | "local" } | null>(() => {
    const tab = ui.activeTab;
    const pane = tab?.panes.find((p) => p.id === tab.activePaneId);
    if (!pane) return null;
    if (pane.target.kind === "host") return { pane, on: pane.target.hostId };
    if (pane.target.kind === "local") return { pane, on: "local" };
    return null;
  });

  const paneOn = (c: PaneConn): Uuid | "local" | null => (c.isLocal ? "local" : c.connectedHost);

  // Follow the folder of the terminal in front: act when the terminal's folder changes, not whenever the browser does,
  // so a person who then browses elsewhere is left there until the terminal moves again.
  let lastFollowed = "";
  $effect(() => {
    const follow = settings.prefs.sftpFollowCwd;
    const term = frontTerminal;
    const cwd = term ? ui.paneInfo[term.pane.id]?.cwd : undefined;
    if (!follow || !term || !cwd) {
      if (!follow) lastFollowed = "";
      return;
    }
    const key = `${term.pane.id}|${cwd}`;
    if (key === lastFollowed) return;
    lastFollowed = key;
    for (const c of panes) {
      const next = followedPath({ follow: true, paneHostId: paneOn(c), terminalHostId: term.on, cwd, current: untrack(() => c.path) });
      if (next) c.path = next;
    }
  });

  function toggleFollow() {
    settings.prefs.sftpFollowCwd = !settings.prefs.sftpFollowCwd;
    lastFollowed = "";
  }

  /** What to tell about following, for a pane that shows something. */
  function followHint(c: PaneConn): { text: string; setup: boolean } | null {
    if (!settings.prefs.sftpFollowCwd) return null;
    const on = paneOn(c);
    const term = frontTerminal;
    if (!term) return { text: "Open a terminal to follow its folder.", setup: false };
    if (term.on !== on) return { text: "Follows the terminal of the same host: the one in front is on another.", setup: false };
    if (!ui.paneInfo[term.pane.id]?.cwd) return { text: "This terminal has not said which folder it is in.", setup: true };
    return null;
  }

  /** Types the line that makes the shell report its folder into the terminal in front. */
  async function setupTracking() {
    const term = frontTerminal;
    if (!term) return;
    const shell = await choose(
      "Which shell is the terminal running? This types one line into it, so the shell reports its folder after every command. Add the same line to its startup file (~/.bashrc, ~/.zshrc or fish's config.fish) to keep it for new sessions.",
      [
        { value: "bash", label: "bash or zsh" },
        { value: "fish", label: "fish" },
      ],
      { title: "Set up folder tracking" },
    );
    if (shell === null) return;
    await writeToPane(term.pane, (shell === "fish" ? OSC7_FISH : OSC7_BASH_ZSH) + "\r");
  }

  /** The terminal to send a folder to: the one in front if it is on this place, else any open one that is. */
  function terminalOn(on: Uuid | "local"): Pane | undefined {
    if (frontTerminal?.on === on) return frontTerminal.pane;
    for (const tab of ui.tabs) {
      for (const p of tab.panes) {
        const t = p.target;
        if ((on === "local" && t.kind === "local") || (t.kind === "host" && t.hostId === on)) return p;
      }
    }
    return undefined;
  }

  /** "Open in terminal (cd here)": take a terminal on this place to the folder, or open one that starts there. */
  function cdHere(c: PaneConn, entry: FileEntry) {
    const on = paneOn(c);
    if (!on) return;
    const cmd = cdCommand(entry.path, c.isLocal && navigator.userAgent.includes("Windows"));
    const term = terminalOn(on);
    if (term) {
      if (ui.paneInfo[term.id]?.running) {
        ui.notify("error", "A program is running in that terminal, so the folder was not changed. Open a new terminal there instead.");
        return;
      }
      void writeToPane(term, cmd + "\r");
      ui.showPane(term.id);
      ui.view = "hosts";
      return;
    }
    if (on === "local") ui.openLocal(cmd);
    else ui.openTerminal(on, c.label, cmd);
  }
</script>

{#snippet picker(conn: PaneConn)}
  {#if conn.isLocal}
    <Monitor size={14} class="shrink-0 text-accent" />
  {:else}
    <Server size={14} class="shrink-0 text-accent" />
  {/if}
  <Combobox
    class="min-w-0 flex-1"
    inputClass="py-1"
    options={choices}
    bind:value={conn.choice}
    ariaLabel={conn.side === "right" ? "Remote host" : "Left pane host"}
    placeholder="Search hosts…"
    emptyText="No host matches"
    onchange={(v) => {
      // Picking is intent enough: connect straight away.
      if (v) void conn.pick(v);
    }}
  />
  {#if conn.connecting}<Loader2 size={13} class="shrink-0 animate-spin text-fg-muted" />{/if}
  {#if conn.connectedHost}
    {#if conn.connectedData?.protocol === "ftp"}
      {#if (conn.connectedData.ftp?.tls ?? "explicit") === "none"}
        <Badge tone="warning" title="Plain FTP sends everything, including the password, readable by anyone on the network path">UNENCRYPTED</Badge>
      {:else}
        <Badge title="The connection is encrypted with TLS">FTPS</Badge>
      {/if}
    {:else if conn.connectedData?.file_protocol === "scp"}
      <Badge title="This host's files go over SCP, not SFTP">SCP</Badge>
    {/if}
  {/if}
  {#if conn.open}
    <button
      class="icon-btn h-7 w-7 shrink-0 {settings.prefs.sftpFollowCwd ? 'text-accent' : ''}"
      title={settings.prefs.sftpFollowCwd ? "Following the terminal's folder (click to stop)" : "Follow the terminal's folder"}
      aria-label="Follow terminal folder"
      aria-pressed={settings.prefs.sftpFollowCwd}
      onclick={toggleFollow}
      data-testid="follow-cwd"
    >
      <Link2 size={14} />
    </button>
  {/if}
  {#if conn.connectedHost}
    <button class="btn-ghost shrink-0 py-1 text-xs" onclick={() => void disconnect(conn)}><Unplug size={12} /> Disconnect</button>
  {/if}
{/snippet}

{#snippet noticeFor(conn: PaneConn)}
  {@const hint = followHint(conn)}
  {#if hint}
    <div class="flex items-center gap-2 border-b border-line bg-base/40 px-3 py-1 text-[11px] text-fg-muted" data-testid="follow-hint">
      <Link2 size={11} class="shrink-0" />
      <span class="min-w-0 flex-1 truncate">{hint.text}</span>
      {#if hint.setup}<button class="shrink-0 font-medium text-accent hover:underline" onclick={() => void setupTracking()}>Set up folder tracking</button>{/if}
    </div>
  {/if}
{/snippet}

{#snippet placeholderFor(conn: PaneConn)}
  <div class="max-w-xs">
    {#if conn.askCreds}
      <form
        class="space-y-3 rounded-xl border border-line bg-panel p-5 text-left"
        onsubmit={(e) => {
          e.preventDefault();
          void conn.connect({ username: conn.username, password: conn.password });
        }}
      >
        <div class="text-sm font-semibold">Credentials for {conn.host?.label}</div>
        <div>
          <label class="label" for="sftp-user-{conn.side}">Username</label>
          <input id="sftp-user-{conn.side}" class="input font-mono" autocomplete="username" bind:value={conn.username} required />
        </div>
        <div>
          <label class="label" for="sftp-pass-{conn.side}">Password</label>
          <input id="sftp-pass-{conn.side}" class="input" type="password" autocomplete="current-password" bind:value={conn.password} required />
        </div>
        <div class="flex gap-2">
          <button class="btn-ghost flex-1" type="button" onclick={() => conn.cancelCreds()}>Cancel</button>
          <button class="btn-primary flex-1" type="submit">Connect</button>
        </div>
      </form>
    {:else}
      <HardDrive size={28} class="mx-auto mb-3 text-fg-muted/50" />
      <p class="text-sm text-fg-muted">Choose This computer or a host above to browse its files over SFTP (or SCP, where SFTP is off).</p>
      {#if conn.connectError}
        <p class="mt-3 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{conn.connectError}</p>
        {#if conn.sftpOff}
          <button class="btn-primary mt-3" onclick={() => void conn.useScp()}>Use SCP for this host</button>
          <p class="mt-2 text-xs text-fg-muted">SCP can browse, upload, download, rename and delete, but can't continue a partly copied file. The choice is saved on the host.</p>
        {/if}
      {/if}
    {/if}
  </div>
{/snippet}

{#snippet filepane(conn: PaneConn)}
  {@const verb = verbFor(conn)}
  <FilePane
    side={conn.isLocal ? "local" : "remote"}
    pane={conn.side}
    {verb}
    source={conn.source}
    bind:path={conn.path}
    refreshKey={conn.refresh}
    transferLabel={conn.side === "left" ? `${verb} →` : `← ${verb}`}
    onTransfer={(paths) => start(conn, other(conn), paths)}
    onDropFrom={(paths) => start(other(conn), conn, paths)}
    onEdit={conn.isLocal ? undefined : (e) => edit(conn, e)}
    onOpenEditor={(e) => openEditor(conn, e)}
    onCdHere={paneOn(conn) ? (e) => cdHere(conn, e) : undefined}
  >
    {#snippet header()}
      {@render picker(conn)}
    {/snippet}
    {#snippet notice()}
      {@render noticeFor(conn)}
    {/snippet}
    {#snippet placeholder()}
      {@render placeholderFor(conn)}
    {/snippet}
  </FilePane>
{/snippet}

<div class="relative flex min-w-0 flex-1 flex-col">
  {#if osDrop && dropTarget}
    <div class="pointer-events-none absolute inset-0 z-30 flex items-center justify-center border-2 border-dashed border-accent bg-accent/10 text-sm font-medium text-accent">
      Drop to upload to {dropTarget.path || "the remote folder"} on {dropTarget.label}
    </div>
  {/if}
  <div bind:this={paneRow} class="flex min-h-0 flex-1">
    <div class="flex min-h-0 min-w-0 flex-col overflow-hidden" style:flex="0 0 {settings.prefs.sftpSplitRatio * 100}%">
      {@render filepane(left)}
    </div>

    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="relative w-1.5 shrink-0 cursor-col-resize outline-none after:absolute after:left-1/2 after:top-0 after:h-full after:w-px after:-translate-x-1/2 after:bg-line after:transition-colors hover:after:bg-accent/60 focus-visible:after:bg-accent"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the left and right panes"
      aria-valuenow={Math.round(settings.prefs.sftpSplitRatio * 100)}
      aria-valuemin={15}
      aria-valuemax={85}
      tabindex="0"
      title="Drag to resize · double-click to centre"
      onpointerdown={startSplitDrag}
      ondblclick={() => (settings.prefs.sftpSplitRatio = 0.5)}
      onkeydown={(e) => {
        const step = e.shiftKey ? 0.05 : 0.01;
        if (e.key === "ArrowLeft") { e.preventDefault(); settings.prefs.sftpSplitRatio = Math.max(0.15, settings.prefs.sftpSplitRatio - step); }
        else if (e.key === "ArrowRight") { e.preventDefault(); settings.prefs.sftpSplitRatio = Math.min(0.85, settings.prefs.sftpSplitRatio + step); }
        else if (e.key === "Home" || e.key === "Enter") { e.preventDefault(); settings.prefs.sftpSplitRatio = 0.5; }
      }}
    ></div>

    <div class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden border-l border-line">
      {@render filepane(right)}
    </div>
  </div>

  {#if edits.length}
    <div class="border-t border-line bg-panel">
      <div class="flex items-center justify-between px-3 py-1.5 text-xs text-fg-muted">
        <span class="font-medium uppercase tracking-wide">Editing</span>
        <span>Saves in your editor upload automatically. Stop to delete the local copy.</span>
      </div>
      {#each edits as ed (ed.id)}
        <div class="flex items-center gap-3 px-3 py-1.5 text-xs">
          <FilePen size={13} class="shrink-0 text-accent" />
          <span class="min-w-0 flex-1 truncate font-mono" title={ed.remote}>{ed.remote}</span>
          {#if ed.error}
            <span class="truncate text-danger" title={ed.error}>Upload failed</span>
          {:else if ed.lastUpload}
            <span class="text-success">Uploaded {ed.lastUpload.toLocaleTimeString()}</span>
          {:else}
            <span class="text-fg-muted">Open in editor</span>
          {/if}
          <button class="btn-ghost py-0.5 text-xs" onclick={() => stopEdit(ed.id)}>Stop</button>
        </div>
      {/each}
    </div>
  {/if}

  {#if transfers.length}
    <div class="border-t border-line bg-panel" data-testid="transfer-drawer">
      <div class="flex items-center justify-between gap-3 px-3 py-1.5 text-xs text-fg-muted">
        <button
          class="flex min-w-0 items-center gap-1.5 hover:text-fg"
          aria-expanded={transfersOpen}
          aria-controls="transfer-list"
          onclick={() => (transfersOpen = !transfersOpen)}
        >
          {#if transfersOpen}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}
          <span class="truncate font-medium uppercase tracking-wide">Transfers{running || queued ? ` · ${running} running${queued ? `, ${queued} queued` : ""}` : ""}</span>
          {#if totalBps > 0}<span class="shrink-0 font-mono normal-case tabular-nums text-accent">{speedText(totalBps)}</span>{/if}
        </button>
        <span class="flex gap-3">
          {#if transfers.some((t) => t.progress.state === "failed" || t.progress.state === "cancelled")}
            <button class="hover:text-fg" onclick={() => transfers.filter((t) => t.progress.state === "failed" || t.progress.state === "cancelled").forEach(retry)}>Retry all</button>
          {/if}
          <button class="hover:text-fg" onclick={() => (transfers = transfers.filter((t) => active(t.progress) || t.progress.state === "queued"))}>Clear finished</button>
        </span>
      </div>
      {#if !transfersOpen && running}
        <div class="h-0.5 bg-base"><div class="h-full bg-accent transition-[width]" style:width="{overall}%"></div></div>
      {/if}
    </div>
    <div id="transfer-list" class="max-h-48 overflow-y-auto bg-panel" hidden={!transfersOpen}>
      {#each transfers as t (t.id)}
        <div class="flex items-center gap-3 px-3 py-1.5 text-xs" data-testid="transfer-row" data-relay={t.relay}>
          <span class="w-4 text-fg-muted" title={t.relay ? "Host to host" : t.toLocal ? "Download" : "Upload"}>{t.relay ? "⇄" : t.toLocal ? "↓" : "↑"}</span>
          <div class="min-w-0 flex-1">
            <div class="flex justify-between gap-2">
              <span class="truncate">{t.label}</span>
              <span class="shrink-0 text-fg-muted">
                {#if t.progress.state === "progress"}
                  <span class="font-mono tabular-nums">{formatBytes(t.progress.bytes)} / {formatBytes(t.progress.total_bytes)}</span> · {t.progress.files_done}/{t.progress.total_files} files{#if t.speed?.bps}{" · "}<span class="font-mono tabular-nums text-accent">{speedText(t.speed.bps)}</span>{@const eta = formatEta(t.progress.total_bytes - t.progress.bytes, t.speed.bps)}{#if eta}{" · "}<span class="font-mono tabular-nums">{eta}</span>{/if}{/if}
                {:else if t.progress.state === "done"}
                  <span class="text-success">Done · {formatBytes(t.progress.bytes)}</span>
                {:else if t.progress.state === "failed"}
                  <span class="text-danger" title={t.progress.message}>Failed</span>
                {:else if t.progress.state === "cancelled"}
                  Cancelled
                {:else if t.progress.state === "paused"}
                  <span class="text-warning">Paused · {formatBytes(t.progress.bytes)} / {formatBytes(t.progress.total_bytes)}</span>
                {:else if t.progress.state === "queued"}
                  Queued
                {:else}
                  Scanning…
                {/if}
              </span>
            </div>
            {#if t.relay}
              <!-- Host to host: what moves goes from one connection into the other in memory, never to this computer's disk. -->
              <div class="mt-0.5 flex min-w-0 items-center gap-1.5 font-mono text-[11px] tabular-nums text-fg-muted" data-testid="relay-line">
                <span class="max-w-[10rem] truncate text-fg" title={t.fromLabel}>[{t.fromLabel}]</span>
                <span aria-hidden="true">──</span>
                <span class="shrink-0 {t.speed?.bps ? 'text-accent' : ''}">({t.speed?.bps ? speedText(t.speed.bps) : "—"})</span>
                <span aria-hidden="true">──▸</span>
                <span class="shrink-0">[Relay buffer]</span>
                <span aria-hidden="true">──▸</span>
                <span class="max-w-[10rem] truncate text-fg" title={t.toLabel}>[{t.toLabel}]</span>
              </div>
            {/if}
            <div class="mt-1 h-1 overflow-hidden rounded bg-base">
              <div
                class="h-full transition-[width] {t.progress.state === 'failed' ? 'bg-danger' : t.progress.state === 'done' ? 'bg-success' : 'bg-accent'}"
                style:width="{pct(t.progress)}%"
              ></div>
            </div>
            {#if t.progress.state === "failed"}
              <div class="mt-0.5 truncate text-danger" title={t.progress.message}>{t.progress.message}</div>
            {/if}
          </div>
          {#if t.progress.state === "paused"}
            <button class="icon-btn h-6 w-6" title="Resume" onclick={() => sftp.resume(t.id)}><Play size={13} /></button>
          {:else if t.progress.state === "progress" || t.progress.state === "started"}
            <button class="icon-btn h-6 w-6" title="Pause" onclick={() => sftp.pause(t.id)}><Pause size={13} /></button>
          {/if}
          {#if t.progress.state === "failed" || t.progress.state === "cancelled"}
            <button class="icon-btn h-6 w-6" title="Retry (continues partial files)" onclick={() => retry(t)}><RotateCcw size={13} /></button>
          {/if}
          {#if active(t.progress) || t.progress.state === "queued"}
            <button class="icon-btn h-6 w-6 hover:text-danger" title="Cancel" onclick={() => cancel(t)}><CircleStop size={13} /></button>
          {:else}
            <button class="icon-btn h-6 w-6" title="Dismiss" onclick={() => (transfers = transfers.filter((x) => x.id !== t.id))}><X size={13} /></button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if asking}
  <ConflictDialog
    src={asking.src}
    dst={asking.dst}
    srcSession={asking.srcSession}
    dstSession={asking.dstSession}
    srcLabel={asking.srcLabel}
    dstLabel={asking.dstLabel}
    remaining={asking.remaining}
    onanswer={(decision, all) => answered({ decision, all })}
    oncancel={() => answered(null)}
  />
{/if}

{#if editor}
  {@const ed = editor}
  <RemoteEditor sessionId={ed.conn.id} path={ed.path} hostId={ed.hostId} onclose={() => (editor = null)} onsaved={() => ed.conn.refresh++} />
{/if}
