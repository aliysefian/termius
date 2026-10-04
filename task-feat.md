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

- [x] **C1. Status bar.** Done, narrower than first scoped: a 24 px bar
  under the terminal area with Lock vault, SSH agent on/off (keys
  offered), CLI on/off and active-tunnel count, each a button that jumps
  to its view, plus the active pane's label, cwd and live terminal size.
  Hidden in focus mode. **Not done**: per-pane latency/cipher/KEX and a
  vault sync-device readout — both need backend data this environment
  can't compile or verify (no webkit/gtk dev libs here; see
  `docs/DEVELOPMENT.md`), and a vault-folder fetch added for low value on
  its own. Centralizing agent/CLI status also fixed a real bug: Keys and
  Settings each kept their own local copy, so toggling one didn't update
  the other until a remount.

- [x] **C2. Tab strip overflow and activity.** Fade on scrolled edges, a
  "list all tabs" button (shown once the strip overflows) with a
  searchable dropdown, and per-tab unread-output dot, running-command
  spinner and bell icon. Command palette and the new-connection buttons
  no longer scroll out of view with the tabs (they used to). **Not
  done**: true MRU tab switching; Ctrl+Tab/Ctrl+Shift+Tab already cycle
  tabs sequentially, which was judged enough for now.

- [x] **C3. Pane header overflow menu.** Select mode, sync, record and
  maximize moved into a "⋯" menu; split and close stay inline. Added
  Reconnect (for a specific pane, not just the active one), Duplicate
  pane, Open SFTP here, Copy ssh command, and Swap panes (two-pane tabs).
  A recording pane still shows a small dot in the header even with the
  action moved to the menu.

- [x] **C4. Keyboard navigation between panes and dividers**, mostly.
  Added Alt+Shift+Arrow (not plain Alt+Arrow — that's readline's
  back/forward-word binding, used constantly in a shell) to focus the
  spatially nearest pane; dividers are keyboard-focusable with
  `role=separator`, `aria-valuenow/min/max`, arrow-key resize, Home/Enter
  and double-click to centre, and Escape cancels a drag — all matching
  `ResizablePanel`'s pattern. Added "Swap panes" (see C3). **Not done**:
  drag-a-pane-header-to-re-dock; too large and too hard to get right
  without a running app to try it against.

- [x] **C5. Scroll-to-bottom pill.** A floating "New output" pill appears
  when scrolled back in the buffer and more arrives; click or Shift+End
  jumps to the bottom.

- [x] **C6. Host tree keyboard model.** A new `flattenTree()` (tested)
  gives a single, visibility-aware row order that Recent and the real
  tree share. Real roving-tabindex keyboard support: Up/Down/Home/End,
  Left/Right to collapse/expand or move to parent/child, type-ahead,
  `*` expands all, Enter connects, Space opens details, F2 opens the
  host for editing (there's no separate inline-rename to map to), Delete
  asks to delete. **Gap**: no nested `role=group` wrapper per level, so
  a screen reader won't announce containment as precisely as a
  textbook ARIA tree; real keyboard operability was prioritised over
  that, and it couldn't be checked with an actual screen reader here.

- [x] **C7. Window title and taskbar.** The window title follows the
  active pane (`user@host · SSHVault`, or just `SSHVault` with none).
  SFTP transfers show aggregate progress on the taskbar/dock
  (`setProgressBar`), on platforms that support it.

---

## Phase D: forms and screens

- [x] **D1. HostForm in sections.** Split into four tabs: Connection,
  Route, Organise, Automation. Errors and the jump-host-identity warning
  show under the tab strip regardless of which tab is open (not only at
  the bottom), and a save error switches to Connection, where every
  thrown validation error actually lives. Added **Test connection**
  (reuses the reachability check) and a "none" colour swatch plus a
  custom colour picker. Automation also lists this host's tunnels with
  an auto-start checkbox, pulling in the "local tunnels to auto-start"
  idea. **Not done as literally specified**: Test connection only works
  for an already-saved host, reading the saved record rather than
  unsaved edits — probing an arbitrary, unsaved hostname/port needs a
  new backend command, and this sandbox has no way to compile or verify
  Rust changes (no webkit/gtk dev libs; see `docs/DEVELOPMENT.md`).

- [x] **D2. Settings with navigation and search**, mostly as specified.
  Left nav (Appearance, Terminal, Connections, Safety, Integrations,
  Updates, Advanced) and a search box that matches across every
  category. Every action's message now shows under its own section
  (`themeMsg`/`cliMsg`/`exportMsg`/`safetyMsg`) instead of one shared
  message far from whatever set it. **Deliberately not done**: Safety
  keeps its explicit Save, now with a "Synced" badge and a line
  explaining why — it's shared by every device that opens the vault, so
  an instant-save there would let a stray keystroke change a safety
  setting (paste threshold, destructive-command patterns) everywhere at
  once with nothing to review first. Judged safer than literally
  matching the plan.

- [x] **D3. Terminal appearance preview.** A real, live `xterm.js`
  instance (not a CSS mockup) next to the theme gallery, showing a
  prompt, an `ls` listing, a diff and bold text, reactive to every
  appearance setting. Added letter spacing, padding, minimum contrast
  ratio, cursor colour and bold-as-bright, all wired into real terminal
  sessions too, not just the preview. **Not done**: the ligatures
  toggle — `@xterm/addon-ligatures` isn't a dependency, and adding one
  sight-unseen (its font-shaping behaviour can't be checked without a
  running browser here) seemed worse than leaving it out and saying so.

- [x] **D4. SFTP polish**, most of it. Resizable, keyboard-adjustable
  split between the two panes (same pattern as the sidebar and pane
  dividers); sortable columns (name/size/modified/mode, folders always
  first); a per-row context menu (open/edit, download or upload, copy
  path, rename, permissions, reveal, delete); a breadcrumb path with
  clickable segments and a fallback to typing a raw path; a per-folder
  filter box; and labelled fields plus Cancel on the ask-credentials
  form. **Not done**: bookmarks per host (a new, persistent per-host
  feature judged out of scope for this pass) and free space in the
  footer (SFTP has no standard free-space query, and probing the local
  filesystem would need a new backend command this sandbox can't verify
  compiles).

- [x] **D5. Empty-state shortcut grid reads real keybindings.** Pulled
  from `ACTIONS`/`comboFor` so a remapped shortcut shows correctly;
  copy/paste and zoom stay hand-written since they aren't remappable
  actions, so they can't go stale.

- [x] **D6. First-run onboarding**, scoped down from a guided tour to
  one dismissible card (import / add a host by hand / quick connect,
  plus a short "finding your way around" note on the activity bar,
  palette and shortcuts), shown once right after creating a brand-new
  vault, never after opening an existing one. A local shell tab opens
  automatically alongside it. **Not done**: a multi-step wizard that
  highlights live UI elements — a real "spotlight" tour needs to
  position itself against the actual DOM and be seen to get right, which
  isn't possible to verify in this environment.

- [x] **D7. Unlock screen details**, partly. Added a "Recent vaults"
  list on the home screen (click to jump straight to its password
  screen) and hardened the recovery-key reminder so it truly cannot be
  dismissed without ticking the box — no bypass-via-confirm, which the
  old version had. **Not done**: which device last changed the vault and
  when — that needs `vault_info`, which only works after unlocking (it
  decrypts records), so it can't be shown on the lock screen itself
  without new, unauthenticated backend metadata this sandbox can't
  verify compiles.

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
