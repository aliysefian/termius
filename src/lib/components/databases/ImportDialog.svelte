<script lang="ts">
  import { FileUp, Loader2 } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "../Modal.svelte";
  import * as api from "$lib/api";
  import { autoMapping } from "$lib/dbimport";
  import { databases } from "$lib/stores/databases.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type DbCsvPreview, type DbTableInfo, type Uuid } from "$lib/types";

  let { connId, database, table }: { connId: Uuid; database: string; table: string } = $props();

  const ENCODINGS = [
    ["utf-8", "UTF-8"],
    ["windows-1252", "Western European (Windows-1252 / Latin-1)"],
    ["windows-1256", "Arabic (Windows-1256)"],
    ["windows-1251", "Cyrillic (Windows-1251)"],
    ["utf-16le", "UTF-16 (little endian)"],
    ["utf-16be", "UTF-16 (big endian)"],
    ["shift_jis", "Japanese (Shift_JIS)"],
    ["gbk", "Chinese (GBK)"],
  ];
  const DELIMITERS = [
    ["", "Detect"],
    [",", "Comma ,"],
    [";", "Semicolon ;"],
    ["tab", "Tab"],
    ["|", "Pipe |"],
  ];

  let path = $state<string | null>(null);
  let encoding = $state("utf-8");
  let delimiter = $state("");
  let header = $state(true);
  let emptyAsNull = $state(true);
  let preview = $state<DbCsvPreview | null>(null);
  let info = $state<DbTableInfo | null>(null);
  let mapping = $state<(string | null)[]>([]);
  let busy = $state(false);
  let loading = $state(false);
  let rows = $state(0);
  let jobId = $state<Uuid | null>(null);
  let error = $state<string | null>(null);
  let confirmed = $state(false);

  const session = $derived(databases.sessions[connId]);
  const production = $derived(databases.isProduction(connId));
  const mapped = $derived(mapping.filter((m) => m !== null) as string[]);
  const doubled = $derived(mapped.filter((m, i) => mapped.indexOf(m) !== i));
  const ready = $derived(!!path && !!preview && mapped.length > 0 && doubled.length === 0 && (!production || confirmed));

  $effect(() => {
    const off = api.db.onTransfer((p) => {
      if (p.job_id === jobId) rows = p.rows;
    });
    return () => void off.then((f) => f());
  });

  $effect(() => {
    if (session) void api.db.tableInfo(session.sessionId, database, table).then((i) => (info = i)).catch(() => (info = null));
  });

  async function choose() {
    const picked = await open({ title: "Choose a CSV file", multiple: false, filters: [{ name: "CSV", extensions: ["csv", "tsv", "txt"] }] });
    if (typeof picked !== "string") return;
    path = picked;
    await read();
  }

  async function read() {
    if (!path) return;
    loading = true;
    error = null;
    try {
      const p = await api.db.importPreview(path, encoding, delimiter || null, header);
      preview = p;
      mapping = autoMapping(p.headers, info?.columns.map((c) => c.name) ?? []);
    } catch (e) {
      preview = null;
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  async function run() {
    if (!ready || !session || !path) return;
    busy = true;
    error = null;
    rows = 0;
    jobId = crypto.randomUUID();
    try {
      const s = await api.db.importCsv(session.sessionId, jobId, { database, table, encoding, delimiter: delimiter || null, has_header: header, mapping, empty_as_null: emptyAsNull }, path);
      ui.notify("info", `Imported ${s.rows.toLocaleString()} rows into ${table}.`);
      for (const t of databases.tabs.filter((t) => t.connId === connId && t.table?.table === table && t.table.database === database)) void databases.loadPage(t.id);
      ui.modal = null;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
      jobId = null;
    }
  }

  async function stop() {
    if (jobId) await api.db.stopImport(jobId).catch(() => {});
  }

  const fileName = $derived(path ? path.split(/[\\/]/).pop() : "");
</script>

<Modal title="Import into {table}" width="max-w-3xl" onclose={() => !busy && (ui.modal = null)}>
  <div class="space-y-4 text-sm">
    <p class="text-xs text-fg-muted">Reads a CSV file a chunk at a time and adds its rows to <span class="font-mono">{database}.{table}</span>. Rows are saved in batches of 200, each in its own transaction: if a batch is refused, the batches before it stay saved.</p>
    {#if !session}<p class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">This connection isn't open. Connect to it first.</p>{/if}

    <div class="flex flex-wrap items-end gap-3">
      <div class="min-w-0 flex-1">
        <span class="mb-1 block text-xs font-medium">File</span>
        <button class="btn-secondary w-full justify-start truncate text-xs" disabled={busy || !session} onclick={choose}><FileUp size={13} /> {fileName || "Choose a CSV file…"}</button>
      </div>
      <label class="text-xs"><span class="mb-1 block font-medium">Encoding</span>
        <select class="input h-8 text-xs" bind:value={encoding} onchange={read} disabled={busy}>{#each ENCODINGS as [v, l] (v)}<option value={v}>{l}</option>{/each}</select>
      </label>
      <label class="text-xs"><span class="mb-1 block font-medium">Separator</span>
        <select class="input h-8 text-xs" bind:value={delimiter} onchange={read} disabled={busy}>{#each DELIMITERS as [v, l] (v)}<option value={v}>{l}</option>{/each}</select>
      </label>
    </div>
    <div class="flex flex-wrap gap-4 text-xs">
      <label class="flex items-center gap-2"><input type="checkbox" class="accent-input" bind:checked={header} onchange={read} disabled={busy} /> The first line is a header</label>
      <label class="flex items-center gap-2"><input type="checkbox" class="accent-input" bind:checked={emptyAsNull} disabled={busy} /> Save an empty field as NULL</label>
    </div>

    {#if loading}<p class="flex items-center gap-2 text-xs text-fg-muted"><Loader2 size={13} class="animate-spin" /> Reading the start of the file…</p>{/if}

    {#if preview}
      <section aria-labelledby="map-h">
        <h3 id="map-h" class="mb-1.5 text-xs font-medium">Which column goes where <span class="font-normal text-fg-muted">({(preview.file_bytes / 1024).toFixed(0)} KB file, separator {preview.delimiter === "\t" ? "tab" : `"${preview.delimiter}"`})</span></h3>
        <div class="overflow-auto rounded-lg border border-line">
          <table class="w-full min-w-[34rem] text-left text-xs" aria-label="Column mapping and preview">
            <thead class="bg-panel">
              <tr class="border-b border-line">
                {#each preview.headers as h, i (i)}
                  <th class="px-2 py-1.5 align-bottom font-normal">
                    <span class="block truncate font-mono font-medium" title={h}>{h}</span>
                    <select class="input mt-1 h-8 w-full text-xs" aria-label="Table column for {h}" bind:value={mapping[i]} disabled={busy}>
                      <option value={null}>(skip this column)</option>
                      {#each info?.columns ?? [] as c (c.name)}<option value={c.name}>{c.name} · {c.data_type}</option>{/each}
                    </select>
                  </th>
                {/each}
              </tr>
            </thead>
            <tbody class="divide-y divide-line/60 font-mono">
              {#each preview.rows.slice(0, 8) as r, ri (ri)}
                <tr>{#each r as cell, ci (ci)}<td class="max-w-48 truncate px-2 py-1 {mapping[ci] === null ? 'text-fg-muted/50' : ''}" title={cell}>{cell}</td>{/each}</tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="mt-1 text-[11px] text-fg-muted">The first {Math.min(8, preview.rows.length)} rows. Columns of the table you don't map get their default.</p>
        {#if doubled.length}<p class="mt-1 text-xs text-danger" role="alert">{[...new Set(doubled)].join(", ")} is chosen for more than one column.</p>{/if}
      </section>
    {/if}

    {#if production}
      <label class="flex items-start gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs"><input type="checkbox" class="mt-0.5 accent-input" bind:checked={confirmed} disabled={busy} /> <span>This is a <strong>production</strong> database. I understand these rows are added to it.</span></label>
    {/if}
    {#if busy}<p class="flex items-center gap-2 text-xs" role="status"><Loader2 size={13} class="animate-spin" /> {rows.toLocaleString()} rows saved…</p>{/if}
    {#if error}<p class="whitespace-pre-wrap rounded-md border border-danger/30 bg-danger/10 px-3 py-2 font-mono text-xs text-danger" role="alert">{error}</p>{/if}
  </div>
  {#snippet footer()}
    {#if busy}
      <button class="btn-danger" onclick={stop}>Stop after this batch</button>
    {:else}
      <button class="btn-secondary" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-primary" disabled={!ready || !session} onclick={run} data-testid="run-import">Import</button>
    {/if}
  {/snippet}
</Modal>
