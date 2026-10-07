<script lang="ts">
  import { keepInView } from "$lib/actions";
  import { onDestroy, onMount } from "svelte";
  import { registerTerminal } from "$lib/terminalsearch";
  import { matchLine, sanitize, triggered } from "$lib/highlight";
  import { MAX_BLOCKS, failed, nextMatching, summary, trim, visibleBlocks, type Block } from "$lib/commandblocks";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ChevronDown, ChevronUp, KeyRound, Loader2, MousePointer2, RefreshCw, Search, ShieldAlert, ShieldX, Unplug, X } from "lucide-svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import { ssh, type Credentials, type SessionStatus } from "$lib/ssh";
  import { closePane, markLocal, markRaw, resizePane, writeToPane } from "$lib/terminalio";
  import { contextFor, hostContextFor, runSnippet } from "$lib/runsnippet";
  import { render } from "$lib/snippetvars";
  import { ask } from "$lib/dialogs.svelte";
  import { connectionLog } from "$lib/stores/connectionlog.svelte";
  import { findHostPortMatches, findPathMatches, resolveBrowsePath, withoutLocation } from "$lib/termlinks";
  import { judgeLink } from "$lib/linksafety";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { ImageAddon } from "@xterm/addon-image";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { DEFAULT_PREFS, settings } from "$lib/stores/settings.svelte";
  import { localShells } from "$lib/stores/localshells.svelte";
  import { adhocLabel, ui, type Pane } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { allThemes, themeById } from "$lib/themes";
  import { effectiveLook } from "$lib/terminalprofile";
  import { mix } from "$lib/themeimport";
  import { errorMessage, isApiError } from "$lib/types";
  import { LineTracker, looksLikeSecret, matchDestructive, pastedLines, pasteNeedsConfirm } from "$lib/guard";
  import { CommandTracker, parseOsc133, parseOsc7 } from "$lib/shellintegration";
  import { InputWatch, completionLine, policyFor } from "$lib/completion";
  import { completionBridge, type InlineControls } from "$lib/completion/bridge";
  import { cursorCell, ghostBox, nextWord, type Ghost } from "$lib/completion/ghost";
  import { hostItems } from "$lib/completion/hosts";
  import { MAX_HISTORY_ITEMS, MENU_WIDTH, flatten, groups, historyItems, menuHeight, placeMenu, snippetItems, specItems, type MenuGroup, type MenuItem, type Placement } from "$lib/completion/menu";
  import { completeLine, parseLine } from "$lib/completion/command";
  import { loadSpec, specs } from "$lib/completion/specs";
  import { arrange, escapeName, pathQuestion, RemoteLookup } from "$lib/completion/remote";
  import CompletionMenu from "./CompletionMenu.svelte";
  import { completionHistory, entriesFromHistoryLines, seededHosts } from "$lib/completion/history";
  import CompletionOverlay from "./CompletionOverlay.svelte";
  import { MOUSE_OFF, RESET_INPUT_MODES, RESET_SESSION_MODES, decodeOsc52, mouseReportAllowed, redundantMouseEnable } from "$lib/termprotocol";
  import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

  let {
    pane,
    active = true,
    broadcast = null,
  }: {
    pane: Pane;
    active?: boolean;
    /** When set, input goes here (to every pane in the tab) instead of just this pane. */
    broadcast?: ((data: string | Uint8Array) => void) | null;
  } = $props();

  // Target and id never change for a mounted pane (panes are keyed by id).
  // svelte-ignore state_referenced_locally
  const target = pane.target;
  // svelte-ignore state_referenced_locally
  const paneId = pane.id;
  const host = $derived(target.kind === "host" ? vaultStore.hostById.get(target.hostId)?.data : undefined);
  const label = $derived(
    target.kind === "host"
      ? (host?.label ?? "host")
      : target.kind === "adhoc"
        ? adhocLabel(target.adhoc)
        : target.kind === "telnet"
          ? `${target.host}:${target.port}`
          : target.kind === "serial"
            ? target.config.path
            : "Local",
  );
  const telnetHost = $derived(target.kind === "host" && host?.protocol === "telnet");
  /** Telnet or serial: plain byte streams without SSH authentication. */
  const isRaw = $derived(target.kind === "telnet" || target.kind === "serial" || telnetHost);
  /** Set after "Use plain SSH": this pane skips Mosh from then on. */
  let plainSsh = $state(false);
  /** The last Mosh attempt failed because the host has no mosh-server. */
  let moshServerMissing = $state(false);
  const useMosh = $derived(target.kind === "host" && !!host?.mosh && !telnetHost && !plainSsh);
  const needsCredentials = $derived(target.kind === "host" && !!host && !telnetHost && !vaultStore.effectiveIdentity(host));
  const production = $derived(vaultStore.effectiveEnv(host) === "production");
  const shared = $derived(vaultStore.settings?.settings);

  // -- input guards (a safety net, not a guarantee) -------------------------
  const tracker = new LineTracker();
  type Pending =
    | { kind: "paste"; text: string; lines: number; danger: string | null; secretWarning: string | null }
    | { kind: "command"; line: string; pattern: string };
  let pending = $state<Pending | null>(null);

  function send(d: string | Uint8Array) {
    if (broadcast) broadcast(d);
    else void writeToPane(pane, d);
  }

  /** Typed input: hold Enter on production when the line looks destructive. */
  function typed(d: string) {
    // Without shell integration, history comes from what was typed.
    if ((d === "\r" || d === "\n") && !commands.active && tracker.line) recordTyped(tracker.line);
    if (production && shared && (d === "\r" || d === "\n")) {
      const line = tracker.line;
      const pattern = line === null ? null : matchDestructive(line, shared.destructive_patterns);
      if (line !== null && pattern) {
        pending = { kind: "command", line, pattern };
        return;
      }
    }
    tracker.feed(d);
    send(d);
    refreshSuggestion();
  }

  /** Every paste path (menu, shortcut, native Ctrl+V) goes through here. */
  function guardedPaste(text: string) {
    if (!text || status.kind !== "connected") return;
    if (settings.prefs.trimPasteNewline) text = text.replace(/\r\n$|\r$|\n$/, "");
    if (!text) return;
    const lines = pastedLines(text);
    const danger = production && shared ? text.split(/\r\n|\r|\n/).map((l) => matchDestructive(l, shared.destructive_patterns)).find(Boolean) ?? null : null;
    const secretWarning = looksLikeSecret(text);
    if (danger || secretWarning || pasteNeedsConfirm(text, shared?.paste_confirm_lines ?? 2, production)) {
      pending = { kind: "paste", text, lines, danger, secretWarning };
      return;
    }
    doPaste(text);
  }

  function doPaste(text: string) {
    // term.paste handles bracketed-paste mode and newline normalisation.
    tracker.reset();
    term.paste(text);
  }

  function resolvePending(go: boolean) {
    const p = pending;
    pending = null;
    if (go && p?.kind === "paste") doPaste(p.text);
    else if (go && p?.kind === "command") {
      tracker.reset();
      send("\r");
    }
    term?.focus();
  }

  let container: HTMLDivElement;
  let screenWatch: ResizeObserver | undefined;
  // Created once in onMount; the appearance effect re-reads it on each run.
  // svelte-ignore non_reactive_update
  let term: Terminal;
  let fit: FitAddon;
  let search: SearchAddon;
  let unlisten: UnlistenFn | null = null;
  let resizeObserver: ResizeObserver | null = null;

  let status = $state<SessionStatus>({ kind: "disconnected", code: null });
  let hostKeyNotices = $state<{ host: string; fingerprint: string }[]>([]);
  let started = $state(false);
  let username = $state("");
  let password = $state("");
  let remember = $state(true);
  let credentialError = $state<string | null>(null);

  // -- shell integration (OSC 7 and 133) --------------------------------------
  const commands = new CommandTracker((line) => term.buffer.active.getLine(line)?.translateToString(true) ?? "");
  const cursor = () => ({ line: term.buffer.active.baseY + term.buffer.active.cursorY, col: term.buffer.active.cursorX });
  const hostId = target.kind === "host" ? target.hostId : null;
  const logKind = target.kind === "adhoc" ? "adhoc" : target.kind;
  let logId: string | null = null;
  /** Long enough that the user probably switched away while it ran. */
  const NOTIFY_AFTER_MS = 8000;

  function onMark(data: string): boolean {
    const mark = parseOsc133(data);
    if (!mark) return false;
    const rec = commands.feed(mark, cursor());
    watch.feed(mark, cursor(), term.cols);
    // A shell is drawing its prompt, so the program that wanted the mouse is gone (it crashed or was killed
    // without switching it off): stop reporting, or the reports are typed into the shell.
    if (mark.kind === "prompt" && term.modes.mouseTrackingMode !== "none") term.write(MOUSE_OFF);
    if (rec) recordFinished(rec);
    if (rec) addBlock(rec);
    if (mark.kind === "prompt" || mark.kind === "end") refreshSuggestion();
    if (rec && rec.endedAt - rec.startedAt >= NOTIFY_AFTER_MS) {
      void notifyDone(`${rec.command.split("\n")[0].slice(0, 80)} finished${rec.exit ? ` (exit ${rec.exit})` : ""} on ${label}`);
    }
    if (mark.kind === "command") tracker.reset();
    if ((ui.paneInfo[paneId]?.running ?? false) !== commands.running) {
      ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), running: commands.running };
    }
    return true;
  }


  // -- smart completion: the faint suggestion after the cursor ------------------
  const watch = new InputWatch();
  const hostKey = hostId ?? target.kind;
  const policy = $derived(policyFor(settings.prefs, host, vaultStore.effectiveEnv(host)));
  let ghost = $state<Ghost | null>(null);
  /** What the suggestion would add to the line, or null. */
  let suggestion: string | null = null;
  /** The line text Esc was pressed on; no suggestion for exactly that until the line changes. */
  let dismissedAt: string | null = null;
  /** The command that finished last, which suggestions are ranked against. */
  let lastCommand: string | null = null;
  let refreshQueued = false;

  function indexCommand(command: string, exit: number | null, leadingSpace: boolean) {
    const cwd = ui.paneInfo[paneId]?.cwd;
    if (policy.history) {
      completionHistory.add(hostKey, { command, at: Date.now(), exit, cwd, prev: lastCommand ?? undefined, leadingSpace });
    }
    if (hostId) settings.recordCommand(hostId, command, exit, { cwd, prev: lastCommand ?? undefined, leadingSpace });
    lastCommand = command;
  }

  function recordFinished(rec: { command: string; exit: number | null; leadingSpace: boolean }) {
    indexCommand(rec.command, rec.exit, rec.leadingSpace);
  }

  /** Without shell integration, the line that was typed, if the screen agrees it is a command and not a password. */
  function recordTyped(line: string) {
    if (policy.enabled) {
      const seen = completionLine(policy, term, watch, line);
      if (!seen || seen.text !== line) return;
    }
    indexCommand(line, null, false);
  }

  function hideGhost() {
    suggestion = null;
    if (ghost) ghost = null;
  }

  /** The line as typed, when a suggestion or the list may be offered for it right now; else null. */
  function offerableLine() {
    if (!term || status.kind !== "connected" || pending || !active) return null;
    const buf = term.buffer.active;
    // Scrolled back: the cursor row isn't on screen.
    if (buf.viewportY !== buf.baseY) return null;
    const line = completionLine(policy, term, watch, tracker.line);
    return line && line.cursorAtEnd ? line : null;
  }

  function refreshNow() {
    refreshQueued = false;
    const line = offerableLine();
    if (popup) {
      hideGhost();
      if (!line || !policy.menu) return closeMenu();
      return updateMenu(line.text);
    }
    // With a screen reader the terminal is read aloud as it changes; faint text would only be noise.
    if (!policy.inline || settings.prefs.screenReaderMode || !line || !line.text.trim()) return hideGhost();
    if (line.text === dismissedAt) return hideGhost();
    dismissedAt = null;
    const add = completionHistory.suggest({ text: line.text, host: hostKey, cwd: ui.paneInfo[paneId]?.cwd, prev: lastCommand });
    const m = measure();
    if (!add || !m) return hideGhost();
    const box = ghostBox({ ...m, suggestion: add });
    if (!box) return hideGhost();
    suggestion = add;
    ghost = box;
  }

  /** The terminal's grid and the cursor on it, in the overlay's coordinates. */
  function measure() {
    const screen = container.querySelector<HTMLElement>(".xterm-screen");
    if (!screen || !ghostRoot) return null;
    const buf = term.buffer.active;
    return {
      screen: screen.getBoundingClientRect(),
      root: ghostRoot.getBoundingClientRect(),
      cols: term.cols,
      rows: term.rows,
      cursorX: buf.cursorX,
      cursorY: buf.cursorY,
    };
  }

  /** Once per frame at most, however much output arrives. */
  function refreshSuggestion() {
    if (refreshQueued) return;
    // Off: nothing is read, nothing is drawn.
    if (!policy.inline && !ghost && !popup) return;
    refreshQueued = true;
    requestAnimationFrame(refreshNow);
  }

  function takeSuggestion(part: (s: string) => string): boolean {
    if (!suggestion || !ghost) return false;
    const text = part(suggestion);
    hideGhost();
    if (!text) return false;
    // The normal input path: guards, the line tracker and broadcast all see it like typed keys.
    typed(text);
    return true;
  }

  // -- the popup list --------------------------------------------------------
  let popup = $state<{ groups: MenuGroup[]; items: MenuItem[]; selected: number; place: Placement; text: string } | null>(null);

  /** `keepWaiting`: the list is empty only until the host answers, and should open then. */
  function closeMenu(keepWaiting = false) {
    popup = null;
    remoteFor = null;
    if (!keepWaiting) waitingFor = null;
  }

  // -- file names and lists from the host --------------------------------------
  /** Only a pane that is an SSH session has a connection to ask over; Mosh, Telnet, serial and local tabs don't. */
  const askable = () => (target.kind === "host" && !useMosh && !telnetHost) || target.kind === "adhoc";
  /** A shell on this computer (not a Windows one, whose paths are not the Unix kind the lookup speaks): folders only. */
  const localShell = target.kind === "local" && !/Windows/i.test(navigator.userAgent);
  const remote = new RemoteLookup({
    fetch: (req) => (localShell ? api.completion.lookupLocal(req) : api.completion.lookup(paneId, req)),
    // The setting is read at the moment of asking, so turning it off stops a question already waiting.
    enabled: () => policy.remotePaths && (askable() || localShell) && status.kind === "connected",
  });
  /** Once per host per run, with the person's consent: the commands already in the host's own history files. */
  async function seedFromHost() {
    if (!hostId || !policy.seedHistory || seededHosts.has(hostId)) return;
    seededHosts.add(hostId);
    const r = await remote.generator("shell-history");
    if (!r || !policy.history) return;
    completionHistory.seed(hostKey, entriesFromHistoryLines(r.entries.map((e) => e.name), Date.now()));
  }
  /** The host's names for the line `text`, once they have arrived. */
  let remoteFor: { text: string; items: MenuItem[] } | null = null;
  /** The line a question to the host is out for, if any. Asking again for the same line waits for that answer. */
  let inFlight: string | null = null;

  const GENERATOR_TITLES: Record<string, string> = {
    "git-branches": "Branches",
    "git-tags": "Tags",
    "docker-containers": "Containers",
    "docker-images": "Images",
    "systemd-units": "Units",
    "kubectl-contexts": "Contexts",
    "kubectl-namespaces": "Namespaces",
    "kubectl-pods": "Pods",
  };

  /** Up to the last slash that is not escaped. */
  function rawDirectory(raw: string): string {
    for (let i = raw.length - 1; i >= 0; i--) {
      if (raw[i] !== "/") continue;
      let slashes = 0;
      for (let j = i - 1; j >= 0 && raw[j] === "\\"; j--) slashes++;
      if (slashes % 2 === 0) return raw.slice(0, i + 1);
    }
    return "";
  }

  /** Ask the host about the word being typed, and list what it says once it does. */
  function askHost(text: string, c: ReturnType<typeof completeLine>) {
    // A local tab only lists folders; the named lookups (branches, units, …) run on a host.
    if (!policy.remotePaths || !(askable() || localShell) || (!c.path && !(c.generator && !localShell))) return;
    const cwd = ui.paneInfo[paneId]?.cwd ?? null;
    const word = c.word;
    const erase = [...word.raw].length;
    if (inFlight === text) return;
    inFlight = text;
    const finish = () => {
      if (inFlight === text) inFlight = null;
    };
    const done = (title: string, items: MenuItem[], truncated: boolean) => {
      finish();
      if (!popup && waitingFor !== text) return;
      if (offerableLine()?.text !== text) return;
      remoteFor = { text, items: items.map((i) => ({ ...i, group: truncated ? `${title} · cut, keep typing` : title })) };
      if (popup) updateMenu(text);
      else if (waitingFor === text) {
        waitingFor = null;
        openMenu();
      }
    };
    if (c.generator && !localShell) {
      const generator = c.generator;
      void remote.generator(generator, cwd ?? undefined).then((r) => {
        if (!r) return finish();
        const names = r.entries.map((e) => e.name).filter((n) => n.startsWith(word.value) && n !== word.value).slice(0, 12);
        done(
          GENERATOR_TITLES[generator] ?? "Names",
          names.map((n) => ({ id: `g:${generator}:${n}`, kind: "value", group: "", label: n, labelHit: Array.from({ length: [...word.value].length }, (_, i) => i), detail: "", detailHit: [], insert: escapeName(n) + " ", variables: false, erase })),
          r.truncated,
        );
      });
      return;
    }
    const q = pathQuestion(word.value, cwd);
    if (!q) return finish();
    const dirRaw = rawDirectory(word.raw);
    void remote.dir(q.dir, q.prefix).then((r) => {
      if (!r) return finish();
      const shown = arrange(r.entries, q.prefix, c.path === "dir").slice(0, 12);
      done(
        c.path === "dir" ? "Folders" : "Files",
        shown.map((e) => ({
          id: `p:${q.dir}/${e.name}`,
          kind: "value" as const,
          group: "",
          label: e.dir ? `${e.name}/` : e.name,
          labelHit: Array.from({ length: [...q.prefix].length }, (_, i) => i),
          detail: e.dir ? "folder" : "",
          detailHit: [],
          // Folders keep the path going; a file ends the word.
          insert: dirRaw + escapeName(e.name, dirRaw === "") + (e.dir ? "/" : " "),
          variables: false,
          erase,
        })),
        r.truncated,
      );
    });
  }

  function menuGroups(text: string): MenuGroup[] {
    const history = historyItems(text, completionHistory.search(text, hostKey, MAX_HISTORY_ITEMS + 1));
    const snippets = policy.snippets ? snippetItems(text, vaultStore.snippets.map((r) => ({ id: r.id, label: r.data?.label ?? "", command: r.data?.command ?? "", description: r.data?.description, abbreviation: r.data?.abbreviation }))) : [];
    let commands: MenuItem[] = [];
    if (policy.options || policy.remotePaths) {
      const c = completeLine(text, specs);
      // The spec for this command is fetched the first time it is typed; the list fills in when it arrives.
      if (c.missing) {
        specLoading = true;
        void loadSpec(c.missing).then(() => {
          const waited = waitingFor;
          waitingFor = null;
          if (popup) refreshSuggestion();
          // Asked for the list on a line only this spec could fill: open it now (which may need the next spec, for sudo git).
          else if (waited !== null && waited === offerableLine()?.text) openMenu();
        });
      }
      else {
        if (policy.options) commands = specItems(c.word.raw, c.items);
        if (remoteFor?.text !== text) askHost(text, c);
      }
    }
    const fromHost = remoteFor?.text === text ? remoteFor.items : [];
    // `ssh we` offers the saved hosts; they live in the vault, so nothing is asked of any host for this.
    const vaultHosts = policy.options
      ? hostItems(text, vaultStore.hosts.filter((r) => !r.deleted && r.data).map((r) => ({ id: r.id, label: r.data!.label, hostname: r.data!.hostname, port: r.data!.port, protocol: r.data!.protocol, username: r.data!.identity_id ? vaultStore.identityById.get(r.data!.identity_id)?.data?.username : undefined })))
      : [];
    // What the command's own spec says comes first, then the host's own names; both are more specific than history.
    return groups([...vaultHosts, ...commands, ...fromHost, ...history, ...snippets]);
  }

  /** Rebuild the list for the line as it is now; the choice stays on the same entry if that is still listed. */
  function updateMenu(text: string, keep?: string) {
    const gs = menuGroups(text);
    const items = flatten(gs);
    const m = measure();
    const anchor = m && cursorCell(m);
    if (items.length === 0 && inFlight === text && m && anchor) {
      waitingFor = text;
      return closeMenu(true);
    }
    if (items.length === 0 || !m || !anchor) return closeMenu();
    const kept = keep ?? (popup && popup.text === text ? popup.items[popup.selected]?.id : undefined);
    const at = kept ? items.findIndex((i) => i.id === kept) : -1;
    const rootBox = m.root;
    // Inside the pane and inside the window, whichever is smaller.
    const left = Math.max(rootBox.left, 0);
    const top = Math.max(rootBox.top, 0);
    const right = Math.min(rootBox.left + rootBox.width, window.innerWidth);
    const bottom = Math.min(rootBox.top + rootBox.height, window.innerHeight);
    const place = placeMenu(
      { left: anchor.left, top: anchor.top, width: anchor.cellWidth, height: anchor.cellHeight },
      { width: MENU_WIDTH, height: menuHeight(gs) },
      { left: left - rootBox.left, top: top - rootBox.top, width: right - left, height: bottom - top },
    );
    popup = { groups: gs, items, selected: at >= 0 ? at : 0, place, text };
    menuAnnouncement = `${items.length} suggestion${items.length === 1 ? "" : "s"}. ${items[popup.selected].label}, 1 of ${items.length}.`;
  }

  /** A spec was being fetched during the last build of the list; and the line the list was asked for while it was. */
  let specLoading = false;
  let waitingFor: string | null = null;

  function openMenu(): boolean {
    if (!policy.menu) return false;
    const line = offerableLine();
    if (!line) return false;
    hideGhost();
    specLoading = false;
    updateMenu(line.text);
    const waiting = specLoading || inFlight === line.text;
    if (!popup && waiting) waitingFor = line.text;
    // Nothing to show yet counts as handled: the key was for us, and the list comes when the spec or the host does.
    return popup !== null || waiting;
  }

  function moveMenu(by: 1 | -1) {
    if (!popup) return;
    const n = popup.items.length;
    popup.selected = (popup.selected + by + n) % n;
    menuAnnouncement = `${popup.items[popup.selected].label}, ${popup.selected + 1} of ${n}.`;
  }

  /** Put the chosen entry on the line in place of what was typed. It is typed, not run: Enter stays the person's. */
  function pickMenu(index: number) {
    const m = popup;
    const item = m?.items[index];
    if (!m || !item) return;
    closeMenu();
    const erase = item.erase ?? [...m.text].length;
    if (item.variables) {
      // The variable dialog asks for the values, then puts the text in place of what was typed.
      tracker.reset();
      void runSnippet(item.insert, { execute: false, scope: "pane", erase });
    } else {
      const text = item.kind === "snippet" ? render(item.insert, contextFor(target), {}) : item.insert;
      typed("\x7f".repeat(erase) + text);
    }
    term.focus();
  }

  /** What a screen reader is told about the list: how many entries, and the one chosen. */
  let menuAnnouncement = $state("");

  const controls: InlineControls = {
    accept: () => takeSuggestion((s) => s),
    acceptWord: () => takeSuggestion(nextWord),
    dismiss: () => {
      if (popup) {
        closeMenu();
        return true;
      }
      if (!suggestion) return false;
      dismissedAt = completionLine(policy, term, watch, tracker.line)?.text ?? null;
      hideGhost();
      return true;
    },
    openMenu,
    menuKey: (key) => {
      if (!popup) return false;
      if (key === "choose") pickMenu(popup.selected);
      else moveMenu(key === "down" ? 1 : -1);
      return true;
    },
  };

  // Registered while this pane has the keyboard and smart completion is on; off, no key reaches this code.
  $effect(() => {
    if (active && policy.enabled) completionBridge.set(controls);
    else {
      completionBridge.clear(controls);
      hideGhost();
      closeMenu();
    }
  });

  let ghostRoot: HTMLDivElement | undefined;

  /** The whole connection error, with a Copy button; the bar only has room for a line or two. */
  async function showError() {
    if (status.kind !== "error") return;
    const message = status.message;
    if (await ask(message, { title: "Connection error", confirm: "Copy" })) {
      try {
        await writeText(message);
      } catch {
        // Nothing to do; the text is on screen.
      }
    }
  }

  async function notifyDone(body: string) {
    if (!settings.prefs.notifyBackground) return;
    if (active && document.hasFocus() && !document.hidden) return;
    try {
      let ok = await isPermissionGranted();
      if (!ok) ok = (await requestPermission()) === "granted";
      if (ok) sendNotification({ title: "SSHVault", body });
    } catch {
      // No notification service; nothing to do.
    }
  }

  /** Select the last command's output and copy it. */
  async function copyLastOutput() {
    const last = commands.last;
    if (!last || last.outputStart === null || last.outputEnd === null) return ui.notify("error", "No command output recorded yet.");
    term.selectLines(last.outputStart, last.outputEnd);
    const text = term.getSelection();
    term.clearSelection();
    if (!text.trim()) return ui.notify("info", "The last command printed nothing.");
    try {
      await writeText(text);
      ui.notify("info", `Copied the output of "${last.command.split("\n")[0].slice(0, 40)}".`);
    } catch (e) {
      ui.notify("error", `Copy failed: ${errorMessage(e)}`);
    }
  }

  async function copyEntireBuffer() {
    const buf = term.buffer.active;
    const lines: string[] = [];
    for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? "");
    // Trailing blank lines are just unused scrollback rows, not real output.
    while (lines.length && !lines[lines.length - 1]) lines.pop();
    const text = lines.join("\n");
    if (!text) return ui.notify("info", "Nothing in the buffer yet.");
    try {
      await writeText(text);
      ui.notify("info", `Copied ${lines.length} line${lines.length === 1 ? "" : "s"} of buffer.`);
    } catch (e) {
      ui.notify("error", `Copy failed: ${errorMessage(e)}`);
    }
  }

  function jumpPrompt(dir: -1 | 1) {
    const here = term.buffer.active.viewportY;
    const line = commands.nearestPrompt(dir === -1 ? here : here + 1, dir);
    if (line !== null) term.scrollToLine(line);
  }

  // Reconnect by itself after a dropped connection (never after `exit`).
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  let reconnectAttempts = 0;
  const MAX_RECONNECTS = 3;

  function scheduleReconnect() {
    if (target.kind === "local" || target.kind === "serial" || !settings.prefs.autoReconnect || needsCredentials || reconnectAttempts >= MAX_RECONNECTS) return;
    reconnectAttempts += 1;
    const secs = 2 * reconnectAttempts;
    term.write(`\r\n\x1b[90m[connection lost; reconnecting in ${secs} s (${reconnectAttempts}/${MAX_RECONNECTS})…]\x1b[0m\r\n`);
    reconnectTimer = setTimeout(() => void connect(null), secs * 1000);
  }

  let findOpen = $state(false);
  let findQuery = $state("");
  let findInput = $state<HTMLInputElement>();
  let menu = $state<{ x: number; y: number } | null>(null);
  // Scrolled back into the buffer, with output since: offer a way back down.
  let scrolledUp = $state(false);
  let newOutputWhileScrolled = $state(false);

  /** The chosen theme, with a red cast on production hosts if enabled. */
  /** The settings' look, with this host's own theme, size and scrollback over it. */
  function look() {
    const p = settings.prefs;
    const known = new Set(allThemes(p.customThemes).map((t) => t.id));
    return effectiveLook(p, host?.profile, DEFAULT_PREFS.fontSize, (id) => known.has(id));
  }

  function paneTheme() {
    const base = themeById(look().themeId, settings.prefs.customThemes).theme;
    const tinted = production && settings.prefs.prodTint && base.background ? { ...base, background: mix(base.background, "#ff0000", 0.08) } : base;
    return settings.prefs.cursorColor ? { ...tinted, cursor: settings.prefs.cursorColor } : tinted;
  }

  function setInfo(s: SessionStatus["kind"]) {
    ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? {}), status: s };
  }

  // -- mouse: programs like tmux, vim and htop take over the mouse -----------
  //
  // While a program tracks the mouse, xterm.js hands clicks and drags to it
  // and only selects text with Shift held. "Select mode" makes plain drags
  // select (and copy) again by re-sending them as Shift-drags, which xterm
  // treats as forced selection and doesn't forward to the program.
  const mouseTracked = $derived(ui.paneInfo[paneId]?.mouseTracked ?? false);
  const selectMode = $derived(ui.paneInfo[paneId]?.selectMode ?? false);
  const overrideMouse = $derived(selectMode && mouseTracked);

  function toggleSelectMode() {
    ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), selectMode: !selectMode };
    term?.focus();
  }

  function refreshModes() {
    const tracked = term.modes.mouseTrackingMode !== "none";
    if ((ui.paneInfo[paneId]?.mouseTracked ?? false) !== tracked) {
      ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), mouseTracked: tracked };
    }
  }

  function forceSelection(e: MouseEvent) {
    if (!overrideMouse || e.shiftKey || e.button !== 0) return;
    e.stopImmediatePropagation();
    e.preventDefault();
    e.target?.dispatchEvent(new MouseEvent(e.type, { ...eventInit(e), shiftKey: true }));
  }

  function eventInit(e: MouseEvent): MouseEventInit {
    return {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: e.view,
      detail: e.detail,
      screenX: e.screenX,
      screenY: e.screenY,
      clientX: e.clientX,
      clientY: e.clientY,
      ctrlKey: e.ctrlKey,
      altKey: e.altKey,
      metaKey: e.metaKey,
      button: e.button,
      buttons: e.buttons,
      relatedTarget: e.relatedTarget,
    };
  }

  /**
   * Full-screen programs (tmux with the mouse off, less, vim) have no
   * scrollback of their own to show, so xterm.js ignores the wheel there.
   * Turn each notch into arrow keys, like other terminals' "alternate
   * scroll", so the program scrolls instead.
   */
  // -- Ctrl+scroll zoom ---------------------------------------------------------
  // Trackpads send many small deltas; add them up to one notch per step.
  const ZOOM_NOTCH = 60;
  let zoomAccum = 0;
  let zoomHint = $state<number | null>(null);
  let zoomHintTimer: ReturnType<typeof setTimeout> | undefined;

  function wheelZoom(e: WheelEvent) {
    e.preventDefault();
    // deltaMode 1 = lines (a mouse wheel on some systems): one line, one step.
    zoomAccum += e.deltaMode === 1 ? Math.sign(e.deltaY) * ZOOM_NOTCH : e.deltaY;
    while (Math.abs(zoomAccum) >= ZOOM_NOTCH) {
      const step = zoomAccum < 0 ? 1 : -1; // wheel up = bigger
      settings.zoom(step);
      zoomAccum -= -step * ZOOM_NOTCH;
    }
    zoomHint = settings.prefs.fontSize;
    clearTimeout(zoomHintTimer);
    zoomHintTimer = setTimeout(() => {
      zoomHint = null;
      zoomAccum = 0;
    }, 900);
  }

  function wheelToKeys(e: WheelEvent): boolean {
    // Ctrl (Cmd on macOS) + scroll changes the font size, in any pane.
    if (e.ctrlKey || e.metaKey) {
      wheelZoom(e);
      return false;
    }
    if (status.kind !== "connected" || term.buffer.active.type !== "alternate" || mouseTracked) return true;
    if (e.deltaY === 0) return true;
    // deltaMode 0 = pixels (about 40 per notch on most systems), 1 = lines.
    const lines = Math.max(1, Math.round(Math.abs(e.deltaY) / (e.deltaMode === 0 ? 40 : 1)));
    const key = e.deltaY < 0 ? (term.modes.applicationCursorKeysMode ? "\x1bOA" : "\x1b[A") : term.modes.applicationCursorKeysMode ? "\x1bOB" : "\x1b[B";
    send(key.repeat(Math.min(lines, 20)));
    e.preventDefault();
    return false;
  }

  function safeFit() {
    // Skip while hidden (another view is showing); xterm would otherwise
    // shrink to 0x0 and resize the remote PTY to nothing.
    if (container && container.clientWidth > 0 && container.clientHeight > 0) fit?.fit();
  }

  async function connect(credentials: Credentials | null) {
    if (target.kind === "host" && !host) return;
    started = true;
    hostKeyNotices = [];
    status = { kind: "connecting" };
    setInfo("connecting");
    // A new or restarted session starts with every mode off, whatever the last program left on.
    term.write(RESET_SESSION_MODES);
    term.clear();
    safeFit();
    const onData = (bytes: Uint8Array) => {
      term.write(bytes);
      if (!active && !ui.paneInfo[paneId]?.unread) {
        ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), unread: true };
      }
      if (scrolledUp) newOutputWhileScrolled = true;
    };
    try {
      markRaw(paneId, isRaw);
      markLocal(paneId, useMosh);
      moshServerMissing = false;
      if (target.kind === "telnet") {
        await api.raw.telnet(paneId, target.host, target.port, term.cols, term.rows, onData);
      } else if (target.kind === "serial") {
        await api.raw.serial(paneId, $state.snapshot(target.config) as typeof target.config, onData);
      } else if (target.kind === "host" && telnetHost && host) {
        await api.raw.telnet(paneId, host.hostname, host.port, term.cols, term.rows, onData);
      } else if (target.kind === "host" && useMosh) {
        await api.mosh.connect(paneId, target.hostId, term.cols, term.rows, credentials, onData);
      } else if (target.kind === "host") {
        await ssh.connect(paneId, target.hostId, term.cols, term.rows, credentials, onData);
      } else if (target.kind === "adhoc") {
        await ssh.connectAdhoc(paneId, target.adhoc, term.cols, term.rows, onData);
      } else {
        await localShells.ensure();
        // This tab's own shell, else your default if it is still installed, else the free-text command.
        const shellId = (target.kind === "local" ? target.shellId : undefined) ?? localShells.defaultId;
        await api.localTerm.spawn(paneId, term.cols, term.rows, onData, settings.prefs.localShell.trim() || null, settings.prefs.localCwd.trim() || null, shellId);
        // A command to start with (for example a container shell): give the shell a moment to print its prompt.
        const startup = target.kind === "local" ? target.command?.trim() : "";
        if (startup) setTimeout(() => void writeToPane(pane, startup + "\r"), 400);
      }
    } catch (e) {
      status = { kind: "error", message: errorMessage(e) };
      moshServerMissing = useMosh && isApiError(e) && (e.code === "mosh_server_missing" || e.code === "mosh_client_missing");
      setInfo("error");
    }
  }

  async function followLink(uri: string, shown: string) {
    const v = judgeLink(uri, shown);
    if (!v.ok) return void ui.notify("info", v.reason);
    if (v.confirm && !(await ask(`Open this address?\n\n${v.address}\n\nThe text on screen says “${shown.slice(0, 80)}”.`, { title: "Open link", confirm: "Open" }))) return;
    try {
      await openUrl(v.address);
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  function usePlainSsh() {
    plainSsh = true;
    void connect(null);
  }

  async function submitCredentials(e: SubmitEvent) {
    e.preventDefault();
    credentialError = null;
    const creds = { username, password };
    password = "";
    if (remember && target.kind === "host" && host) {
      // Store them as this host's own credentials, then connect normally.
      try {
        await vaultStore.saveHostWithCredentials(target.hostId, $state.snapshot(host), {
          mode: "inline",
          username: creds.username,
          auth: { type: "password", password: creds.password },
          save_to_keychain: null,
        });
        connect(null);
      } catch (err) {
        credentialError = errorMessage(err);
      }
      return;
    }
    connect(creds);
  }

  function reconnect() {
    if (needsCredentials) started = false;
    else connect(null);
  }


  // -- clipboard --------------------------------------------------------

  async function copySelection() {
    const sel = term.getSelection();
    if (!sel) return;
    try {
      await writeText(sel);
    } catch (e) {
      ui.notify("error", `Copy failed: ${errorMessage(e)}`);
    }
  }

  async function paste() {
    if (status.kind !== "connected") return;
    try {
      const text = await readText();
      if (text) guardedPaste(text);
    } catch (e) {
      ui.notify("error", `Paste failed: ${errorMessage(e)}`);
    }
  }

  // -- find ---------------------------------------------------------------

  /**
   * Read live rather than hard-coded, so a match highlight follows the
   * active app theme's accent colour (and light vs. dark) instead of
   * always being the dark theme's purple.
   */
  function searchDecorations() {
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--color-accent").trim() || "#7b61ff";
    const fg = getComputedStyle(document.documentElement).getPropertyValue("--color-fg").trim() || "#ffffff";
    return {
      matchBackground: `${accent}55`,
      activeMatchBackground: accent,
      matchOverviewRuler: accent,
      activeMatchColorOverviewRuler: fg,
    };
  }

  function openFind() {
    findOpen = true;
    const sel = term?.getSelection();
    if (sel && !sel.includes("\n")) findQuery = sel;
    queueMicrotask(() => findInput?.select());
  }

  function closeFind() {
    findOpen = false;
    search?.clearDecorations();
    term?.focus();
  }

  function findNext(backwards = false) {
    if (!findQuery) return;
    const opts = { decorations: searchDecorations(), incremental: false };
    if (backwards) search.findPrevious(findQuery, opts);
    else search.findNext(findQuery, opts);
  }

  // -- lifecycle ----------------------------------------------------------

  onMount(async () => {
    const p = settings.prefs;
    term = new Terminal({
      theme: paneTheme(),
      fontFamily: p.fontFamily,
      fontSize: look().fontSize,
      lineHeight: p.lineHeight,
      cursorStyle: p.cursorStyle,
      cursorBlink: p.cursorBlink,
      scrollback: look().scrollback,
      allowProposedApi: true,
      macOptionIsMeta: true,
      rightClickSelectsWord: false,
      // Links programs mark up themselves (OSC 8): only web and mail addresses are followed, and an address that
      // differs from the text shown is confirmed first, since the text can lie about where it goes.
      linkHandler: {
        allowNonHttpProtocols: false,
        activate: (e, uri, range) => {
          if (!e.ctrlKey && !e.metaKey) return;
          const shown = term.buffer.active.getLine(range.start.y - 1)?.translateToString(true, range.start.x - 1, range.end.x - 1) ?? uri;
          void followLink(uri, shown);
        },
      },
    });
    fit = new FitAddon();
    search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new WebLinksAddon());
    // Emoji and other wide characters take two cells, as programs expect; without this the text after one is misplaced.
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    // Pictures (Sixel and the iTerm2 protocol), within limits so a program can't fill the memory.
    if (settings.prefs.terminalImages) term.loadAddon(new ImageAddon({ sixelSupport: true, iipSupport: true, pixelLimit: 8_000_000, storageLimit: 32, showPlaceholder: false }));
    // Ctrl+click a path to browse to it in SFTP, or a host:port to quick
    // connect. Saved-host panes only: there's no SFTP target for a telnet,
    // serial or ad-hoc session, and no reliable remote cwd without one.
    term.registerLinkProvider({
      provideLinks(lineNumber, callback) {
        const lineText = term.buffer.active.getLine(lineNumber - 1)?.translateToString(true);
        if (!lineText) {
          callback(undefined);
          return;
        }
        const links: import("@xterm/xterm").ILink[] = [];
        if (hostId) {
          for (const m of findPathMatches(lineText)) {
            links.push({
              range: { start: { x: m.start + 1, y: lineNumber }, end: { x: m.end + 1, y: lineNumber } },
              text: m.text,
              activate: (e) => {
                if (!e.ctrlKey && !e.metaKey) return;
                ui.openSftpAt(hostId, resolveBrowsePath(withoutLocation(m.text), ui.paneInfo[paneId]?.cwd));
              },
            });
          }
        }
        for (const m of findHostPortMatches(lineText)) {
          links.push({
            range: { start: { x: m.start + 1, y: lineNumber }, end: { x: m.end + 1, y: lineNumber } },
            text: m.text,
            activate: (e) => {
              if (!e.ctrlKey && !e.metaKey) return;
              ui.modal = { kind: "quick-connect", initial: m.text };
            },
          });
        }
        callback(links.length ? links : undefined);
      },
    });
    term.open(container);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // Falls back to the DOM renderer.
    }
    safeFit();

    // Terminal-local shortcuts. Returning false stops xterm from sending
    // the key to the remote shell.
    term.attachCustomKeyEventHandler((e) => {
      // Shift+End jumps to the bottom, matching the "New output" pill's click.
      if (e.type === "keydown" && e.shiftKey && !e.ctrlKey && !e.altKey && e.code === "End") {
        term.scrollToBottom();
        return false;
      }
      if (e.type !== "keydown" || !e.ctrlKey || !e.shiftKey) return true;
      switch (e.code) {
        case "KeyC":
          void copySelection();
          return false;
        case "KeyV":
          void paste();
          return false;
        case "KeyF":
          openFind();
          return false;
        case "ArrowUp":
          jumpPrompt(-1);
          return false;
        case "ArrowDown":
          jumpPrompt(1);
          return false;
      }
      return true;
    });
    // A soft reset (CSI ! p) leaves xterm.js's mouse tracking and bracketed paste on; switch them off too,
    // then let xterm.js do the rest of the reset.
    term.parser.registerCsiHandler({ intermediates: "!", final: "p" }, () => {
      term.write(RESET_INPUT_MODES);
      return false;
    });
    term.parser.registerOscHandler(133, onMark);
    term.parser.registerOscHandler(7, (data) => {
      const cwd = parseOsc7(data);
      if (cwd !== null) ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), cwd };
      return cwd !== null;
    });
    // OSC 52: tmux (mouse selection with set-clipboard on), Claude Code and
    // Neovim copy by asking the terminal to set the clipboard. xterm.js
    // ignores it, which looked like "copying doesn't work" in those programs.
    // Queries ("?") are never answered.
    term.parser.registerOscHandler(52, (data) => {
      if (!settings.prefs.remoteClipboard) return true;
      const text = decodeOsc52(data);
      if (text) writeText(text).catch((e) => ui.notify("error", `Copy failed: ${errorMessage(e)}`));
      return true;
    });
    // Swallow DECSETs that only re-enable the mouse mode already in force.
    // xterm.js treats them as a protocol change, clears the selection and
    // ends the drag, so programs that re-send the mode while redrawing
    // (Claude Code, tmux) made Shift+drag and select mode unusable.
    term.parser.registerCsiHandler({ prefix: "?", final: "h" }, (params) => redundantMouseEnable(params, term.modes.mouseTrackingMode));
    term.onBell(() => {
      void notifyDone(`Bell from ${label}`);
      if (!active) ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), bell: true };
    });

    // Native paste (Ctrl+V, middle click): intercept before xterm sends it.
    container.addEventListener(
      "paste",
      (e) => {
        const text = e.clipboardData?.getData("text/plain") ?? "";
        e.preventDefault();
        e.stopImmediatePropagation();
        guardedPaste(text);
      },
      { capture: true },
    );

    for (const type of ["mousedown", "mouseup"] as const) {
      container.addEventListener(type, forceSelection, { capture: true });
    }
    term.attachCustomWheelEventHandler(wheelToKeys);
    term.onWriteParsed(refreshModes);
    term.onWriteParsed(scheduleHighlight);
    term.onScroll(scheduleHighlight);
    term.onWriteParsed(refreshSuggestion);
    // Font, zoom, padding and fitting all change the screen's size; the suggestion follows.
    const screenEl = container.querySelector(".xterm-screen");
    if (screenEl) {
      screenWatch = new ResizeObserver(refreshSuggestion);
      screenWatch.observe(screenEl);
    }
    if (hostId && settings.prefs.rememberCommands) completionHistory.seed(hostKey, settings.history[hostId] ?? []);

    term.onData((d) => {
      if (status.kind !== "connected" || pending) return;
      // A mouse report is only for a program that asked for them, and only while it is still there.
      if (!mouseReportAllowed(d, term.modes.mouseTrackingMode)) return;
      typed(d);
    });
    term.onBinary((d) => {
      if (status.kind !== "connected") return;
      // Legacy mouse reports for positions beyond column 223 come this way.
      if (!mouseReportAllowed(d, term.modes.mouseTrackingMode)) return;
      const bytes = Uint8Array.from(d, (c) => c.charCodeAt(0));
      if (broadcast) broadcast(bytes);
      else void writeToPane(pane, bytes);
    });
    term.onResize(({ cols, rows }) => {
      ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), cols, rows };
      if (status.kind === "connected") void resizePane(pane, cols, rows);
    });
    term.onScroll((y) => {
      refreshSuggestion();
      scrolledUp = y < term.buffer.active.baseY;
      if (!scrolledUp) newOutputWhileScrolled = false;
    });
    term.onSelectionChange(() => {
      if (settings.prefs.copyOnSelect && term.hasSelection()) void copySelection();
    });
    term.onTitleChange((title) => {
      ui.paneInfo[paneId] = { ...(ui.paneInfo[paneId] ?? { status: status.kind }), remoteTitle: title };
    });

    resizeObserver = new ResizeObserver(safeFit);
    resizeObserver.observe(container);

    // Lets "Search all terminals" read this terminal's lines and bring one into view.
    unregisterSearch = registerTerminal({
      paneId,
      label: host?.label ?? (ui.paneInfo[paneId]?.remoteTitle || "Terminal"),
      lines: () => {
        const buf = term.buffer.active;
        const out: string[] = [];
        for (let i = 0; i < buf.length; i++) out.push(buf.getLine(i)?.translateToString(true) ?? "");
        return out;
      },
      reveal: (line) => {
        term.scrollToLine(Math.max(0, line - Math.floor(term.rows / 2)));
        term.select(0, line, term.cols);
      },
    });

    unlisten = await ssh.onStatus((e) => {
      if (e.pane_id !== paneId) return;
      if (e.status.kind === "new_host_key") {
        hostKeyNotices = [...hostKeyNotices, { host: e.status.host, fingerprint: e.status.fingerprint }];
        return;
      }
      const wasConnected = status.kind === "connected";
      status = e.status;
      setInfo(e.status.kind);
      // The session is over, and so is whatever program asked for the mouse: stop reporting it, or the
      // shell that is left (or the next one) would print the reports as text.
      if (e.status.kind === "disconnected" || e.status.kind === "error" || e.status.kind === "host_key_changed") term.write(RESET_SESSION_MODES);
      if (e.status.kind === "connected") {
        reconnectAttempts = 0;
        safeFit();
        term.focus();
        logId = connectionLog.start(hostId, label, logKind);
        // After the shell has started, and its prompt is up.
        setTimeout(() => void seedFromHost(), 1500);
        if (target.kind === "host") {
          settings.markRecent(target.hostId);
          const startup = [host?.startup_command?.trim(), target.command?.trim()].filter(Boolean).join(" && ");
          // Give the remote shell a moment to print its prompt first.
          if (startup) setTimeout(() => void writeToPane(pane, render(startup, hostContextFor(target.hostId), {}) + "\r"), 300);
        }
      } else if (e.status.kind === "disconnected") {
        term.write(`\r\n\x1b[90m[session closed${e.status.code != null ? `, exit ${e.status.code}` : ""}]\x1b[0m\r\n`);
        if (wasConnected && !active) ui.notify("info", `Session to ${label} closed.`);
        if (logId) {
          connectionLog.end(logId, e.status.code, e.status.code != null ? "exited" : "dropped");
          logId = null;
        }
        // No exit code means the link dropped rather than the shell ending.
        if (wasConnected && e.status.code == null) scheduleReconnect();
      } else if (e.status.kind === "error" && logId) {
        connectionLog.end(logId, null, "failed");
        logId = null;
      }
    });

    setInfo("connecting");
    if (!needsCredentials) connect(null);
  });

  let unregisterSearch: (() => void) | undefined;

  // Highlight rules: only the rows on screen get decorations, redrawn when output or scrolling changes them.
  let highlightFrame = 0;
  let decorations: { dispose(): void }[] = [];
  let scannedTo = -1;
  const scheduleHighlight = () => {
    if (highlightFrame) return;
    highlightFrame = requestAnimationFrame(() => {
      highlightFrame = 0;
      paintHighlights();
      paintBlocks();
    });
  };
  function paintHighlights() {
    for (const d of decorations) d.dispose();
    decorations = [];
    if (!term) return;
    const rules = sanitize($state.snapshot(settings.prefs.highlightRules));
    const buf = term.buffer.active;
    const cursorAbs = buf.baseY + buf.cursorY;
    // Output that came before a rule existed is never reported.
    if (!rules.some((r) => r.enabled)) {
      scannedTo = Math.max(scannedTo, cursorAbs - 1);
      return;
    }
    if (buf.type === "alternate") return;
    for (let y = buf.viewportY; y < buf.viewportY + term.rows && y < buf.length; y++) {
      const line = buf.getLine(y);
      if (!line) continue;
      for (const m of matchLine(rules, line.translateToString(true))) {
        const marker = term.registerMarker(y - cursorAbs);
        if (!marker) continue;
        const d = term.registerDecoration({ marker, x: m.from, width: m.to - m.from, backgroundColor: m.rule.color, layer: "bottom" });
        if (d) decorations.push(d, marker);
      }
    }
    // New lines that match a rule asking for a notice.
    for (let y = Math.max(scannedTo + 1, cursorAbs - 200); y < cursorAbs; y++) {
      const text = buf.getLine(y)?.translateToString(true) ?? "";
      for (const r of triggered(rules, text)) void notifyDone(`${r.name || r.pattern} on ${host?.label ?? "a terminal"}: ${text.trim().slice(0, 100)}`);
    }
    scannedTo = Math.max(scannedTo, cursorAbs - 1);
  }

  // Command blocks: a marker at each end keeps the lines right while the scrollback moves.
  type Live = Block & { startM: { line: number; isDisposed: boolean; dispose(): void }; endM: { line: number; isDisposed: boolean; dispose(): void } };
  let live: Live[] = [];
  let blockDecorations: { dispose(): void }[] = [];
  let blockSeq = 0;
  let blockMenu = $state<{ x: number; y: number; id: number } | null>(null);

  function addBlock(rec: { command: string; exit: number | null; startedAt: number; endedAt: number; outputStart: number | null; outputEnd: number | null }) {
    if (!settings.prefs.commandBlocks || rec.outputStart === null || rec.outputEnd === null) return;
    const cur = cursor().line;
    const startM = term.registerMarker(Math.max(0, rec.outputStart - 1) - cur);
    const endM = term.registerMarker(rec.outputEnd - cur);
    if (!startM || !endM) return;
    live.push({ id: ++blockSeq, command: rec.command, exit: rec.exit, durationMs: rec.endedAt - rec.startedAt, pinned: false, start: startM.line, end: endM.line, startM, endM });
    const kept = trim(live, MAX_BLOCKS);
    for (const b of live) if (!kept.includes(b)) { b.startM.dispose(); b.endM.dispose(); }
    live = kept;
    scheduleHighlight();
  }

  /** The blocks whose lines are still in the buffer, with their lines brought up to date. */
  function liveBlocks(): Live[] {
    live = live.filter((b) => !b.startM.isDisposed && !b.endM.isDisposed && b.startM.line >= 0);
    for (const b of live) { b.start = b.startM.line; b.end = b.endM.line; }
    return live;
  }

  // The bars sit in the left padding, clear of the list panel's resize handle that overlaps the first pixels.
  const BAR_AT = 6;
  const leftPad = $derived(settings.prefs.commandBlocks ? Math.max(settings.prefs.terminalPadding, BAR_AT + 4) : settings.prefs.terminalPadding);

  function paintBlocks() {
    for (const d of blockDecorations) d.dispose();
    blockDecorations = [];
    if (!term || !settings.prefs.commandBlocks || term.buffer.active.type === "alternate") return;
    const buf = term.buffer.active;
    for (const b of visibleBlocks(liveBlocks(), buf.viewportY, term.rows)) {
      const d = term.registerDecoration({ marker: b.startM as never, x: 0, width: 1, height: Math.max(1, b.end - b.start + 1) });
      if (!d) continue;
      blockDecorations.push(d);
      d.onRender((el) => {
        // In the terminal's padding, so the first letter of each line stays whole.
        el.style.width = "3px";
        el.style.transform = `translateX(-${leftPad - BAR_AT}px)`;
        el.style.background = b.pinned ? "var(--color-accent)" : failed(b) ? "var(--color-danger)" : b.exit === 0 ? "var(--color-success)" : "var(--color-fg-muted)";
        el.style.opacity = b.pinned ? "1" : "0.75";
        el.style.cursor = "pointer";
        el.style.pointerEvents = "auto";
        el.style.zIndex = "5";
        el.title = summary(b);
        el.dataset.block = String(b.id);
        el.onclick = (e) => { e.stopPropagation(); blockMenu = { x: e.clientX, y: e.clientY, id: b.id }; };
      });
    }
  }

  const blockById = (id: number) => liveBlocks().find((b) => b.id === id);
  function outputOf(b: Block): string {
    term.selectLines(Math.min(b.end, b.start + 1), b.end);
    const text = term.getSelection();
    term.clearSelection();
    return text;
  }
  async function copyBlock(b: Block, what: "output" | "command") {
    const text = what === "output" ? outputOf(b) : b.command;
    if (!text.trim()) return ui.notify("info", "That command printed nothing.");
    try {
      await writeText(text);
      ui.notify("info", `Copied the ${what} of "${b.command.split("\n")[0].slice(0, 40)}".`);
    } catch (e) {
      ui.notify("error", `Copy failed: ${errorMessage(e)}`);
    }
  }
  function jumpBlock(dir: -1 | 1, only: "failed" | "pinned" = "failed") {
    const here = term.buffer.active.viewportY;
    const b = nextMatching(liveBlocks(), dir === -1 ? here : here, dir, only);
    if (b) term.scrollToLine(Math.max(0, b.start - 1));
    else ui.notify("info", only === "failed" ? "No failed command that way." : "No pinned command that way.");
  }

  $effect(() => {
    void settings.prefs.commandBlocks;
    scheduleHighlight();
  });
  $effect(() => {
    void settings.prefs.highlightRules.map((r) => `${r.pattern}${r.color}${r.enabled}${r.regex}${r.matchCase}`).join();
    scheduleHighlight();
  });
  onDestroy(() => {
    unregisterSearch?.();
    cancelAnimationFrame(highlightFrame);
    for (const d of blockDecorations) d.dispose();
    for (const d of decorations) d.dispose();
    completionBridge.clear(controls);
    screenWatch?.disconnect();
    clearTimeout(reconnectTimer);
    clearTimeout(zoomHintTimer);
    unlisten?.();
    resizeObserver?.disconnect();
    if (logId) connectionLog.end(logId, null, "closed");
    void closePane(pane);
    term?.dispose();
  });

  // Apply appearance changes from Settings to the live terminal.
  $effect(() => {
    const p = settings.prefs;
    const theme = paneTheme();
    const { fontFamily, lineHeight, cursorStyle, cursorBlink, letterSpacing, minimumContrastRatio, boldAsBright, terminalPadding, wordSeparator, screenReaderMode } = p;
    void terminalPadding; // read so this effect (and its safeFit()) reruns when padding changes too
    if (!term) return;
    term.options.theme = theme;
    term.options.fontFamily = fontFamily;
    const { fontSize, scrollback } = look();
    term.options.fontSize = fontSize;
    term.options.lineHeight = lineHeight;
    term.options.cursorStyle = cursorStyle;
    term.options.cursorBlink = cursorBlink;
    term.options.scrollback = scrollback;
    term.options.letterSpacing = letterSpacing;
    term.options.minimumContrastRatio = minimumContrastRatio;
    term.options.drawBoldTextInBrightColors = boldAsBright;
    term.options.screenReaderMode = screenReaderMode;
    if (wordSeparator) term.options.wordSeparator = wordSeparator;
    safeFit();
  });

  $effect(() => {
    if (active && status.kind === "connected") term?.focus();
  });

  // Coming back to this pane clears what you missed while it was in the background.
  $effect(() => {
    if (active && (ui.paneInfo[paneId]?.unread || ui.paneInfo[paneId]?.bell)) {
      ui.paneInfo[paneId] = { ...ui.paneInfo[paneId], unread: false, bell: false };
    }
  });

  // Reconnect requested from outside (the pane header's overflow menu),
  // for a specific pane rather than only the active one.
  let lastReconnectRequest = ui.paneInfo[paneId]?.reconnectRequest ?? 0;
  $effect(() => {
    const n = ui.paneInfo[paneId]?.reconnectRequest ?? 0;
    if (n !== lastReconnectRequest) {
      lastReconnectRequest = n;
      reconnect();
    }
  });

  // Global "find" shortcut targets the active pane only.
  let lastFind = ui.findRequest;
  $effect(() => {
    const n = ui.findRequest;
    if (n !== lastFind) {
      lastFind = n;
      if (active) openFind();
    }
  });

  const background = $derived(paneTheme().background);
