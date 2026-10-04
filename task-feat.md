# UI/UX improvements and missing features

Working list for the next rounds of SSHVault. Written 2026-10-04 after a
read-through of every component, `app.css`, the stores and the roadmap.
Tick items as they land and move finished
ones to `ROADMAP.md`.

Each item has a size (**S** under half a day, **M** a day or two, **L**
several days), the files it touches, and what "done" looks like. Phases
are in the order they should be built: the earlier ones fix things that
bite every day; the later ones are features an SSH client at this level is
expected to have.

---

## Phase A: fix what hurts daily (bugs and guard rails)

- [x] **A1. Escape closes only the top dialog.** `Modal.svelte:19-24`
  listens on `window` without stopping propagation, so Escape in the
  master-password prompt (opened from "Reveal saved" in HostForm) also
  closes the host form underneath. Use a capture listener plus
  `stopImmediatePropagation`, as `DialogHost` already does, and keep a
  stack of open modals so only the topmost reacts. **S**
  Done: open HostForm → Reveal saved → Escape leaves HostForm open.

- [x] **A2. Unsaved-changes guard on modals.** Backdrop click or Escape
  closes `Modal.svelte` even when HostForm, IdentityForm, SnippetForm or
  ForwardForm has edits. Add a `dirty` prop; when set, `ask()` "Discard
  changes?" before closing. **S**

- [x] **A3. Confirm before dropping live sessions.** Close tab, close
  pane, "Close other tabs" and app quit kill connected sessions with no
  prompt (`ui.svelte.ts:274-287`, `TerminalArea.svelte:232,263,369`).
  Confirm when a pane is connected, and (with shell integration) say
  whether a command is still running. Add a Tauri `onCloseRequested`
  handler for quit with N open sessions. A "don't ask again" checkbox
  stores a pref. **M**

- [x] **A4. Settings "Reset" is too broad and unguarded.** The button in
  the Terminal appearance header (`SettingsPanel.svelte:126`) resets every
  pref including keybindings, custom themes and auto-lock. Scope it to
  appearance, move a "Reset all settings" to the bottom, and confirm both.
  **S**

- [x] **A5. Focus trap and focus restore in Modal.** Tab must cycle inside
  the dialog, the first field gets focus on open, and focus returns to
  the opener on close. Use `aria-labelledby` pointing at the title.
  Apply to `Modal.svelte` once; every modal inherits it. **S**

- [x] **A6. RunOnHosts can be closed mid-run.** Disable Close and Escape
  while runs are active, or confirm "Cancel N running commands?"
  (`RunOnHosts.svelte:225`). **S**

- [x] **A7. Dismissable, stacking toasts.** Today one toast replaces the
  previous and errors vanish after 4 s (`ui.svelte.ts:125`). Show up to
  three stacked, give each a close button, let errors persist until
  dismissed, and add "Copy details" for errors with a long message. The
  undismissable `vaultStore.error` box (`+page.svelte:210`) becomes a
  persistent error toast with `role=alert`. **M**

- [x] **A8. Full error text in the reconnect bar.** The disconnected bar
  truncates the error to one line with the rest in a tooltip
  (`TerminalPane.svelte` ~813). Wrap to two lines and add "Show details"
  that opens a small dialog with the whole message and a copy button. **S**

---

## Phase B: design-system consistency

- [x] **B1. Token the accent colour everywhere.** `accent-[#7b61ff]` is
  hard-coded in about 35 checkboxes and ignores the light-theme accent.
  Add `.checkbox` (and `.radio`) classes in `app.css` using
  `accent-color: var(--color-accent)` and replace every instance. Same for
  the `#7B61FF` host-colour fallback (`HostRow.svelte:97`,
  `HostDetails.svelte:65`) and the search highlight colours
  (`TerminalPane.svelte:405-408`), which should derive from the active
  terminal theme. **S**

- [x] **B2. Shared primitives: Badge, Kbd, Spinner, EmptyState,
  SecondaryButton.** PROD, SYNC, UNENCRYPTED, env and status pills are
  each hand-written at `text-[9px]`–`text-[11px]`; "secondary" buttons
  are `btn-ghost border border-line` in three places; loading is a mix of
  plain "Starting…"/"Decrypting…"/"Loading…" text, `Loader2` and
  `animate-pulse`. Create `Badge.svelte`, `Kbd.svelte`, `Spinner.svelte`,
  `EmptyState.svelte` and a `.btn-secondary` class, then replace. Raise
  the minimum text size to 11 px (10 px only inside badges). **M**

