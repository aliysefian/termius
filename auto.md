# Smart Completion (auto-complete) for SSHVault

Tasks and Definition of Done for an agent, in the style of `feat.md`. No AI is
part of this work: every suggestion comes from the person's own history, their
snippets, bundled command data, and the host they are connected to. The whole
feature can be switched on and off in **Settings**.

---

## 1. Prompt for the agent

> You are adding **Smart Completion** to SSHVault (Tauri v2 + Rust + SvelteKit +
> xterm.js, at `/home/aliyousefian/termius`). While the person types at a shell
> prompt in a terminal tab, SSHVault suggests the rest of the command: first a
> faint inline suggestion from their history, then a popup (Tab or Ctrl+Space)
> with snippets, command options and remote file paths. Nothing is ever sent to
> the shell until the person accepts it, and Enter still goes through the
> existing safety checks.
>
> Read `auto.md` fully, then build the tasks in section 5 in order (AC0 first).
> For each task: write the code, write the tests, run the project checks
> (`pnpm check`, `pnpm test`, `cargo +1.99.0 clippy --all-targets -- -D
> warnings`, `cargo test`), and look at the result in the headless browser with
> mocked IPC (see `feat.md`, "Tooling and traps"). Tick a DoD line only when you
> have run what it says. If you could not (for example no real server), leave it
> unticked and write exactly what was not verified.
>
> Hard rules:
> 1. **Off-switch.** The master setting turns every part off immediately, with
>    no restart. When it is off no suggestion code runs, no extra SSH channel
>    is opened, and nothing is recorded because of it.
> 2. **Never type for the person.** Suggestions are display only. Accepting
>    inserts text at the prompt; it never presses Enter.
> 3. **Secrets stay out.** No suggestion, stored history entry or remote lookup
>    may contain something `looksLikeSecret` flags. Nothing leaves the computer
>    except the SSH connection the person already has.
> 4. **Don't break the terminal.** No suggestions in full-screen programs (vim,
>    tmux, less), at password prompts, while a command runs, or when the line
>    can't be known. A missing suggestion is fine; a corrupted prompt is not.
> 5. Match the surrounding code (comments, naming, Svelte 5 runes). Don't touch
>    unrelated files.

---

## 2. What other products do