</script>

<svelte:window onkeydown={(e) => { if (menu && e.key === "Escape") menu = null; }} onresize={() => (menu = null)} />

<div class="relative flex min-h-0 flex-1 flex-col" style:background>
  {#if production && target.kind === "host"}
    <div class="flex shrink-0 items-center gap-2 bg-danger px-3 py-0.5 text-[11px] font-semibold tracking-wide text-white" role="status">
      <ShieldAlert size={12} /> PRODUCTION · {label}
    </div>
  {/if}
  <!-- `isolate` keeps xterm's internal z-indexed layers (up to 11) inside
       this box, so the overlays below always sit on top and stay clickable. -->
  <div
    class="relative isolate z-0 min-h-0 flex-1"
    style:padding="{settings.prefs.terminalPadding}px"
    style:padding-left="{leftPad}px"
    bind:this={container}
    role="presentation"
    oncontextmenu={(e) => {
      e.preventDefault();
      menu = { x: e.clientX, y: e.clientY };
    }}
    ondragover={(e) => {
      // Only text drags here; OS file drops are handled at the webview
      // level (uploads), not as a native browser drop on this element.
      if (e.dataTransfer?.types.includes("text/plain")) e.preventDefault();
    }}
    ondrop={(e) => {
      if (e.dataTransfer?.types.includes("Files")) return;
      const text = e.dataTransfer?.getData("text/plain");
      if (!text) return;
      e.preventDefault();
      guardedPaste(text);
    }}
  ></div>

  <div bind:this={ghostRoot} class="pointer-events-none absolute inset-0 z-[5]">
    <CompletionOverlay
      {ghost}
      fontFamily={settings.prefs.fontFamily}
      fontSize={settings.prefs.fontSize}
      color={paneTheme().foreground ?? "#cccccc"}
    />
    {#if popup}
      <CompletionMenu groups={popup.groups} selected={popup.selected} place={popup.place} fontFamily={settings.prefs.fontFamily} onpick={pickMenu} />
    {/if}
  </div>
  <div class="sr-only" aria-live="polite" role="status">{menuAnnouncement}</div>

  {#if newOutputWhileScrolled}
    <button
      class="absolute bottom-2 left-1/2 z-10 flex -translate-x-1/2 items-center gap-1.5 rounded-md border border-line bg-panel/90 px-2.5 py-1 text-[11px] text-fg shadow hover:bg-panel-hover"
      title="Shift+End also jumps to the bottom"
      onclick={() => { term.scrollToBottom(); newOutputWhileScrolled = false; }}
    >
      <ChevronDown size={12} /> New output
    </button>
  {/if}

  {#if mouseTracked && !selectMode && status.kind === "connected"}
    <button
      class="absolute bottom-2 right-3 z-10 flex items-center gap-1.5 rounded-md border border-line bg-panel/90 px-2 py-1 text-[11px] text-fg-muted shadow hover:text-fg"
      title="The program in this terminal (for example tmux or vim) is using the mouse. Hold Shift to select text, or switch to select mode."
      onclick={toggleSelectMode}
    >
      <MousePointer2 size={11} /> Program has the mouse · Shift+drag selects · click to select instead
    </button>
  {/if}

  {#if zoomHint !== null}
    <div class="pointer-events-none absolute left-1/2 top-1/2 z-20 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-line bg-panel/90 px-3 py-1.5 font-mono text-sm shadow-lg" role="status">
      {zoomHint} px
    </div>
  {/if}

  {#if findOpen}
    <div class="absolute right-3 top-2 z-20 flex items-center gap-1 rounded-md border border-line bg-panel px-2 py-1 shadow-lg">
      <Search size={13} class="text-fg-muted" />
      <input
        bind:this={findInput}
        class="w-48 bg-transparent px-1 py-0.5 text-xs outline-none"
        placeholder="Find"
        bind:value={findQuery}
        oninput={() => findQuery && search.findNext(findQuery, { decorations: searchDecorations(), incremental: true })}
        onkeydown={(e) => {
          if (e.key === "Enter") findNext(e.shiftKey);
          else if (e.key === "Escape") closeFind();
        }}
      />
      <button class="icon-btn h-6 w-6" title="Previous (Shift+Enter)" onclick={() => findNext(true)}><ChevronUp size={13} /></button>
      <button class="icon-btn h-6 w-6" title="Next (Enter)" onclick={() => findNext()}><ChevronDown size={13} /></button>
      <button class="icon-btn h-6 w-6" title="Close (Esc)" onclick={closeFind}><X size={13} /></button>
    </div>
  {/if}

  {#if blockMenu}
    {@const blk = blockById(blockMenu?.id ?? -1)}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (blockMenu = null)} oncontextmenu={(e) => { e.preventDefault(); blockMenu = null; }}></button>
    <div class="fixed z-50 w-60 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" use:keepInView={blockMenu} role="menu" aria-label="Command" data-testid="block-menu">
      {#if blk}
        <div class="truncate px-3 py-1 text-xs text-fg-muted" title={summary(blk)}>{summary(blk)}</div>
        {#each [
          { label: "Copy output", run: (b: Block) => void copyBlock(b, "output") },
          { label: "Copy command", run: (b: Block) => void copyBlock(b, "command") },
          { label: "Select output", run: (b: Block) => term.selectLines(Math.min(b.end, b.start + 1), b.end) },
          { label: blk.pinned ? "Unpin" : "Pin this command", run: (b: Block) => { b.pinned = !b.pinned; scheduleHighlight(); } },
          { label: "Next pinned command", run: () => jumpBlock(1, "pinned") },
        ] as item (item.label)}
          <button class="flex w-full items-center px-3 py-1.5 text-left hover:bg-hover" role="menuitem" onclick={() => { const b = blk; blockMenu = null; item.run(b); }}>{item.label}</button>
        {/each}
      {:else}
        <div class="px-3 py-1 text-xs text-fg-muted">That output has scrolled out of the buffer.</div>
      {/if}
    </div>
  {/if}

  {#if menu}
    <button class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (menu = null)} oncontextmenu={(e) => { e.preventDefault(); menu = null; }}></button>
    <div class="fixed z-50 w-60 rounded-md border border-line bg-panel py-1 text-sm shadow-2xl" use:keepInView={menu} role="menu">
      {#each [
        { label: "Copy", keys: "Ctrl+Shift+C", run: copySelection, disabled: !term?.hasSelection() },
        { label: "Paste", keys: "Ctrl+Shift+V", run: paste, disabled: status.kind !== "connected" },
        { label: "Select all", keys: "", run: () => term.selectAll(), disabled: false },
        { label: "Find…", keys: "Ctrl+Shift+F", run: openFind, disabled: false },
        { label: "Copy last command output", keys: "", run: copyLastOutput, disabled: !commands.last },
        ...(hostId && askable() ? [{ label: commands.active ? "Shell integration on this host…" : "Install shell integration on this host…", keys: "", run: () => (ui.modal = { kind: "integration-install", hostId }), disabled: false }] : []),
        { label: "Previous failed command", keys: "", run: () => jumpBlock(-1), disabled: !liveBlocks().some(failed) },
        { label: "Next failed command", keys: "", run: () => jumpBlock(1), disabled: !liveBlocks().some(failed) },
        { label: "Previous / next prompt", keys: "Ctrl+Shift+↑ / ↓", run: () => jumpPrompt(-1), disabled: !commands.active },
        { label: selectMode ? "Give the mouse back to the program" : "Select text with the mouse", keys: "", run: toggleSelectMode, disabled: !mouseTracked },
        { label: "Copy entire buffer", keys: "", run: copyEntireBuffer, disabled: false },
        { label: "Clear scrollback", keys: "", run: () => term.clear(), disabled: false },
      ] as item (item.label)}
        <button
          class="flex w-full items-center justify-between px-3 py-1.5 text-left hover:bg-panel-hover disabled:opacity-40 disabled:hover:bg-transparent"
          role="menuitem"
          disabled={item.disabled}
          onclick={() => {
            menu = null;
            void item.run();
            term.focus();
          }}
        >
          <span class="truncate">{item.label}</span>
          <span class="ml-3 shrink-0 text-xs text-fg-muted">{item.keys}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if pending}
    <div class="absolute inset-0 z-40 flex items-center justify-center bg-base/80 p-4" role="presentation">
      <div class="w-full max-w-lg rounded-xl border {pending.kind === 'command' || (pending.kind === 'paste' && pending.danger) ? 'border-danger/50' : 'border-line'} bg-panel p-5 shadow-2xl" role="alertdialog" aria-modal="true" aria-label="Confirm input">
        {#if pending.kind === "command"}
          <div class="mb-2 flex items-center gap-2 text-sm font-semibold text-danger"><ShieldAlert size={16} /> Run this on production?</div>
          <pre class="max-h-32 overflow-auto rounded-md bg-base px-3 py-2 font-mono text-xs whitespace-pre-wrap">{pending.line}</pre>
          <p class="mt-2 text-xs text-fg-muted">It matches a destructive-command pattern for <strong>{label}</strong>. This check only sees what you typed; it's a reminder, not a guarantee.</p>
        {:else}
          <div class="mb-2 flex items-center gap-2 text-sm font-semibold {pending.danger || pending.secretWarning ? 'text-danger' : ''}">
            <ShieldAlert size={16} /> Paste {pending.lines} line{pending.lines === 1 ? "" : "s"} into {label}?
          </div>
          <pre class="max-h-48 overflow-auto rounded-md bg-base px-3 py-2 font-mono text-xs whitespace-pre-wrap">{pending.text.length > 4000 ? `${pending.text.slice(0, 4000)}
…` : pending.text}</pre>
          <p class="mt-2 text-xs text-fg-muted">
            {#if pending.secretWarning}{pending.secretWarning} {/if}
            {#if pending.danger}It contains a command that looks destructive, and this is a production host. {/if}
            {#if /[\r\n]/.test(pending.text)}Each line runs as soon as it's pasted.{/if}
          </p>
        {/if}
        <div class="mt-4 flex justify-end gap-2">
          <!-- svelte-ignore a11y_autofocus -->
          <button class="btn-primary" autofocus onclick={() => resolvePending(false)}>Cancel</button>
          <button class="btn-ghost border {pending.kind === 'command' || (pending.kind === 'paste' && pending.danger) ? 'border-danger/50 text-danger' : 'border-line'}" onclick={() => resolvePending(true)}>
            {pending.kind === "command" ? "Run it" : "Paste"}
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if hostKeyNotices.length}
    <div class="absolute inset-x-2 top-2 z-10 flex items-start gap-2 rounded-md border border-line bg-panel/95 px-3 py-2 text-xs shadow-lg">
      <ShieldAlert size={14} class="mt-0.5 shrink-0 text-accent" />
      <div class="min-w-0 flex-1 space-y-1">
        {#each hostKeyNotices as k (k.host)}
          <div>
            <div class="font-medium">Host key trusted for {k.host}</div>
            <div class="truncate font-mono text-fg-muted">{k.fingerprint}</div>
          </div>
        {/each}
      </div>
      <button class="text-fg-muted hover:text-fg" aria-label="Dismiss" onclick={() => (hostKeyNotices = [])}>✕</button>
    </div>
  {/if}

  {#if !started && needsCredentials}
    <div class="absolute inset-0 z-30 flex items-center justify-center bg-base/95 p-4">
      <form onsubmit={submitCredentials} class="w-full max-w-sm space-y-4 rounded-xl border border-line bg-panel p-6 shadow-2xl">
        <div class="flex items-center gap-3">
          <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/15 text-accent"><KeyRound size={18} /></div>
          <div class="min-w-0">
            <div class="truncate text-sm font-semibold">Log in to {label}</div>
            <div class="truncate font-mono text-xs text-fg-muted">{host?.hostname}{host && host.port !== 22 ? `:${host.port}` : ""}</div>
          </div>
        </div>
        <div>
          <label class="label" for="c-user-{paneId}">Username</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="c-user-{paneId}" class="input font-mono" bind:value={username} required autocomplete="username" spellcheck="false" autofocus />
        </div>
        <div>
          <label class="label" for="c-pw-{paneId}">Password</label>
          <input id="c-pw-{paneId}" class="input" type="password" bind:value={password} required autocomplete="current-password" />
        </div>
        <label class="flex items-center gap-2 text-xs text-fg-muted">
          <input type="checkbox" class="accent-input" bind:checked={remember} />
          Remember for this host (encrypted in your vault)
        </label>
        {#if credentialError}
          <p class="rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger">{credentialError}</p>
        {/if}
        <div class="flex gap-2">
          <button class="btn-ghost flex-1 border border-line" type="button" onclick={() => target.kind === "host" && (ui.modal = { kind: "host", id: target.hostId })}>
            Edit host
          </button>
          <button class="btn-primary flex-1" type="submit">Connect</button>
        </div>
      </form>
    </div>
  {:else if status.kind === "connecting"}
    <div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-base/70">
      <div class="flex items-center gap-2 rounded-md bg-panel px-4 py-2 text-sm text-fg-muted">
        <Loader2 size={16} class="animate-spin text-accent" /> Connecting to {label}…
      </div>
    </div>
  {:else if status.kind === "host_key_changed"}
    <div class="absolute inset-0 z-30 flex items-center justify-center bg-base/95 p-4">
      <div class="w-full max-w-md rounded-xl border border-danger/40 bg-panel p-5">
        <div class="mb-2 flex items-center gap-2 text-sm font-semibold text-danger">
          <ShieldX size={18} /> Host key changed for {status.host}{status.port !== 22 ? `:${status.port}` : ""}
        </div>
        <p class="text-xs text-fg-muted">
          The server presented a different key from the one trusted before, so the connection was stopped and nothing was
          sent. This happens when a server is reinstalled, but it is also exactly what a man-in-the-middle attack looks
          like. Reconnect to compare the old and new fingerprints and decide.
        </p>
        <div class="mt-3 rounded-md bg-base px-3 py-2 font-mono text-xs break-all">{status.fingerprint}</div>
        <div class="mt-4 flex justify-end gap-2">
          <button class="btn-ghost" onclick={() => (status = { kind: "disconnected", code: null })}>Close</button>
          <button class="btn border border-danger/40 text-danger hover:bg-danger/10" onclick={reconnect}>Review and reconnect</button>
        </div>
      </div>
    </div>
  {:else if status.kind === "error" || (status.kind === "disconnected" && started)}
    <div class="absolute inset-x-0 bottom-0 z-10 flex items-center gap-3 border-t border-line bg-panel px-4 py-2 text-xs">
      {#if status.kind === "error"}
        <ShieldAlert size={14} class="shrink-0 text-danger" />
        <span class="line-clamp-2 min-w-0 flex-1 break-words text-danger">{status.message}</span>
        <button class="btn-ghost py-1" onclick={showError}>Details</button>
        {#if moshServerMissing}
          <button class="btn-secondary py-1" onclick={usePlainSsh}>Use plain SSH</button>
        {/if}
      {:else}
        <Unplug size={14} class="shrink-0 text-fg-muted" />
        <span class="flex-1 text-fg-muted">Disconnected</span>
      {/if}
      <button class="btn-ghost py-1" onclick={reconnect}>
        <RefreshCw size={12} /> Reconnect
      </button>
    </div>
  {/if}
</div>
