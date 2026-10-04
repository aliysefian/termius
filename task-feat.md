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

This round covered the items buildable entirely in the frontend, each
verified with `svelte-check`, the full test suite and a production build.
**None of this session's changes touch `src-tauri`** (except two already-
covered items from earlier phases): this sandbox has no webkit/gtk dev
libs, so a Rust change here is unverifiable — `cargo check` itself fails on
missing system libraries, unrelated to any code change (see
`docs/DEVELOPMENT.md`). Items needing backend work are listed, unstarted,
at the end.

- [x] **E3. Restore the last session on launch.** A local-only snapshot
  (never the vault, matching `recentVaults`'s pattern), taken on lock and
  on quit, restored once nothing is open after unlock. "Don't reconnect
  production" is the default; it works by not reopening those tabs at
  all, rather than opening them disconnected.

- [x] **E4. Drag files onto a terminal to upload**, and drag text to
  paste it. Reuses the exact SFTP commands the SFTP view itself calls
  (`sftp_open`/`transfer_start`, already shipped, nothing new added to
  the backend) to open a throwaway session, upload, and type the
  resulting remote path at the cursor. **Narrower than specified**:
  scoped to the active pane only, not whichever split pane the cursor
  is over — the drop event carries a screen position, not a pane id,
  and converting that position to a pane would need physical→logical
  pixel math this sandbox can't check against a real window. No live
  progress percentage either, just a start and a finish/error toast;
  the existing transfer queue lives inside `SftpView`'s own component
  state, and lifting it to a shared store felt like too large a change
  to make blind.

- [x] **E5. Clickable paths and smarter links.** A new link provider
  (`src/lib/termlinks.ts`, unit-tested) finds absolute/`./relative`
  paths and `host:port` pairs in the terminal; Ctrl+click a path browses
  to its folder in SFTP (resolved against the OSC 7 cwd for relative
  ones), Ctrl+click a `host:port` opens Quick connect pre-filled.
  **Narrower than specified**: no `file:line` parsing, and it browses to
  the folder rather than opening a quick look or the editor directly —
  getting to either of those from here needs a deeper hook into
  `FilePane` than exists today. Scoped to saved-host panes only: telnet,
  serial and ad-hoc sessions have no SFTP target and no reliable cwd.

- [x] **E6. Inline command suggestions — reassessed, not built.** The
  command palette already offers this host's (and recent hosts')
  history, fuzzy-searchable, re-run in the right pane. A ghost-text
  overlay inside the live terminal was judged too easy to get subtly
  wrong with no running browser to check it against. The plan's
  suggested **Ctrl+R** wasn't bound to anything: it's bash/zsh's own
  reverse-history-search, used constantly, so claiming it app-wide would
  break shell editing in every session, the same class of problem as
  the Alt+Arrow conflict fixed in Phase C.

- [x] **E8. Expiry and hygiene warnings → Security review page**
  (`Vault → Security review`, also reachable from the command palette).
  Flags a certificate within 14 days of `valid_before` or already past
  it, an RSA key under 3072 bits or any DSA key, a key older than 5
  years, a host using a saved password while the Key Manager has a key,
  and a password identity whose record hasn't changed in over a year.
  Two small parsers back this, both unit-tested against **real
  `ssh-keygen`-generated certificates and keys** (ed25519/RSA/ECDSA/DSA,
  with and without an expiry), not just hand-built byte strings:
  `src/lib/sshcert.ts` (OpenSSH certificate validity) and
  `src/lib/sshkeyinfo.ts` (algorithm and RSA/DSA modulus size).
  **Narrower than "one-click fixes"**: each finding has a "Review" button
  that opens the right host/identity/Keys view, not an automated fix —
  an automated fix (rotate a key, change a password) is exactly the kind
  of action that needs care I can't give it without seeing it run.
  "Password hasn't rotated" is a proxy from the vault record's own
  `updated_at`, since the vault doesn't timestamp a password change
  specifically; a label-only edit would reset it too.

- [x] **E9. Connection log and session timeline**
  (`src/lib/stores/connectionlog.svelte.ts`). Records each pane's
  connect/disconnect/error locally (never synced), shown on
  **Vault → Activity** (newest first, "Copy as CSV") and as "Recent
  connections" on the host details card. **Narrower than specified**:
  "Export CSV" copies to the clipboard rather than writing a file —
  there's no generic frontend file-write command, only the SSH-config
  export's own purpose-built one, and adding a new one is exactly the
  kind of backend change this sandbox can't verify.

- [x] **E13. Terminal extras, the frontend-only half.** Word-separator
  setting, screen-reader mode toggle, trim-one-trailing-newline-on-paste
  (off by default — it changes whether a pasted single line submits by
  itself), a warning (reusing the paste-confirm dialog) when a paste
  looks like a private key or an AWS/GitHub/Slack token
  (`looksLikeSecret` in `guard.ts`, tested against a real generated key),
  and "Copy entire buffer" in the terminal's menu (clipboard, for the
  same file-write reason as E9). Triple-click-selects-a-line needed no
  work: it's xterm.js's own default. **Not done**: "paste on right
  click"/"middle click pastes selection" (platform habits, skipped for
  time rather than risk); the sixel/iTerm2 image addon (`@xterm/addon-
  image` isn't a dependency, and its rendering can't be checked without
  a running browser, the same reasoning as skipping font ligatures in
  Phase D); "save buffer to file" is the same clipboard-copy
  substitution as E9's CSV, folded into "Copy entire buffer".

## Not started: needs backend work this sandbox can't verify, or is too
## large to build and ship blind

- [ ] **E1. Interactive authentication prompts (2FA/TOTP, Duo, PAM).**
  Needs `ssh.rs` changes to relay a keyboard-interactive prompt to the UI
  and wait for an answer. **L**, backend.

- [ ] **E2. Install public key on host (ssh-copy-id) and key rotation.**
  Needs a new backend command to authenticate and append to
  `~/.ssh/authorized_keys` over the session. **L**, backend.

- [ ] **E7. Host monitoring widgets and Fleet view.** Sampling CPU/
  memory/disk is reachable frontend-only (`run_on_hosts` already
  executes a command and returns output), but a tile-per-host Fleet view
  with live, colour-coded refresh is a genuinely large UI surface to
  build and ship without seeing it render. Deferred, not attempted.
  **L**.

- [ ] **E10. Login scripts (expect/send).** Matching terminal output and
  sending a response is frontend-only, but storing the rules needs a new
  field on the `Host` record, and the vault's Rust struct may reject an
  unrecognised field on save (strict deserialisation, unverified here).
  **M**, backend-adjacent.

- [ ] **E11. System tray and global hotkey.** Needs the
  `tauri-plugin-global-shortcut` crate as a new Cargo dependency and Rust
  tray wiring. **M**, backend.

- [ ] **E12. Detach tab to a new window.** Each Tauri window is a
  separate webview with its own JS module state, so `ui.tabs` wouldn't
  be shared between two windows without a real cross-window sync
  mechanism (events, a shared store) — a materially different, larger
  feature than "open a window," and multi-window behaviour is exactly
  what's hardest to get right with no display to test against. **L**.

- [ ] **E14. ZMODEM transfers.** Protocol detection in the raw byte
  stream plus coordinating with the file dialogs; large and easy to get
  subtly wrong unverified. **L**.

- [ ] **E15. Hardware keys and YubiKey.** Unchanged from the roadmap:
  depends on E1. **L**, backend.

- [ ] **E16. Localisation.** Extracting every string behind `t()` across
  dozens of files, plus RTL layout, is large on its own merits and not
  attempted this round. **L**.

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
