<script lang="ts">
  import { Copy, Loader2, ShieldCheck } from "lucide-svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Badge from "./Badge.svelte";
  import * as api from "$lib/api";
  import { AUDIT_SCRIPT, DOMAINS, SEVERITIES, SEVERITY_NAMES, auditHost, auditText, bySeverityOrder, type AuditReport, type Check, type Severity } from "$lib/hostaudit";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type Uuid } from "$lib/types";

  let { sessionId, label }: { sessionId: Uuid | null; label: string } = $props();

  let running = $state(false);
  let result = $state<{ at: number; report: AuditReport } | null>(null);
  let error = $state<string | null>(null);

  const TONE: Record<Severity, "danger" | "warning" | "neutral"> = { critical: "danger", high: "danger", medium: "warning", low: "neutral", info: "neutral" };
  const domainName = (c: Check) => DOMAINS.find((d) => d.id === c.domain)?.name ?? c.domain;

  async function run() {
    if (!sessionId || running) return;
    running = true;
    error = null;
    try {
      // Every slow tool in the script is bounded, so a healthy host answers in seconds and the worst case is under this.
      const r = await api.monitor.exec(sessionId, AUDIT_SCRIPT, 120);
      if (!r.stdout.trim()) throw new Error(r.stderr.trim() || `the command ended with status ${r.code ?? "?"}`);
      result = { at: Date.now(), report: auditHost(r.stdout) };
    } catch (e) {
      error = errorMessage(e);
    } finally {
      running = false;
    }
  }

  async function copy() {
    if (!result) return;
    try {
      await writeText(auditText(result.report, result.at, label));
      ui.notify("info", "Audit copied.");
    } catch (e) {
      ui.notify("error", errorMessage(e));
    }
  }

  const failures = $derived(result ? bySeverityOrder(result.report.checks) : []);
  const unchecked = $derived(result ? result.report.checks.filter((c) => c.outcome === "unknown") : []);
  const passed = $derived(result ? result.report.checks.filter((c) => c.outcome === "pass") : []);
  const percent = $derived(result?.report.score.percent ?? null);
  const scoreTone = $derived(percent === null ? "text-fg-muted" : percent >= 85 ? "text-success" : percent >= 60 ? "text-warning" : "text-danger");
</script>

