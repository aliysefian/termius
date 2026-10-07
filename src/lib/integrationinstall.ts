// Putting the shell-integration lines into a host's startup file for the person, instead of making them paste them
// into `.bashrc` by hand. The change is the exact block shown to them first, marked so it can be recognised and
// taken out again, added once (never twice), with a copy of the file kept beside it.
import { SHELL_SNIPPETS } from "./shellintegration";
import { shq } from "./ops/quote";

export type Shell = "bash" | "zsh" | "fish";

export const MARK_BEGIN = "# >>> SSHVault shell integration >>>";
export const MARK_END = "# <<< SSHVault shell integration <<<";
export const BACKUP_SUFFIX = ".sshvault.bak";
const EOF_MARK = "SSHVAULT_INTEGRATION_EOF";

/** The startup file for each shell, as the host's shell expands it. */
export const RC_FILE: Record<Shell, string> = {
  bash: "$HOME/.bashrc",
  zsh: "$HOME/.zshrc",
  fish: "$HOME/.config/fish/config.fish",
};

/** The lines that go into the file, without the markers. */
export function snippetFor(shell: Shell): string {
  const found = SHELL_SNIPPETS.find((s) => s.shell === shell);
  if (!found) throw new Error(`No integration for ${shell}`);
  // The snippet's own first line is a comment naming it; the markers do that here.
  return found.text.replace(/^# SSHVault shell integration\n/, "").trimEnd();
}

/** The block as it lands in the file: a blank line, then the marked lines. */
export function blockFor(shell: Shell): string {
  const body = snippetFor(shell);
  if (body.includes(EOF_MARK) || body.includes(MARK_BEGIN) || body.includes(MARK_END)) throw new Error("The snippet can't hold its own markers");
  return `${MARK_BEGIN}\n${body}\n${MARK_END}`;
}

export const isShell = (s: string): s is Shell => s === "bash" || s === "zsh" || s === "fish";

/** Prints `shell|file|state`: the login shell's name, and whether the block is already in its file. */
export const DETECT_SCRIPT = [
  'sh_name=$(basename "${SHELL:-sh}")',
  'case "$sh_name" in bash) f="$HOME/.bashrc";; zsh) f="$HOME/.zshrc";; fish) f="$HOME/.config/fish/config.fish";; *) f="";; esac',
  'state=none',
  `if [ -n "$f" ] && [ -f "$f" ] && grep -qF ${shq(MARK_BEGIN)} "$f"; then state=installed; fi`,
  'printf "%s|%s|%s\\n" "$sh_name" "$f" "$state"',
].join("\n");

export interface Detected {
  shell: Shell | null;
  /** The login shell's name, whatever it is. */
  name: string;
  file: string;
  installed: boolean;
}

export function parseDetect(out: string): Detected {
  const line = out.trim().split("\n").pop() ?? "";
  const [name = "", file = "", state = ""] = line.split("|");
  return { shell: isShell(name) ? name : null, name, file, installed: state === "installed" };
}

/** Adds the block once. Prints ALREADY if it is there, DONE when it was added. Keeps `<file>.sshvault.bak` first. */
export function installScript(shell: Shell): string {
  return [
    `f="${RC_FILE[shell]}"`,
    'mkdir -p "$(dirname "$f")" || exit 1',
    'touch "$f" || exit 1',
    `if grep -qF ${shq(MARK_BEGIN)} "$f"; then echo ALREADY; exit 0; fi`,
    `cp -p "$f" "$f${BACKUP_SUFFIX}" || exit 1`,
    // A file that doesn't end in a newline would glue the marker onto its last line.
    '[ -s "$f" ] && [ -n "$(tail -c1 "$f")" ] && printf "\\n" >> "$f"',
    `printf "\\n" >> "$f"`,
    `cat >> "$f" <<'${EOF_MARK}'`,
    blockFor(shell),
    EOF_MARK,
    "echo DONE",
  ].join("\n");
}

/** Takes the marked block out again, leaving everything else as it was. Prints GONE, or NOTHING if it wasn't there. */
export function removeScript(shell: Shell): string {
  return [
    `f="${RC_FILE[shell]}"`,
    '[ -f "$f" ] || { echo NOTHING; exit 0; }',
    `grep -qF ${shq(MARK_BEGIN)} "$f" || { echo NOTHING; exit 0; }`,
    `cp -p "$f" "$f${BACKUP_SUFFIX}" || exit 1`,
    // Everything outside the markers is copied across, and the blank line the install added before them goes too.
    `awk -v b=${shq(MARK_BEGIN)} -v e=${shq(MARK_END)} 'BEGIN{skip=0} index($0,b)==1{skip=1; next} skip&&index($0,e)==1{skip=0; next} !skip{print}' "$f" > "$f.sshvault.tmp" || { rm -f "$f.sshvault.tmp"; exit 1; }`,
    'mv "$f.sshvault.tmp" "$f" || exit 1',
    "echo GONE",
  ].join("\n");
}
