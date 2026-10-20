// Running a script on a host as root when it takes root, without ever putting a password in a command line.
//
// `execWithSudoFallback` tries the script as the user first (unless told it needs root), and when the host
// says "permission denied" runs it again through `sudo`: first `sudo -n` (works where sudo asks nothing), then,
// if sudo wants a password, with the password fed to `sudo -S` on standard input. The password the person typed is
// kept in memory for the host, for the rest of the app session, and never written anywhere.
import { shq } from "$lib/ops/quote";

export interface ExecResult {
  stdout: string;
  stderr: string;
  code: number | null;
}

/** Runs a script on the host's connection; `stdin` reaches the script's input, not its command line. */
export type Runner = (script: string, timeoutSecs: number, stdin?: string) => Promise<ExecResult>;

export type SudoFailure = "needs-password" | "wrong-password" | "not-allowed" | "failed";

export class SudoError extends Error {
  constructor(
    readonly kind: SudoFailure,
    message: string,
  ) {
    super(message);
  }
}

/** What `sudo` said, as a cause. The same readings as the file editor's save-as-root. */
export function classifySudo(stderr: string, hadPassword: boolean): SudoFailure | null {
  const s = stderr.toLowerCase();
  if (s.includes("incorrect password") || s.includes("sorry, try again") || (hadPassword && s.includes("no password was provided"))) return "wrong-password";
  if (s.includes("a password is required") || s.includes("no tty present") || s.includes("a terminal is required") || s.includes("no password was provided")) return "needs-password";
  if (s.includes("not in the sudoers") || s.includes("is not allowed to") || s.includes("may not run sudo")) return "not-allowed";
  return null;
}

/** The script is the same word to `sh` whoever runs it; only the sudo in front changes. */
export function sudoCommand(script: string, withPassword: boolean): string {
  return `${withPassword ? "sudo -S -p ''" : "sudo -n"} sh -c ${shq(script)}`;
}

/** Output that says the user may not do this, as opposed to the script failing for its own reasons. */
export function looksDenied(r: ExecResult): boolean {
  if (r.code === 0) return false;
  return /permission denied|operation not permitted|must be (?:run as )?root|you must be root|need to be root|are you root|not permitted|root privileges|access denied/i.test(`${r.stderr}\n${r.stdout}`);
}

const passwords = new Map<string, string>();
const rootHosts = new Map<string, boolean>();

export const sudoPasswords = {
  has: (key: string) => passwords.has(key),
  set: (key: string, password: string) => void passwords.set(key, password),
  /** When the connection closes, or the person asks to forget it. */
  forget: (key: string) => {
    passwords.delete(key);
    rootHosts.delete(key);
  },
};

export interface SudoOptions {
  run: Runner;
  script: string;
  timeoutSecs?: number;
  /** Where the password is remembered (a host id). */
  key: string;
  /** `auto`: as the user, then root on "permission denied". `always`: root from the start (a firewall's rules). */
  mode?: "auto" | "always";
  /** Asked for when sudo wants a password: resolves with it, or `null` if the person gave up. `reason` is set after a wrong one. */
  askPassword: (reason?: string) => Promise<string | null>;
}

export interface SudoResult extends ExecResult {
  /** The script ran under sudo. */
  usedSudo: boolean;
}

const MAX_TRIES = 3;

async function isRoot({ run, key }: SudoOptions): Promise<boolean> {
  const known = rootHosts.get(key);
  if (known !== undefined) return known;
  const r = await run("id -u", 10);
  const root = r.code === 0 && r.stdout.trim() === "0";
  rootHosts.set(key, root);
  return root;
}

export async function execWithSudoFallback(opts: SudoOptions): Promise<SudoResult> {
  const { run, script, key, askPassword } = opts;
  const timeout = opts.timeoutSecs ?? 30;
  const mode = opts.mode ?? "auto";
  const root = await isRoot(opts);
  if (root) return { ...(await run(script, timeout)), usedSudo: false };

  if (mode === "auto") {
    const plain = await run(script, timeout);
    if (!looksDenied(plain)) return { ...plain, usedSudo: false };
  }

  let password = passwords.get(key) ?? null;
  // Without a stored password, sudo may still ask for none.
  if (password === null) {
    const quiet = await run(sudoCommand(script, false), timeout);
    const why = classifySudo(quiet.stderr, false);
    if (why === null) return { ...quiet, usedSudo: true };
    if (why === "not-allowed") throw new SudoError("not-allowed", quiet.stderr.trim());
    if (why !== "needs-password") return { ...quiet, usedSudo: true };
  }

  let reason: string | undefined;
  for (let attempt = 0; attempt < MAX_TRIES; attempt++) {
    if (password === null) {
      password = await askPassword(reason);
      if (password === null) throw new SudoError("needs-password", "sudo needs a password and none was given");
    }
    const r = await run(sudoCommand(script, true), timeout, `${password}\n`);
    const why = classifySudo(r.stderr, true);
    if (why === null) {
      passwords.set(key, password);
      return { ...r, usedSudo: true };
    }
    if (why === "not-allowed") throw new SudoError("not-allowed", r.stderr.trim());
    passwords.delete(key);
    password = null;
    reason = "sudo did not accept that password. Try again.";
  }
  throw new SudoError("wrong-password", "sudo did not accept the password");
}