- [x] **B3. One name per concept.** "Credentials" (activity bar) vs
  "Keychain"/"Identity" (HostForm), "Tunnels" vs "Port Forwarding",
  "Groups and proxies" vs "Groups", "colour" vs "Color". Pick one each
  (suggest: Credentials, Tunnels, Groups, and British or American
  spelling consistently) and sweep all strings and docs. **S**

- [x] **B4. Consistent page width and section layout.** Settings uses
  `max-w-2xl`, other pages `max-w-3xl`. Standardise on one width and one
  section card style (`PageSection.svelte` with title, description and
  optional header action). **S**

- [x] **B5. Fix invalid markup.** A `span role=button` sits inside a
  `<button>` in the theme gallery (`SettingsPanel.svelte:155`); replace
  with a sibling icon button using a lucide `X`. Audit other
  interactive-inside-interactive cases. **S**

- [x] **B6. Hover-only actions must also be reachable.** Snippet row
  actions, Known hosts "Stop trusting" and inactive-tab close buttons are
  `opacity-0` until hover (`SnippetsPanel.svelte:73`,
  `KnownHostsPanel.svelte:104`, `TerminalArea.svelte:231`). Show them on
  focus-within, on touch devices always, and give every row a context
  menu with the same actions. **S**

---

## Phase C: the shell

- [ ] **C1. Status bar.** A 24 px bar under the terminal area showing, left
  to right: vault state (locked/unlocked, sync folder, last change and
  which device made it), SSH agent on/off with request count, CLI on/off,
  active tunnels (click to open Tunnels), and for the active pane:
  user@host, latency (from keep-alive round trip), cipher/KEX, terminal
  size and encoding. Hidden in focus mode. Each segment is a button that
  jumps to the right view. **M**

- [ ] **C2. Tab strip overflow and activity.** The strip scrolls sideways
  with no indicator (`TerminalArea.svelte:164`). Add a "list all tabs"
  dropdown with search when tabs overflow, a fade on the scrolled edges,
  and per-tab indicators: an "unread output" dot when a background tab
  printed since you last saw it, a running-command spinner (shell
  integration), and a bell icon after a bell. Ctrl+1…9 already select
  tabs; add Ctrl+Shift+Tab-style MRU switching. **M**

- [ ] **C3. Pane header overflow menu.** The h-7 header holds up to 12
  controls and truncates on narrow panes. Keep label, env pill and cwd
  visible; put select mode, sync, record, zoom, split and close under a
  single "⋯" menu, with the most common two (split, close) still inline.
  Add Reconnect, Duplicate pane, Open SFTP here and Copy ssh command to
  that menu. **S**

- [ ] **C4. Keyboard navigation between panes and dividers.** Alt+Arrow
  focuses the pane in that direction; Ctrl+Shift+Alt+Arrow resizes the
  divider; double-click on a divider resets to 50/50. Dividers get
  `role=separator` with `aria-valuenow`, like `ResizablePanel`. Add
  "Swap panes" and drag-a-pane-header to re-dock. **M**

- [ ] **C5. Scroll-to-bottom pill and scroll position.** When scrolled up
  in a terminal with new output arriving, show a floating "↓ New output"
  pill (like Discord/Slack). Shift+End or clicking it jumps down. **S**

- [ ] **C6. Host tree keyboard model.** Rows are `role=button`; the tree
  needs `treeitem`s with Up/Down, Left/Right to collapse/expand, Home/End,
  type-ahead, and `*` to expand all. Enter connects, Space opens details
  (already), F2 renames, Delete asks to delete. **M**

- [ ] **C7. Window title and taskbar.** Set the window title to
  "web-01 · SSHVault" for the active pane (Tauri `setTitle`), and show
  progress on the taskbar/dock for SFTP transfers where the platform
  supports it. **S**

---

## Phase D: forms and screens

- [ ] **D1. HostForm in sections.** One 600-line scroll today. Split into
  tabs or collapsible sections: **Connection** (address, protocol,
  credentials), **Route** (jump host, proxy, agent/X11 forwarding,
  keep-alive), **Organise** (group, environment, tags, favorite, colour,
  notes, custom fields) and **Automation** (startup command, local
  tunnels to auto-start). Show validation inline next to the field and a
  summary at the top, not only at the bottom (`HostForm.svelte:591`).
  Add a **Test connection** button (TCP + SSH banner + auth, using the
  existing reachability check) that reports in the form. Host colour gets
  a "none" option and a custom picker (`HostForm.svelte:570-578`). **M**

