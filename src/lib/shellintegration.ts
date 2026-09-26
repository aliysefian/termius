// Shell integration: the escape sequences a shell can emit so the terminal
// knows where prompts, commands and output are (OSC 133, as used by VS Code,
// WezTerm, Kitty and iTerm2) and which directory it's in (OSC 7).
//
// Nothing here touches xterm.js directly, so it can be unit tested; the
// pane feeds it events with buffer line numbers.

/** `file://host/path` → the path, percent-decoded. Null for anything else. */
export function parseOsc7(data: string): string | null {
  const m = /^file:\/\/[^/]*(\/.*)$/.exec(data.trim());
  if (!m) return null;
  try {
    return decodeURIComponent(m[1]);
  } catch {
    return m[1];
  }
}

export type Osc133 =
  | { kind: "prompt" } // A: prompt starts
  | { kind: "command" } // B: user input starts
  | { kind: "output" } // C: command started, output follows
  | { kind: "end"; exit: number | null }; // D[;exit]

export function parseOsc133(data: string): Osc133 | null {
  const [code, arg] = data.split(";");
  switch (code) {
    case "A":
      return { kind: "prompt" };
    case "B":
      return { kind: "command" };
    case "C":
      return { kind: "output" };
    case "D": {
      const n = arg === undefined || arg === "" ? NaN : Number(arg);
      return { kind: "end", exit: Number.isFinite(n) ? n : null };
    }
    default:
      return null;
  }
}

export interface CommandRecord {
  command: string;
  startedAt: number;
  endedAt: number;
  exit: number | null;
  /** First and last buffer line of the output, when known. */
  outputStart: number | null;
  outputEnd: number | null;
}

/** Where the cursor is, in absolute buffer lines and columns. */
export interface Pos {
  line: number;
  col: number;
}

/**
 * Turns OSC 133 marks into command records. `readLine` returns the text of
 * an absolute buffer line (without trailing spaces).
 */
export class CommandTracker {
  /** Absolute lines of every prompt seen, oldest first. */
  prompts: number[] = [];
  /** Finished commands, newest last. */
  records: CommandRecord[] = [];
  #commandStart: Pos | null = null;
  #outputStart: number | null = null;
  #startedAt = 0;
  #command = "";

  constructor(
    private readonly readLine: (line: number) => string,
    private readonly now: () => number = Date.now,
    private readonly maxPrompts = 500,
  ) {}

  /** The shell has emitted at least one mark. */
  get active(): boolean {
    return this.prompts.length > 0 || this.records.length > 0 || this.#commandStart !== null;
  }

  feed(mark: Osc133, cursor: Pos): CommandRecord | null {
    switch (mark.kind) {
      case "prompt":
        this.prompts.push(cursor.line);
        if (this.prompts.length > this.maxPrompts) this.prompts.shift();
        return null;
      case "command":
        this.#commandStart = cursor;
        this.#command = "";
        return null;
      case "output": {
        // The command is what was typed between B and here.
        if (this.#commandStart) {
          // Enter has moved the cursor to a fresh line; the command sits above it.
          const lastLine = cursor.col === 0 ? cursor.line - 1 : cursor.line;
          const lines: string[] = [];
          for (let l = this.#commandStart.line; l <= lastLine; l++) {
            const text = this.readLine(l);
            lines.push(l === this.#commandStart.line ? text.slice(this.#commandStart.col) : text);
          }
          this.#command = lines.join("\n").trim();
        }
        this.#outputStart = cursor.line;
        this.#startedAt = this.now();
        return null;
      }
      case "end": {
        const rec: CommandRecord | null = this.#command
          ? {
              command: this.#command,
              startedAt: this.#startedAt,
              endedAt: this.now(),
              exit: mark.exit,
              outputStart: this.#outputStart,
              // The end mark is printed at the start of the next prompt line.
              outputEnd: this.#outputStart === null ? null : Math.max(this.#outputStart, cursor.line - 1),
            }
          : null;
        if (rec) this.records.push(rec);
        if (this.records.length > 200) this.records.shift();
        this.#commandStart = null;
        this.#outputStart = null;
        this.#command = "";
        return rec;
      }
    }
  }

  /** The prompt line just before `line` (or after, for `dir` = 1). */
  nearestPrompt(line: number, dir: -1 | 1): number | null {
    const before = this.prompts.filter((p) => p < line);
    const after = this.prompts.filter((p) => p > line);
    return dir === -1 ? (before.at(-1) ?? null) : (after[0] ?? null);
  }

  get last(): CommandRecord | null {
    return this.records.at(-1) ?? null;
  }
}

/** Snippets a user pastes into their shell startup file. */
export const SHELL_SNIPPETS: { shell: string; file: string; text: string }[] = [
  {
    shell: "bash",
    file: "~/.bashrc",
    text: `# SSHVault shell integration
__sshvault_prompt() { local e=$?; printf '\\e]133;D;%s\\a\\e]7;file://%s%s\\a\\e]133;A\\a' "$e" "$HOSTNAME" "$PWD"; }
PROMPT_COMMAND="__sshvault_prompt\${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
PS1="$PS1"'\\[\\e]133;B\\a\\]'
PS0='\\[\\e]133;C\\a\\]'`,
  },
  {
    shell: "zsh",
    file: "~/.zshrc",
    text: `# SSHVault shell integration
__sshvault_precmd() { local e=$?; printf '\\e]133;D;%s\\a\\e]7;file://%s%s\\a\\e]133;A\\a' "$e" "$HOST" "$PWD"; }
__sshvault_preexec() { printf '\\e]133;C\\a'; }
autoload -Uz add-zsh-hook
add-zsh-hook precmd __sshvault_precmd
add-zsh-hook preexec __sshvault_preexec
PS1="$PS1"$'%{\\e]133;B\\a%}'`,
  },
  {
    shell: "fish",
    file: "(nothing to add)",
    text: "# fish 3.6 and later emit these sequences by itself.",
  },
];
