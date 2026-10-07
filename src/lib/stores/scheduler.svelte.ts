// Runs scheduled runbooks while the app is open. A schedule that comes due while the vault is locked, or that would
// touch a production host it wasn't allowed to, is skipped and says so; nothing is caught up later.
import * as api from "$lib/api";
import { isDue } from "$lib/schedule";
import { settings } from "$lib/stores/settings.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage } from "$lib/types";

const running = new Set<string>();
const CHECK_EVERY_MS = 30_000;

/** Why a schedule can't run now, or null. */
export function blocker(id: string): string | null {
  const s = settings.prefs.schedules.find((x) => x.id === id);
  if (!s) return "that schedule is gone";
  const rb = vaultStore.runbooks.find((r) => r.id === s.runbookId && !r.deleted);
  if (!rb?.data) return "its runbook was deleted";
  const hosts = s.hostIds.map((h) => vaultStore.hostById.get(h)?.data).filter(Boolean);
  if (hosts.length === 0) return "none of its hosts exist any more";
  if (!s.allowProduction && hosts.some((h) => vaultStore.effectiveEnv(h) === "production")) return "it includes a production host and wasn't allowed to run on those";
  return null;
}

export async function runSchedule(id: string, now = Date.now()): Promise<void> {
  const s = settings.prefs.schedules.find((x) => x.id === id);
  if (!s || running.has(id)) return;
  // Counted as started now, whatever happens, so a schedule that can't run doesn't retry every half minute.
  s.lastRun = now;
  const rb = vaultStore.runbooks.find((r) => r.id === s.runbookId && !r.deleted)?.data;
  const why = blocker(id);
  if (why || !rb) {
    ui.notify("error", `Scheduled run of "${rb?.name ?? "a runbook"}" skipped: ${why}.`);
    return;
  }
  const check = await api.runbookCheck(rb.body).catch(() => null);
  if (!check || check.problems.length || check.params.some((p) => p.kind === "file" && !p.optional)) {
    ui.notify("error", `Scheduled run of "${rb.name}" skipped: the runbook has problems or needs a file, which a schedule can't give.`);
    return;
  }
  running.add(id);
  const runId = crypto.randomUUID();
  try {
    await api.runbookStart(runId, rb.body, { ...s.params }, {}, [...s.hostIds], true, (e) => {
      if (e.event !== "done") return;
      running.delete(id);
      void api.runbookHistory.get(runId).then((r) => {
        const failed = r?.hosts.filter((h) => h.ok === false).length ?? 0;
        if (failed) ui.notify("error", `Scheduled run of "${rb.name}": ${failed} of ${r?.hosts.length} hosts failed. See Runbooks → History.`);
      });
    });
  } catch (e) {
    running.delete(id);
    ui.notify("error", `Scheduled run of "${rb.name}" didn't start: ${errorMessage(e)}`);
  }
}

export function startScheduler(): () => void {
  const timer = setInterval(() => {
    if (!vaultStore.unlocked) return;
    const now = Date.now();
    for (const s of settings.prefs.schedules) if (isDue(s, now)) void runSchedule(s.id, now);
  }, CHECK_EVERY_MS);
  return () => clearInterval(timer);
}
