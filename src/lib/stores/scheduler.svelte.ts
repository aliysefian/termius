// Runs scheduled runbooks while the app is open. A schedule that comes due while the vault is locked, or that would
// touch a production host it wasn't allowed to, is skipped and says so; nothing is caught up later.
import * as api from "$lib/api";
import { isDue, retryPlan } from "$lib/schedule";
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

/** A try again of the hosts that failed: which ones, and how many retries have been made. */
export interface Retry {
  hostIds: string[];
  attempt: number;
}

export async function runSchedule(id: string, now = Date.now(), retry?: Retry): Promise<void> {
  const s = settings.prefs.schedules.find((x) => x.id === id);
  if (!s || running.has(id)) return;
  // Counted as started now, whatever happens, so a schedule that can't run doesn't retry every half minute.
  // (A retry is the same run again: it doesn't move the schedule on.)
  if (!retry) s.lastRun = now;
  // A one-time schedule is spent once it has started, whether or not it could run.
  if (s.when.kind === "once") s.enabled = false;
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
  const attempt = retry?.attempt ?? 0;
  try {
    await api.runbookStart(runId, rb.body, { ...s.params }, {}, retry ? [...retry.hostIds] : [...s.hostIds], true, (e) => {
      if (e.event !== "done") return;
      running.delete(id);
      void api.runbookHistory.get(runId).then((r) => {
        const failedHosts = r?.hosts.filter((h) => h.ok === false) ?? [];
        if (!failedHosts.length) return;
        const plan = retryPlan(s, attempt, failedHosts.map((h) => h.host_id), Date.now());
        const again = plan ? ` It will try those ${failedHosts.length === 1 ? "host" : "hosts"} again in ${Math.round((plan.at - Date.now()) / 60_000)} minutes, if SSHVault is still open.` : "";
        ui.notify("error", `Scheduled run of "${rb.name}": ${failedHosts.length} of ${r?.hosts.length} hosts failed. See Runbooks → History.${again}`);
        // The timer lives in this app: closing it ends the retries (a schedule is not a background service).
        if (plan) setTimeout(() => void runSchedule(id, Date.now(), { hostIds: plan.hostIds, attempt: attempt + 1 }), Math.max(0, plan.at - Date.now()));
      });
    }, s.rollback === true && check.rollback_steps > 0);
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
