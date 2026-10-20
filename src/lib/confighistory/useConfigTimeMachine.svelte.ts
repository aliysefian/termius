// When to keep a copy of a remote file, and the keeping. Three entrances: a command typed in a terminal that edits
// a watched file, the SFTP editor opening one, and a rollback about to overwrite one. All of them end in the Rust
// store, which keeps a version only when the content differs from the newest one.
import * as api from "$lib/api";
import type { CaptureOutcome } from "$lib/api";
import { settings, type Prefs } from "$lib/stores/settings.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import type { Uuid } from "$lib/types";
import { detectEdits, matchesWatched } from "./terminalEditInterceptor";
import { snapshotVault } from "./snapshotVaultStore.svelte";

type Config = Pick<Prefs, "cfgEnabled" | "cfgEnvs" | "cfgPatterns" | "cfgMaxKb" | "cfgPostEdit" | "cfgRetentionDays">;

/** Whether a file on a host in `env` is to be kept under these settings. */
export function wantsCapture(cfg: Config, env: string, path: string): boolean {
  return cfg.cfgEnabled && cfg.cfgEnvs.includes(env) && matchesWatched(path, cfg.cfgPatterns);
}

export const maxBytes = (cfg: Pick<Config, "cfgMaxKb">) => Math.max(1, cfg.cfgMaxKb) * 1024;

/** The user name the host's saved login uses, for the version's record. */
export function sshUserFor(hostId: Uuid): string {
  const host = vaultStore.hostById.get(hostId)?.data;
  const id = vaultStore.effectiveIdentity(host);
  return (id && vaultStore.identityById.get(id)?.data?.username) || "";
}

const envOf = (hostId: Uuid) => vaultStore.effectiveEnv(vaultStore.hostById.get(hostId)?.data) ?? "";

/** Same file asked for twice within this long is read once (a command and the prompt redraw around it). */
const DEBOUNCE_MS = 1500;

class ConfigTimeMachine {
  #recent = new Map<string, number>();
  /** The files an editor command on a pane touches, until it ends (for the version after the edit). */
  #running = new Map<string, { command: string; hostId: Uuid; program: string; paths: string[] }>();

  #outcome(o: CaptureOutcome) {
    if (o.status === "saved" && o.snapshot) snapshotVault.announce(o.snapshot);
    if (o.status === "saved" || o.status === "unchanged") void api.configHistory.prune(settings.prefs.cfgRetentionDays).catch(() => {});
  }

  async #capture(paneId: string, hostId: Uuid, path: string, trigger: string) {
    const key = `${hostId}\0${path}\0${trigger}`;
    const last = this.#recent.get(key) ?? 0;
    if (Date.now() - last < DEBOUNCE_MS) return;
    this.#recent.set(key, Date.now());
    try {
      this.#outcome(await api.configHistory.capture(paneId, hostId, path, trigger, sshUserFor(hostId), maxBytes(settings.prefs)));
    } catch {
      // A copy that could not be made is not worth interrupting anyone for.
    }
  }

  /**
   * A line was just sent to the shell. Looks at it afterwards (the caller has already written the Enter), and reads
   * the files it will change over a separate channel while the editor is still starting up.
   */
  onTerminalCommand(p: { paneId: string; hostId: Uuid | null; line: string; cwd: string | null }) {
    const cfg = settings.prefs;
    if (!cfg.cfgEnabled || !p.hostId || !p.line.trim()) return;
    const env = envOf(p.hostId);
    if (!cfg.cfgEnvs.includes(env)) return;
    const hostId = p.hostId;
    for (const edit of detectEdits(p.line, p.cwd)) {
      const paths = edit.paths.filter((path) => wantsCapture(cfg, env, path));
      if (paths.length === 0) continue;
      const trigger = `Terminal (${edit.program})`;
      this.#running.set(p.paneId, { command: p.line, hostId, program: edit.program, paths });
      for (const path of paths) void this.#capture(p.paneId, hostId, path, trigger);
    }
  }

  /** The command finished: keep the file again, so the version the edit produced is there too. */
  onCommandFinished(paneId: string, command: string) {
    const run = this.#running.get(paneId);
    if (!run || run.command.trim() !== command.trim()) return;
    this.#running.delete(paneId);
    if (!settings.prefs.cfgPostEdit) return;
    for (const path of run.paths) {
      // A different trigger, so the debounce on the first read doesn't swallow this one.
      void this.#capture(paneId, run.hostId, path, `Terminal (${run.program}, after)`);
    }
  }

  /** The SFTP editor opened a file: its text is the state before any edit. */
  async onEditorOpened(hostId: Uuid, path: string, text: string, permissions: string, owner: string) {
    const cfg = settings.prefs;
    if (!wantsCapture(cfg, envOf(hostId), path)) return;
    try {
      this.#outcome(await api.configHistory.store(hostId, path, text, permissions, owner, "SFTP Editor", sshUserFor(hostId), maxBytes(cfg)));
    } catch {
      // see #capture
    }
  }
}

export const timeMachine = new ConfigTimeMachine();