| Product | What it offers | How it works | Take | Avoid |
|---|---|---|---|---|
| **Termius** ([docs](https://docs.termius.com/terminal/autocomplete-and-shell-integration)) | Suggestions from history, paths, built-in commands and arguments, and snippets | Shell integration; over SSH it opens **extra exec channels** next to the shell to read the folder and history | Snippets in the same list; paths and history from the host; nothing installed on the host; switch in Settings → Terminal | Needs OpenSSH 7.2+ and bash on the host; a server with `MaxSessions` below 2 can drop the connection when the extra channel is refused. No PowerShell/CMD. Handle a refused channel quietly |
| **Warp** ([autosuggestions](https://docs.warp.dev/terminal/command-completions/autosuggestions/), [completions](https://docs.warp.dev/terminal/command-completions/)) | Faint inline suggestion from history and completions; Tab menu with fuzzy matching | Own terminal with shell hooks | → or Ctrl+F accepts all, Ctrl+→ one word; Tab can be rebound so the menu moves to Ctrl+Space; fuzzy matching in the menu | Anything that reads terminal content without clear consent |
| **Fig / Amazon Q CLI** ([specs](https://github.com/withfig/autocomplete), MIT) | IDE-style popup for hundreds of CLIs: subcommands, options, argument kinds, descriptions | **Declarative specs** (TypeScript) with *generators* that run a shell command to produce suggestions (for example git branches) | Data, not code, per tool; the specs are MIT and can seed ours | Fig was sunset in 2024 and is tied to the Amazon Q CLI; don't depend on it or run Node at run time |
| **inshellisense** ([overview](https://deepwiki.com/microsoft/inshellisense)) | The same popup for 600+ tools in any terminal | A PTY wrapper that runs the shell and draws over it, reusing Fig specs | Shows the specs can be interpreted outside Fig | A wrapper redrawing over the screen is fragile; we own the terminal widget, so draw inside it |
| **fish / zsh-autosuggestions** ([plugin](https://github.com/zsh-users/zsh-autosuggestions)) | Gray inline suggestion; → or End accepts | Strategies in order: `history` (most recent match), `completion`, `match_prev_cmd` (most recent match that followed the same previous command) | The order, and "previous command" ranking | If the person's shell already does this, two grey suggestions collide: provide the switch and say so in the docs |
| **Atuin** ([atuin.sh](https://atuin.sh/)) | Fuzzy history with context: folder, duration, exit code, host | SQLite history; a `history_filter` keeps sensitive commands out | Rank by folder and success; keep failures out; filter secrets; store per host | Sync is not part of this work (our vault could do it later) |
| **Shell integration (OSC 133)** ([notes](https://vtdn.dev/docs/osc/osc133/)) | A: prompt starts, B: input starts, C: output starts, D: finished with exit code | Escape sequences the shell prints (VS Code, iTerm2, WezTerm, kitty, Windows Terminal) | We already parse it; it is the reliable way to know where the typed line begins | Using the `LineTracker` guess as the main source |

**What we do differently:** completion inside the app that already owns the SSH
connections, the vault, the snippets and the safety rules, with a per-host
override and an honest off switch.

---

## 3. What this project already has

- **Terminal:** `TerminalPane.svelte` (xterm.js) feeds `LineTracker`
  (`src/lib/guard.ts`; follows typed keys, says `null` when the line can't be
  known) and parses **OSC 133 and OSC 7** (`src/lib/shellintegration.ts`,
  `CommandTracker`) for prompts, commands, exit codes and the current folder.
- **History:** `settings.recordCommand(hostId, command, exit)` keeps a
  per-computer, per-host history **only when `rememberCommands` is on** (off by
  default because commands can contain secrets), in `localStorage`
  (`sshvault.history.v1`).
- **Safety:** `looksLikeSecret`, `matchDestructive` and the paste checks in
  `guard.ts`.
- **Snippets:** vault records with `{{variables}}`.
- **Settings:** `Prefs` in `src/lib/stores/settings.svelte.ts`, the page in
  `SettingsPanel.svelte` (searchable sections), per-host options in
  `HostForm.svelte`, shortcut overrides in `prefs.keybindings`.
- **Remote exec:** `containers::transport::SshShell` shows how to run a quoted
  command over a connection's exec channels (`shell_quote`); `ssh.rs` owns the
  connection; hostile-name test patterns exist from the SCP and FTP work.

Missing today: drawing over the cursor line, an in-memory history when
`rememberCommands` is off, remote lookups from a terminal pane, a per-host
override.

---

## 4. Design

**Two layers, each with its own switch:**
1. **Inline suggestion**: faint ghost text after the cursor, from history.
2. **Popup** (Tab or Ctrl+Space): snippets, history matches, command options and
   subcommands from bundled specs, and remote file paths.

**Knowing the line.** Use OSC 133 B (input starts) up to the cursor, read from
the xterm buffer, as the source of truth. Fall back to `LineTracker` only when
the shell has no integration. If neither can tell, show nothing. Treat these as
"no suggestions": alternate screen active, mouse tracking on, a command running
(`commands.running`), a line edited with arrows or history in a way we can't
follow, and a prompt that looks like a password prompt (the text before the
cursor matches `/pass(word|phrase)|passcode|PIN|token/i`).

**Drawing.** An overlay inside the terminal element, positioned from the cell
size and cursor position (xterm decorations or a sibling element). It never
writes to the PTY, follows resize, zoom, scrolling and theme changes, and goes
away on any key that doesn't continue the typed prefix.

**Accepting.** `→` or `End` at the end of the line takes the whole inline
suggestion, `Ctrl+→` one word, `Tab` opens the popup (or accepts, if the person
rebinds it), `Esc` dismisses. Accepting sends only the missing characters to the
pane, exactly as typing would, so `LineTracker`, history and the safety code see
them. The keys are `ACTIONS` entries, rebindable in Settings.

**Sources and ranking.**
- *History:* prefix match first, most recent first; prefer the same host, the
  same folder, exit code 0, and entries that followed the same previous command.
  Fuzzy matching only in the popup. Kept in memory; written to storage only
  when `rememberCommands` is on. Without it, a per-session in-memory history
  that is never written.
- *Snippets:* by label and by command text.
- *Command specs:* a compact JSON schema (subcommands, options with
  descriptions, argument kinds: file, folder, host, enum, free text), generated
  at development time from the MIT-licensed `withfig/autocomplete` for the ~60
  most used commands (git, docker, kubectl, systemctl, journalctl, apt, dnf,
  npm, ssh, scp, tar, grep, find, chmod, ...); licence kept in
  `THIRD_PARTY.md`. No Node at run time. Generators are limited to a fixed
  allowlist of read-only commands.
- *Remote paths and generators:* a backend lookup over the session's SSH
  connection using **an extra exec channel**, never the shell channel. Every
  name reaches the host single-quoted. If the server refuses a channel, stop
  quietly for that session and don't try again. Cache per folder for a few
  seconds, debounce 150 ms, cancel stale lookups, cap entries and time.
- *Local shell tabs:* history, snippets, specs and local paths; no SSH.
- *Out of scope:* Telnet and serial consoles, PowerShell and CMD (say so in the
  docs, as Termius does).

**Settings** (Settings → Terminal, searchable):
- **Smart completion**: master switch. Default **on**, because every part is
  display-only, local and in memory; the owner can flip the default (section 7).
- Sub-switches: inline suggestions; popup menu; include snippets; include
  command options; look up remote paths.
- **Per-host override** in the host form: Use default / Always on / History only
  / Off. Production hosts default to History only.
- History persistence stays governed by the existing "Remember commands"
  setting; the page says so in one sentence.
- Keys for accept, accept word, open menu and dismiss are rebindable.

**Privacy and safety:**
- Entries that `looksLikeSecret` flags, commands typed with a leading space (the
  shell convention for "don't record"), and commands that failed to start are
  never stored or suggested.
- Accepting never submits. Enter goes through the existing destructive-command
  confirmation on production hosts.
- No network traffic except the SSH exec channel to the host the person is
  already connected to.

**Performance:** key to ghost text under 16 ms with 10,000 history entries; no
IPC per keystroke for history; at most one remote lookup per 150 ms; the overlay
must not slow typing or large output (measure with a 10 MB `cat` and 200
pasted characters).

**Accessibility:** works with `screenReaderMode` (ghost text off, popup items
announced through a live region), respects reduced motion, fully keyboard
operable.

---

## 5. Tasks and Definition of Done

Do them in order. Each is small enough to commit on its own.

### AC0. The switch, the settings and knowing the line

Everything later tasks hang on, with nothing visible yet.
- Add `Prefs`: `smartCompletion` (master), `acInline`, `acMenu`, `acSnippets`,
  `acOptions`, `acRemotePaths`. Settings UI under Terminal, with the sentence
  tying history to "Remember commands".
- Add `Host.completion?: "on" | "off" | "history"` (absent means default), the
  host-form control, and the Rust field with `skip_serializing_if`.
- `src/lib/completion/line.ts`: a pure module that, given the xterm buffer
  reader, the OSC 133 marks and `LineTracker`, returns `{ text, cursorAtEnd,
  reliable }`, or `null` for the "no suggestions" cases in section 4.

DoD:
- [x] The settings and the host control exist, persist, and default as in
      section 4; an old host loads without the new field and writes none.
      (Settings → Terminal → Smart completion and the host form's Automation
      tab were checked in a headless browser with a mocked backend; the Rust
      field has a vault round-trip test.)
- [x] `line.ts` returns `null` in the alternate screen, with mouse tracking on,
      while a command runs, at password-like prompts, and after history or
      arrow edits when there is no shell integration.
- [x] Unit tests with recorded byte streams from bash, zsh and fish, with and
      without OSC 133, including a wrapped long line and a multi-byte line.
      The sessions were recorded from bash 5.2.21, zsh 5.9 and fish 3.7.0 by
      `src/lib/__tests__/completion/record.py`. Not recorded: fish's Up key
      (in a bare pseudo-terminal fish 3.7 recalled nothing), fish 4, macOS
      shells.
- [x] With the master switch off, a test proves no completion code is reached
      (a terminal that throws if anything reads it).

Found while doing this, for the next tasks:
- **fish 3.7 sends no OSC 133 at all**, although Settings said "fish 3.6 and
  later emit these sequences by itself". The snippet shown in Settings is now a
  real one (verified by recording). fish 4.0 to 4.2 send the prompt and command
  marks but not the end of the prompt (that arrives in 4.3), so there is no
  input start and the reader gives no suggestions there; the snippet fixes it.
- **zsh and fish place each row of a wrapped line with the cursor**, so xterm
  doesn't mark the rows as wrapped. The reader treats a row as continuing when
  the one before it is full to the last column.
- **bash's silent `read` prints no newline**, so without shell integration the
  next prompt sits on the "Password:" row and gets no suggestions until the next
  line. That is the safe side, and it is recorded as the expectation.
- A right-hand prompt (zsh `RPROMPT`) or the shell's own grey suggestion to the
  right of the cursor makes `cursorAtEnd` false, so no suggestion is shown there.
- Settings and the host control did nothing until AC1 (now done).

### AC1. History index and the inline suggestion

- `src/lib/completion/history.ts`: in-memory index (per host plus the session),
  the ranking in section 4, secret and leading-space filtering, size caps;
  persistence only through the existing `rememberCommands` path.
- The overlay in `TerminalPane.svelte` (a new `CompletionOverlay.svelte`) and the
  key actions in `ACTIONS`.

DoD:
- [x] Ghost text matches the terminal's font, size, line height, padding, zoom
      and theme; follows resize and scrolling; never overlaps the next line.
      The box is placed from the terminal's own screen element and cell grid
      (so padding, line height and zoom come with it), is one cell high, and is
      cut off at the last column. Scrolled back, it is hidden. Browser test:
      starts on the cursor's cell, stays inside the terminal, larger after
      zoom. **Resize hides it until the next prompt** (the terminal reflows the
      line, and AC0's reader refuses to guess); it comes back on the new grid.
      Not verified: a light theme, a changed line height or padding (the code
      reads them from the grid, no test changes them), the DOM renderer (the
      headless browser used WebGL).
- [x] → / End accepts, Ctrl+→ takes one word, Esc dismisses; accepted text goes
      through the normal input path (history and `LineTracker` see it).
      Browser test covers all four, that accepting never sends Enter, and that
      Right and Esc go to the shell when nothing is showing. The accepted text
      goes through `typed()`, the same function as typed keys; no test reads
      `LineTracker` or the saved history after an accept.
- [x] With 10,000 history entries, key to ghost text is under 16 ms (measured
      and written down). `HistoryIndex.suggest` over 10,000 entries, 350 calls
      per run, three runs: worst 2.3, 4.9 and 3.3 ms, mean 0.34 to 0.39 ms
      (also a vitest test with a 16 ms limit). That is the search; reading the
      line and placing the box are a few buffer reads and one layout read, not
      timed on their own.
- [x] Ranking tests: recency, same host, same folder, exit 0, previous command;
      a secret-looking entry and a leading-space entry are never offered.
- [x] Without "Remember commands", suggestions come from this session only and
      nothing is written to storage (a test checks `localStorage`). The index
      test makes `localStorage` throw; the browser test reads all of it after
      typing commands. With it on, `recordCommand` refuses secret-looking and
      leading-space commands (unit test).
- [x] Headless-browser test with mocked IPC: type, see, accept, dismiss, resize.
      `src/lib/__tests__/e2e/completion.py` with `mock.js` (a fake shell that
      sends OSC 133): 26 checks, all passing. It needs `pnpm dev` running and is
      not part of `pnpm test`.

Found while doing this:
- The first secret filter missed `API_TOKEN=...` (`\b` doesn't fire after `_`);
  the unit test caught it. Rules are best-effort by nature; the list is in
  `history.ts`.
- **The terminal rounds its cell width down to whole pixels** (7 px for a font
  whose letters are 7.8 px wide), so text drawn as one run drifts off the grid
  within a few letters. The ghost draws each character in its own cell.
- Local terminals have no host, so "Remember commands" never saved anything for
  them; the session index still works there.
- Only the `completion/*` files, the pane and `shortcuts.ts` changed outside
  settings; `CommandRecord` gained `leadingSpace`, `HistoryEntry` gained
  optional `cwd` and `prev`.

### AC2. The popup, snippets and fuzzy matching

- `CompletionMenu.svelte`, opened by Tab (configurable) or Ctrl+Space; items
  from history and snippets (label and body), grouped, with a short description.

Decided while doing this: **Tab is not taken by default.** Tab is the shell's own
completion, and taking it would break the first thing a person does at a prompt
(the "don't break the terminal" rule). Ctrl+Space opens the list; "Open the
suggestion list (Tab)" is an unbound action in Settings → Shortcuts that gives
Tab to it, and Tab still reaches the shell whenever the list can't open.

DoD:
- [x] Keyboard only: arrows, Enter or Tab to accept, Esc to close, typing refines.
      The list follows the line: a typed key goes to the shell as usual and the
      list is rebuilt from the new line, so typing narrows it. Up, Down, Enter
      and Tab belong to the list only while it is open. Choosing puts the entry
      on the line in place of the typed text (backspaces, then the text, through
      the normal input path); it never presses Enter. Browser test.
- [x] Fuzzy matching with the match highlighted; a snippet with `{{variables}}`
      opens the existing variable dialog instead of inserting raw text.
      The typed text stays on the line until the dialog is answered, then the
      answered snippet replaces it. Several-line snippets are not listed (typing
      one would run it). Unit tests for matching and highlighting; browser test
      for the dialog.
- [x] Positioned inside the pane, flips above the line near the bottom, never
      leaves the window; items are announced to screen readers.
      `placeMenu` unit tests; the browser test (700 x 500 window, cursor on the
      last row) opens it above the line and inside the window. A live region
      says how many entries there are and which is chosen; ghost text is off
      with `screenReaderMode`. Not verified: a real screen reader.
- [x] The menu never opens at a password prompt or in a full-screen program.
      Browser test: a command waiting for a password, and a full-screen program.
      The menu also follows the switches (popup off, snippets off, master off:
      Ctrl+Space reaches the shell).

Found while doing this:
- **Snippets with `{{variables}}` never ran from the values dialog**: it set
  `ui.modal = null` and then read its props, which come from `ui.modal`, and
  threw. Fixed in `SnippetVarsDialog.svelte` (listed in the changelog).
- The keys are registered with the pane that has the keyboard while smart
  completion is on, so with it off no key reaches this code at all.
- The list's search is one pass of the fuzzy matcher over the index, once per
  key while the list is open. Timed over the index's full size of 10,000 entries:
  mean 4.8 ms, worst 17.8 ms (one sample over the inline suggestion's 16 ms
  budget; the real index holds at most 1,000 per host, 5,000 overall, so the
  usual cost is a fraction of that). Not timed in the browser.

### AC3. Command specs: options and subcommands

- A compact JSON schema, plus a development-time script
  (`scripts/build-completion-specs.mjs`, output committed) that converts the
  chosen specs from `withfig/autocomplete` and records the licence;
  `THIRD_PARTY.md`.
- The engine parses the typed line into command, subcommand, option and argument
  position, and offers what fits there, with descriptions.

DoD:
- [ ] About 60 commands included, under 400 KB gzipped, loaded lazily.
- [ ] Parsing handles quotes, pipes, `&&`, `;`, `sudo` and `env` prefixes,
      `--opt=value`, and combined short flags (`-xzf`).
- [ ] Tests from real command lines for git, docker, kubectl and systemctl.
- [ ] The licence text and attribution are present, and the build script is
      documented.

### AC4. Remote paths and generators over an exec channel

- Rust `completion.rs`: `list_dir(session, dir)` and an allowlisted generator
  runner over an extra exec channel of the pane's connection; every argument
  single-quoted (`shell_quote`); output and time caps; a quiet "don't try again"
  state per session when a channel is refused.
- Frontend: debounce, cache, cancel; the "look up remote paths" switch; `~` and
  relative paths using the pane's current folder (OSC 7).

DoD:
- [ ] Against a real `sshd`: lists a folder, completes `~/` and relative paths,
      and handles names with spaces, quotes and `$(x)` (a canary proves nothing
      runs).
- [ ] Against `MaxSessions 1`: the lookup fails quietly, the session stays
      connected, and no further attempts are made in that session.
- [ ] A folder with 50,000 entries returns the first N within the time cap and
      says it was cut.
- [ ] Works through a jump host; with the switch off no channel is opened
      (checked in a test).

### AC5. Polish, per-host rules and documentation

- Production hosts default to History only (documented); a docs note that a
  shell with its own suggestions (fish, zsh-autosuggestions) will show two.
- `docs/USAGE.md` (what it does, keys, settings, limits), `README.md` feature
  line, `CHANGELOG.md` under Unreleased.

DoD:
- [ ] Every line of section 6 is ticked, or each exception is written down.
- [ ] A manual pass over bash, zsh, fish, tmux, vim, `top`, a password prompt,
      `sudo`, a multi-line paste and a very long line.

---

## 6. Definition of Done for the whole feature

- [ ] **Setting:** Smart completion can be turned on and off in Settings; off
      means no overlay, no key handling, no remote lookups and no recording;
      the change applies to open tabs immediately.
- [ ] **Per host:** the host form overrides it; the override is respected and
      saved in the vault.
- [ ] **Inline:** history suggestions appear after the cursor, accept with → /
      End, word by word with Ctrl+→, dismiss with Esc, and vanish when the typed
      text no longer matches.
- [ ] **Popup:** Tab or Ctrl+Space lists snippets, history, options and paths
      with descriptions; arrows and Enter select; typing filters; Esc closes.
- [ ] **Safe:** nothing appears in vim, tmux or less, at password prompts, while
      a command runs, or when the line is unknown; accepting never submits;
      secret-looking commands are never stored or suggested.
- [ ] **Remote:** paths and generators come over an extra exec channel with safe
      quoting; a server that refuses the channel causes no disconnect and no
      repeated attempts.
- [ ] **No regressions:** typing, pasting, resize, zoom, themes, split panes and
      synchronized input behave as before with the feature on.
- [ ] **Tested:** unit tests for the engine; a recorded-session test set; a real
      `sshd` test for the remote lookup (including `MaxSessions 1` and a hostile
      directory name); headless-browser checks of the overlay; and a list of what
      was not verified (for example macOS, a real Tauri window).
- [ ] **Documented:** `docs/USAGE.md`, `README.md`, `CHANGELOG.md`, and the
      status of each task ticked in this file.

## 7. Decisions for the owner

1. **Default on or off.** Recommended **on** (display-only, local, in memory). To
   make it opt-in, change one default in AC0. Default must be off
2. **Persisted history.** Keep tying it to "Remember commands" (recommended), or
   add a vault-synced encrypted history later (Atuin style). Not part of this work.
3. **Specs.** Seed from `withfig/autocomplete` (MIT, recommended) or write the
   ~60 specs by hand.
4. **Identities at password prompts** (Termius offers this): not included,
   because typing a stored password into a prompt is a different risk from
   completing a command. It could be a separate, explicit feature. (this feat not need must be secure no need to do like termius)