<div class="space-y-3 p-1" data-testid="host-audit">
  <div class="flex flex-wrap items-center gap-2">
    <button class="btn-secondary py-1 text-xs" disabled={running || !sessionId} onclick={() => void run()}>
      {#if running}<Loader2 size={13} class="animate-spin" />{:else}<ShieldCheck size={13} />{/if}
      {result ? "Run the audit again" : "Run the audit"}
    </button>
    {#if result?.report.supported}<button class="btn-ghost py-1 text-xs" onclick={() => void copy()}><Copy size={13} /> Copy as text</button>{/if}
    <span class="text-xs text-fg-muted">
      {#if result}
        Observed {new Date(result.at).toLocaleString()}. Not live: run it again to refresh.
      {:else}
        A read-only check of this host's SSH, firewall, accounts, kernel settings, containers and updates. Nothing is changed, installed or sent anywhere, and nothing runs until you press this.
      {/if}
    </span>
  </div>

  <details class="text-xs text-fg-muted">
    <summary class="cursor-pointer select-none">What it runs</summary>
    <p class="mt-1">
      One script, run as the account this window connected as. It reads files under <code>/etc</code>, <code>/proc</code> and <code>/sys</code>, lists sockets, set-uid files, containers and services, and asks <code>apt</code> what is waiting. It never uses sudo,
      never asks for a password, and never prints a password hash. What that account may not read (the SSH drop-ins, sudoers, the firewall rules are usually root's) is listed as <em>not checked</em> and never counted as a pass.
    </p>
    <pre class="mt-1 max-h-64 overflow-auto rounded-md border border-line bg-base/40 p-2 font-mono text-[11px] leading-snug">{AUDIT_SCRIPT}</pre>
  </details>

  {#if error}<p class="whitespace-pre-wrap rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning" role="alert">{error}</p>{/if}

  {#if result}
    {@const r = result.report}
    {#if !r.supported}
      <p class="rounded-md border border-line px-3 py-2 text-sm" role="status">The security audit reads Linux hosts{r.os ? `; this one reports ${r.os}` : ""}.</p>
    {:else}
      {#if !r.complete}
        <p class="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning" role="alert">The output stopped early (a timeout or a dropped connection), so some checks are missing. Run the audit again.</p>
      {/if}

      <section class="rounded-lg border border-line p-3" aria-label="Security posture">
        <div class="flex flex-wrap items-end gap-x-6 gap-y-2">
          <div>
            <div class="text-xs text-fg-muted">Hardening score</div>
            <div class="text-3xl font-semibold {scoreTone}" data-testid="audit-score">{percent === null ? "–" : `${percent}%`}</div>
          </div>
          <dl class="flex gap-5 text-sm">
            <div><dt class="text-xs text-fg-muted">Passed</dt><dd data-testid="audit-passed">{r.score.passed}</dd></div>
            <div><dt class="text-xs text-fg-muted">Failed</dt><dd data-testid="audit-failed">{r.score.failed}</dd></div>
            <div><dt class="text-xs text-fg-muted">Not checked</dt><dd data-testid="audit-unchecked">{r.score.unknown}</dd></div>
          </dl>
          <div class="flex flex-wrap gap-1.5">
            {#each SEVERITIES as s (s)}{#if r.bySeverity[s]}<Badge tone={TONE[s]}>{r.bySeverity[s]} {SEVERITY_NAMES[s].toLowerCase()}</Badge>{/if}{/each}
          </div>
        </div>
        <p class="mt-2 text-xs text-fg-muted">
          Read as <span class="font-mono">{r.user ?? "an unknown user"}</span>{r.root ? " (root)" : ""}{r.distro ? ` on ${r.distro}` : ""}.
          {#if r.score.unknown}The score counts only what could be judged. Once the {r.score.unknown} unchecked {r.score.unknown === 1 ? "item is" : "items are"} known it will be between {r.score.worst}% and {r.score.best}%.{/if}
          {#if !r.root && r.score.unknown} Connect as root, or have root run the commands below, to check the rest.{/if}
        </p>
      </section>

      <section aria-label="Findings">
        <h3 class="mb-1 text-xs font-semibold">Findings, worst first ({failures.length})</h3>
        {#if failures.length === 0}
          <p class="rounded-md border border-success/30 bg-success/10 px-3 py-2 text-sm text-success">Nothing that could be checked is wrong.</p>
        {/if}
        <ul class="space-y-1.5">
          {#each failures as c (c.id)}
            <li>
              <details class="rounded-md border border-line" open={c.severity === "critical" || c.severity === "high"}>
                <summary class="flex cursor-pointer select-none items-center gap-2 px-3 py-2 text-sm">
                  <Badge tone={TONE[c.severity ?? "info"]}>{SEVERITY_NAMES[c.severity ?? "info"]}</Badge>
                  <span class="font-medium">{c.title}</span>
                  <span class="ml-auto hidden truncate text-xs text-fg-muted sm:inline">{domainName(c)}</span>
                </summary>
                <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5 border-t border-line px-3 py-2 text-xs">
                  <dt class="text-fg-muted">Component</dt><dd class="break-words font-mono">{c.component}</dd>
                  <dt class="text-fg-muted">Finding</dt><dd class="break-words">{c.finding}</dd>
                  <dt class="text-fg-muted">Risk</dt><dd class="break-words">{c.risk}</dd>
                  <dt class="text-fg-muted">Remediation</dt><dd class="break-words">{c.remediation}</dd>
                </dl>
              </details>
            </li>
          {/each}
        </ul>
      </section>

      {#if unchecked.length}
        <section aria-label="Not checked">
          <h3 class="mb-1 text-xs font-semibold">Not checked ({unchecked.length})</h3>
          <p class="mb-1 text-xs text-fg-muted">The account this window connected as could not read what these need. They are not counted as passed.</p>
          <ul class="space-y-1.5">
            {#each unchecked as c (c.id)}
              <li>
                <details class="rounded-md border border-line">
                  <summary class="flex cursor-pointer select-none items-center gap-2 px-3 py-2 text-sm"><Badge tone="neutral">Not checked</Badge><span class="font-medium">{c.title}</span><span class="ml-auto hidden truncate text-xs text-fg-muted sm:inline">{domainName(c)}</span></summary>
                  <dl class="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1.5 border-t border-line px-3 py-2 text-xs">
                    <dt class="text-fg-muted">Component</dt><dd class="break-words font-mono">{c.component}</dd>
                    <dt class="text-fg-muted">Why</dt><dd class="break-words">{c.finding}</dd>
                    <dt class="text-fg-muted">To check</dt><dd class="break-words">{c.remediation}</dd>
                  </dl>
                </details>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if passed.length}
        <details aria-label="Passed checks">
          <summary class="cursor-pointer select-none text-xs font-semibold">Passed ({passed.length})</summary>
          <ul class="mt-1 space-y-1 text-xs">
            {#each passed as c (c.id)}<li class="flex gap-3"><span class="w-56 shrink-0 font-medium">{c.title}</span><span class="break-words text-fg-muted">{c.finding}</span></li>{/each}
          </ul>
        </details>
      {/if}
    {/if}
  {/if}
</div>
