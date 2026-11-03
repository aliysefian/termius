<script lang="ts">
  import { Download, Loader2 } from "lucide-svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import Modal from "../Modal.svelte";
  import * as api from "$lib/api";
  import { filterProblem } from "$lib/dbpaging";
  import { databases } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type DbExportFormat, type Uuid } from "$lib/types";

  let { connId, database, table }: { connId: Uuid; database: string; table: string } = $props();

  const FORMATS: { id: DbExportFormat; label: string; note: string }[] = [
    { id: "csv", label: "CSV", note: "One line per row, with a header. Opens in a spreadsheet." },
    { id: "json", label: "JSON", note: "An array with one object per row." },
    { id: "sql", label: "INSERT statements", note: "One INSERT per row. Rows only: no schema, indexes or other tables." },
  ];

  let format = $state<DbExportFormat>("csv");
  let filter = $state("");
  let busy = $state(false);
  let rows = $state(0);
  let jobId = $state<Uuid | null>(null);
  let error = $state<string | null>(null);

  const engine = $derived(databases.engineOf(connId));
  const problem = $derived(filterProblem(filter, engine));
  const session = $derived(databases.sessions[connId]);

  $effect(() => {
    const off = api.db.onTransfer((p) => {
      if (p.job_id === jobId) rows = p.rows;
    });
    return () => void off.then((f) => f());
  });

  async function run(e: SubmitEvent) {
    e.preventDefault();
    if (!session || problem) return;
    const ext = format === "sql" ? "sql" : format;
    const path = await save({ title: `Export ${table}`, defaultPath: `${table.replace(/[^\w.-]+/g, "_")}.${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
    if (!path) return;
    busy = true;
    error = null;
    rows = 0;
    jobId = crypto.randomUUID();
    try {
      const s = await api.db.exportTable(session.sessionId, jobId, { database, table, format, filter: filter.trim() || null, order: [] }, path);
      ui.notify(s.cut_cells ? "warning" : "info", `Exported ${s.rows.toLocaleString()} rows to ${path}.${s.cut_cells ? ` ${s.cut_cells} cell${s.cut_cells === 1 ? " was" : "s were"} longer than 64 KB and cut.` : ""}${s.ordered ? "" : " The table has no key, so the row order was the server's."}`);
      ui.modal = null;
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
      jobId = null;
    }
  }

  async function stop() {
    if (!session || !jobId) return;
    await api.db.cancel(session.sessionId, jobId).catch(() => {});
  }
</script>

<Modal title="Export {table}" onclose={() => !busy && (ui.modal = null)}>
  <form id="export-form" class="space-y-4 text-sm" onsubmit={run}>
    <p class="text-xs text-fg-muted">Writes the rows of <span class="font-mono">{database}.{table}</span> to a file, a chunk at a time, so a large table doesn't fill memory. This is not a backup of the database.</p>
    {#if !session}<p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">This connection isn't open. Connect to it first.</p>{/if}
    <fieldset class="space-y-1.5" disabled={busy}>
      <legend class="mb-1 text-xs font-medium">Format</legend>
      {#each FORMATS as f (f.id)}
        <label class="flex cursor-pointer items-start gap-2 rounded-md border border-line px-3 py-2 {format === f.id ? 'border-accent/50 bg-accent/10' : ''}">
          <input type="radio" name="fmt" class="mt-0.5 accent-input" value={f.id} bind:group={format} />
          <span><span class="block text-xs font-medium">{f.label}</span><span class="block text-[11px] text-fg-muted">{f.note}</span></span>
        </label>
      {/each}
    </fieldset>
    <div>
      <label class="mb-1 block text-xs font-medium" for="exp-filter">Only the rows where <span class="font-normal text-fg-muted">(optional)</span></label>
      <input id="exp-filter" class="input w-full font-mono text-xs {problem ? 'border-danger' : ''}" placeholder="a WHERE condition, e.g. created > '2025-01-01'" bind:value={filter} disabled={busy} aria-invalid={!!problem} />
      {#if problem}<p class="mt-1 text-xs text-danger" role="alert">{problem}</p>{/if}
    </div>
    {#if busy}
      <p class="flex items-center gap-2 text-xs" role="status"><Loader2 size={13} class="animate-spin" /> {rows.toLocaleString()} rows written…</p>
    {/if}
    {#if error}<p class="whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{error}</p>{/if}
  </form>
  {#snippet footer()}
    {#if busy}
      <button class="btn-danger" onclick={stop}>Stop</button>
    {:else}
      <button class="btn-secondary" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-primary" type="submit" form="export-form" disabled={!session || !!problem}><Download size={14} /> Choose a file and export</button>
    {/if}
  {/snippet}
</Modal>