- [ ] **D2. Settings with navigation and search.** Ten stacked cards with
  two saving models (instant vs Safety's explicit Save). Add a left nav
  (Appearance, Terminal, Connections, Safety, Local shell, Integrations,
  Updates, Advanced), a search box that filters settings by label, and
  make Safety save instantly with the same "synced setting" badge the
  other vault settings use. Move the export message next to its buttons
  (`SettingsPanel.svelte:428`). **M**

- [ ] **D3. Terminal appearance preview.** A live preview card next to the
  theme gallery and font controls showing a prompt, `ls` colours, a
  diff and the cursor, so changes are visible without an open session.
  Add letter spacing, padding, ligatures toggle, cursor colour, bold-as-
  bright, and minimum contrast ratio (xterm `minimumContrastRatio`). **M**

- [ ] **D4. SFTP polish.** Resizable split between the two panes
  (`SftpView.svelte:267`), sortable columns, a context menu per row
  (open, download/upload, rename, delete, chmod, copy path, quick look),
  breadcrumb path with clickable segments, bookmarks per host, a filter
  box, and labelled fields plus Cancel on the ask-credentials form
  (`SftpView.svelte:328-330`). Show free space in the footer. **M**

- [ ] **D5. Empty-state shortcut grid reads real keybindings.** The grid in
  `TerminalArea.svelte:412-421` is hard-coded and wrong after remapping.
  Render from `settings.prefs.keybindings` via the same source the cheat
  sheet uses. **S**

- [ ] **D6. First-run onboarding.** After creating a vault the host list
  is empty. Offer a three-step card: import (ssh config / PuTTY /
  MobaXterm / CSV / Ansible), add a host by hand, or quick connect; then
  a one-time tour of the activity bar, palette and shortcuts. Add a
  sample "localhost" local-shell tab so the terminal is not empty. **M**

- [ ] **D7. Unlock screen details.** Show which sync folder and device the
  vault is on, time of last change, and a "Recent vaults" list when more
  than one has been opened. Keep the recovery-key reminder visible until
  the user confirms they stored it. **S**

---

## Phase E: features an SSH client at this level needs

Ordered by how often a DevOps user hits the gap.

- [ ] **E1. Interactive authentication prompts (2FA/TOTP, Duo, PAM).**
  `ssh.rs:1045` answers every keyboard-interactive prompt with the stored
  password, so servers that ask for a verification code can't be used.
  Relay prompts to the UI (a dialog in the pane, like the host-key
  dialog), echo-off for secrets, remember "password first, then code"
  ordering per host. Also support password change requests. **L**

- [ ] **E2. Install public key on host (ssh-copy-id).** From a key,
  credential or host: pick target hosts, authenticate with the current
  method, append the key to `~/.ssh/authorized_keys` with correct
  permissions, verify by reconnecting with the key, and optionally
  switch the host's credential to that key. Today the user is told to
  paste an `echo` command (`HostForm.svelte:264-269`). Pair with a **key
  rotation** flow: generate new key, install on every host using the old
  one, remove the old line, retire the old key. **L**

- [ ] **E3. Restore the last session on launch.** Saved workspaces exist;
  add "Reopen tabs from last time" (pref, default on) that snapshots the
  open tabs/panes/layout on exit and restores them after unlock, with
  per-tab reconnect and a "don't reconnect production" option. **M**

- [ ] **E4. Drag files onto a terminal to upload.** Drop a file on a pane
  → SFTP upload to the shell's current directory (known via OSC 7, else
  home), with the transfer in the existing queue; then paste the remote
  path. Drag text onto a pane pastes it. **M**

- [ ] **E5. Clickable paths and smarter links.** Detect absolute and
  `./relative` paths in output (resolved against the OSC 7 cwd) and
  `file:line` patterns; Ctrl+click opens in SFTP quick look or the local
  editor via the existing edit-remote-file flow. Detect IPs and
  `host:port` and offer "Quick connect" or "Add tunnel". **M**

- [ ] **E6. Inline command suggestions.** While typing at a prompt (shell
  integration shows where), offer completions from this host's history,
  the group's history and snippets, in a ghost-text style accepted with
  → or Tab-Tab; Ctrl+R opens a searchable history popover. Opt-in pref.
  **L**

- [ ] **E7. Host monitoring widgets.** On the host details card and
  optionally in the pane header: CPU, memory, disk, load and uptime,
  sampled every N seconds over the existing session with a small
  portable script (`/proc` on Linux, `sysctl` on BSD/macOS). A "Fleet"
  view shows a tile per host for a group, colour-coded, with reachability
  auto-refresh. Opt-in per host. **L**

- [ ] **E8. Expiry and hygiene warnings.** Warn in Keys and on the host
  row when an OpenSSH certificate is within 14 days of `valid_before`,
  when a key is RSA < 3072 or DSA, when a key is older than N years,
  when a password credential hasn't rotated in a year, and when a host
  still uses password auth while a key exists. A "Security review" page
  under Vault lists all of them with one-click fixes. **M**

- [ ] **E9. Connection log and session timeline.** A local, per-computer
  log of connections (who, where, when, how long, exit reason, bytes),
  searchable and exportable to CSV/JSON. Shows under Vault → Activity and
  on the host details card. Never synced. **M**

- [ ] **E10. Login scripts (expect/send).** Per host, a short list of
  "when the terminal shows X, send Y" rules with a timeout, run once
  after connect: `sudo -i`, enable mode on network gear, a `cd`. Never
  stores a second password in clear: Y can reference the host's
  credential or a Keychain entry. Logged in the pane as "login script
  ran". **M**

- [ ] **E11. System tray and global hotkey.** Minimise to tray with a
  tray menu (recent hosts, quick connect, lock, quit), a global shortcut
  to show/hide the window, and "start minimised / start with the
  system". Uses `tauri-plugin-global-shortcut` and the tray API. **M**

- [ ] **E12. Detach tab to a new window.** Drag a tab out of the strip or
  "Move to new window" to open a second Tauri window sharing the same
  sessions, for multi-monitor setups. Each window has its own tab strip;
  the vault, agent and SFTP queue stay process-wide. **L**

- [ ] **E13. Terminal extras.** Triple-click selects a line; word-separator
  setting; "paste on right click" and "middle click pastes selection"
  options (PuTTY/Linux habits); trim trailing newline on paste; warn when
  the pasted text looks like a private key or token; screen-reader mode
  toggle (xterm `screenReaderMode`); image protocol support
  (`@xterm/addon-image` for sixel/iTerm2 images, used by `timg`, `chafa`,
  `viu`); save the whole buffer to a file. **M**

- [ ] **E14. ZMODEM transfers (`rz`/`sz`).** Common on network gear and
  jump boxes without SFTP. Detect the ZMODEM handshake in the stream and
  route to the file dialogs. Tabby and SecureCRT have it. **L**

- [ ] **E15. Hardware keys and YubiKey.** Already on the roadmap as
  "Later": FIDO2 `sk-ssh-ed25519` and PKCS#11 through the vault agent,
  and unlocking the vault with a hardware-backed key. Keep it there; it
  depends on E1 for the touch prompts. **L**

- [ ] **E16. Localisation.** Extract strings behind a tiny `t()` helper
  with English as the source, RTL-aware layout (the activity bar and
  tree flip), and a first additional language. Low effort per string
  once B3 has made names consistent. **L**

---

## Phase F: keep the quality bar

- [ ] **F1. Component tests for the primitives** (Badge, Kbd, Modal focus
  trap, toast stacking) with Vitest + Testing Library; snapshot the empty
  states. **S**
- [ ] **F2. A UI review checklist** in `CONTRIBUTING.md`: every new action
  has a keyboard path, every destructive action asks, every list has an
  empty state, every string uses the agreed names, no hard-coded colours.
  **S**
- [ ] **F3. Screenshots in the README and USAGE** regenerated from a
  scripted demo vault (a `pnpm demo` target that seeds fake hosts), so
  docs stay current. **S**

---

## Suggested order

1. **Phase A** in one release (0.9.10): all small, all fix real bugs.
2. **Phase B + C1–C3 + D5** (0.10.0): the app looks and reads as one piece,
   gains a status bar and tab/pane management.
3. **D1, D2, D4, E1, E3, E4** (0.11.0): forms worth living in, 2FA
   servers work, sessions survive a restart, files drop onto terminals.
4. **E2, E5, E7, E8, E9** (0.12.0): the DevOps features that set the app
   apart from Termius: key installation and rotation, monitoring, hygiene
   warnings and an activity log, all local and vault-encrypted.
5. The rest as demand shows.

## Out of scope for now

- Mosh (see ROADMAP "Needs a decision").
- A hosted sync service or accounts: the folder-sync design is the point.
- Mobile apps.
