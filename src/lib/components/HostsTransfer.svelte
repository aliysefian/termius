<script lang="ts">
  import { FileDown, FileUp, KeyRound, Loader2, ShieldCheck, TriangleAlert } from "lucide-svelte";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import Combobox from "./Combobox.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";
  import * as api from "$lib/api";
  import { groupOptions } from "$lib/pickeroptions";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { describeResult, initialChoices, nameProblems, setAll, toResolutions, type Choice } from "$lib/hostsimport";
  import { errorMessage, type HostsExportSummary, type HostsImportPreview, type HostsImportResult } from "$lib/types";

  let { initialTab = "export" }: { initialTab?: "export" | "import" } = $props();

  const MIN_LENGTH = 8;
  const EXTENSION = "svhosts";

  // svelte-ignore state_referenced_locally: only where the dialog opens; the tabs switch it from then on
  let tab = $state<"export" | "import">(initialTab);
  let busy = $state(false);
  let error = $state<string | null>(null);

  function switchTab(t: "export" | "import") {
    if (busy) return;
    tab = t;
    error = null;
  }

  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

  // -- export ----------------------------------------------------------------------------------------------

  let scope = $state("");
  let includeSecrets = $state(true);
  let pass = $state("");
  let pass2 = $state("");
  let exported = $state<{ summary: HostsExportSummary; path: string } | null>(null);

  const scopeChoices = $derived([{ value: "", label: "All hosts" }, ...groupOptions(vaultStore.hosts, vaultStore.groups)]);
  const exportProblem = $derived(pass.length < MIN_LENGTH ? `Use at least ${MIN_LENGTH} characters.` : pass !== pass2 ? "The two passphrases are different." : null);

  async function doExport() {
    if (exportProblem || busy) return;
    const chosen = await save({
      title: "Save the encrypted hosts file",
      defaultPath: `sshvault-hosts.${EXTENSION}`,
      filters: [{ name: "SSHVault hosts", extensions: [EXTENSION] }],
    });
    if (!chosen) return;
    const path = chosen.toLowerCase().endsWith(`.${EXTENSION}`) ? chosen : `${chosen}.${EXTENSION}`;
    busy = true;
    error = null;
    try {
      const summary = await api.hostsTransfer.export(path, pass, includeSecrets, scope || null);
      exported = { summary, path };
      pass = "";
      pass2 = "";
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  // -- import ----------------------------------------------------------------------------------------------

  let file = $state<string | null>(null);
  let importPass = $state("");
  let preview = $state<HostsImportPreview | null>(null);
  let choices = $state<Record<string, Choice>>({});
  let result = $state<HostsImportResult | null>(null);

  const problems = $derived(preview ? nameProblems(preview.conflicts, choices) : {});
  const blocked = $derived(Object.keys(problems).length > 0);
  const skipping = $derived(preview ? preview.conflicts.filter((c) => choices[c.id]?.action === "skip").length : 0);

  async function chooseFile() {
    const f = await open({
      multiple: false,
      directory: false,
      title: "Choose an SSHVault hosts file",
      filters: [{ name: "SSHVault hosts", extensions: [EXTENSION] }],
    });
    if (typeof f !== "string") return;
    file = f;
    preview = null;
    error = null;
  }

  async function readFile() {
    if (!file || !importPass || busy) return;
    busy = true;
    error = null;
    try {
      preview = await api.hostsTransfer.importPreview(file, importPass);
      choices = initialChoices(preview.conflicts);
    } catch (e) {
      error = errorMessage(e);
      preview = null;
    } finally {
      busy = false;
    }
  }

  async function doImport() {
    if (!file || !preview || blocked || busy) return;
    busy = true;
    error = null;
    try {
      result = await api.hostsTransfer.importApply(file, importPass, toResolutions(preview.conflicts, choices));
      importPass = "";
      await vaultStore.reloadAll();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
  const groupLabel = (g: string) => (g ? g : "top level");
</script>

<Modal title="Export or import hosts" onclose={() => (ui.modal = null)} width="max-w-2xl">
  <div class="mb-4 flex gap-1 rounded-md border border-line p-0.5 text-sm" role="tablist" aria-label="Export or import">
    <button class="flex flex-1 items-center justify-center gap-2 rounded px-3 py-1.5 {tab === 'export' ? 'bg-accent/20 text-fg' : 'text-fg-muted hover:text-fg'}" role="tab" aria-selected={tab === "export"} onclick={() => switchTab("export")}>
      <FileDown size={14} /> Export
    </button>
    <button class="flex flex-1 items-center justify-center gap-2 rounded px-3 py-1.5 {tab === 'import' ? 'bg-accent/20 text-fg' : 'text-fg-muted hover:text-fg'}" role="tab" aria-selected={tab === "import"} onclick={() => switchTab("import")}>
      <FileUp size={14} /> Import
    </button>
  </div>

  {#if error}
    <div class="mb-3 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger" role="alert">{error}</div>
  {/if}

  {#if tab === "export"}
    {#if exported}
      <div class="space-y-3 text-sm">
        <p class="flex items-center gap-2 font-medium text-success"><ShieldCheck size={16} /> Saved an encrypted file</p>
        <p>
          {plural(exported.summary.hosts, "host")} in {plural(exported.summary.groups, "group")}{exported.summary.identities ? `, ${plural(exported.summary.identities, "login")}` : ""}{exported.summary.keys ? `, ${plural(exported.summary.keys, "key")}` : ""}{exported.summary.proxies ? `, ${plural(exported.summary.proxies, "proxy", "proxies")}` : ""}.
        </p>
        <p class="break-all rounded-md border border-line bg-base px-3 py-2 font-mono text-xs">{exported.path}</p>
        <p class="text-xs text-fg-muted">
          To open it on another computer, import it there and enter the passphrase you just chose. The passphrase is not stored anywhere: if it is lost, the file cannot be opened.
        </p>
        <button class="btn-ghost" onclick={() => (exported = null)}>Export again</button>
      </div>
    {:else}
      <div class="space-y-4">
        <p class="text-sm text-fg-muted">
          Saves your hosts, with their groups and folders, to one file protected by a passphrase. Import it in another SSHVault to get the same list there.
        </p>
        <div>
          <label class="label" for="hx-scope">What to export</label>
          <Combobox id="hx-scope" options={scopeChoices} bind:value={scope} ariaLabel="What to export" placeholder="All hosts" />
          {#if scope}
            <p class="mt-1 text-xs text-fg-muted">This group and the groups under it. Jump hosts they go through come along so the connections still work.</p>
          {/if}
        </div>
        <label class="flex items-start gap-2 text-sm">
          <input type="checkbox" class="mt-1 shrink-0" bind:checked={includeSecrets} />
          <span>
            <span class="flex items-center gap-1.5"><KeyRound size={13} /> Include saved user names, passwords and private keys</span>
            <span class="block text-xs text-fg-muted">
              {includeSecrets
                ? "They are inside the encrypted file. Anyone who has the file and the passphrase can read them."
                : "Only user names and the structure are saved. The hosts ask for the password when you connect."}
            </span>
          </span>
        </label>
        <div class="grid gap-3 sm:grid-cols-2">
          <div>
            <label class="label" for="hx-pass">Passphrase for the file</label>
            <input id="hx-pass" class="input" type="password" autocomplete="new-password" bind:value={pass} />
            <StrengthMeter password={pass} />
          </div>
          <div>
            <label class="label" for="hx-pass2">Type it again</label>
            <input id="hx-pass2" class="input" type="password" autocomplete="new-password" bind:value={pass2} onkeydown={(e) => e.key === "Enter" && doExport()} />
            {#if pass2 && exportProblem}<p class="mt-1 text-xs text-danger">{exportProblem}</p>{/if}
          </div>
        </div>
        {#if includeSecrets}
          <p class="flex items-start gap-2 text-xs text-fg-muted"><TriangleAlert size={13} class="mt-0.5 shrink-0 text-warning" /> This is a different passphrase from your vault's master password. Choose one you will remember, and share it separately from the file.</p>
        {/if}
      </div>
    {/if}
  {:else if result}
    <div class="space-y-3 text-sm">
      <p class="flex items-center gap-2 font-medium text-success"><ShieldCheck size={16} /> Import finished</p>
      {#each describeResult(result) as line (line)}<p>{line}</p>{/each}
      {#if result.warnings.length}
        <ul class="list-disc space-y-1 rounded-md border border-warning/30 bg-warning/10 py-2 pl-7 pr-3 text-xs">
          {#each result.warnings as w (w)}<li>{w}</li>{/each}
        </ul>
      {/if}
    </div>
  {:else if preview}
    <div class="space-y-4 text-sm">
      <p>
        <span class="font-medium">{baseName(file ?? "")}</span> has {plural(preview.hosts, "host")} in {plural(preview.groups, "group")}{preview.identities ? `, ${plural(preview.identities, "login")}` : ""}{preview.keys ? `, ${plural(preview.keys, "key")}` : ""}{preview.proxies ? `, ${plural(preview.proxies, "proxy", "proxies")}` : ""}{preview.exported_at ? `, saved ${new Date(preview.exported_at * 1000).toLocaleString()}` : ""}.
        Groups and folders keep their structure. Nothing you already have is changed or removed.
      </p>
      {#each preview.notes as note (note)}
        <p class="flex items-start gap-2 text-xs text-fg-muted"><TriangleAlert size={13} class="mt-0.5 shrink-0 text-warning" /> {note}</p>
      {/each}

      {#if preview.conflicts.length}
        <div class="rounded-md border border-warning/40 bg-warning/5">
          <div class="flex flex-wrap items-center justify-between gap-2 border-b border-warning/30 px-3 py-2">
            <p class="font-medium">{plural(preview.conflicts.length, "host")} already {preview.conflicts.length === 1 ? "has" : "have"} the same name in the same group. Rename {preview.conflicts.length === 1 ? "it" : "them"}?</p>
            {#if preview.conflicts.length > 1}
              <span class="flex gap-3 text-xs">
                <button class="text-accent hover:underline" onclick={() => (choices = setAll(preview!.conflicts, "rename"))}>Rename all</button>
                <button class="text-accent hover:underline" onclick={() => (choices = setAll(preview!.conflicts, "skip"))}>Skip all</button>
              </span>
            {/if}
          </div>
          <ul class="max-h-72 divide-y divide-line overflow-y-auto">
            {#each preview.conflicts as c (c.id)}
              {@const ch = choices[c.id]}
              <li class="space-y-2 px-3 py-2">
                <p class="text-xs"><span class="font-mono text-sm">{c.label}</span> <span class="text-fg-muted">in {groupLabel(c.group)}</span></p>
                <div class="flex flex-wrap items-center gap-3 text-xs">
                  <label class="flex items-center gap-1.5">
                    <input type="radio" name="c-{c.id}" checked={ch?.action === "rename"} onchange={() => (choices[c.id] = { action: "rename", label: c.suggested })} /> Keep both, name the new one
                  </label>
                  <input
                    class="input w-56 py-1 font-mono text-xs"
                    aria-label="New name for {c.label}"
                    disabled={ch?.action !== "rename"}
                    value={ch?.action === "rename" ? ch.label : c.suggested}
                    oninput={(e) => (choices[c.id] = { action: "rename", label: e.currentTarget.value })}
                  />
                  <label class="flex items-center gap-1.5">
                    <input type="radio" name="c-{c.id}" checked={ch?.action === "skip"} onchange={() => (choices[c.id] = { action: "skip" })} /> Skip it
                  </label>
                </div>
                {#if problems[c.id]}<p class="text-xs text-danger">{problems[c.id]}</p>{/if}
              </li>
            {/each}
          </ul>
        </div>
        {#if skipping}
          <p class="text-xs text-fg-muted">Skipped hosts are not imported. Other imported hosts that connect through one use the host you already have.</p>
        {/if}
      {:else}
        <p class="text-xs text-fg-muted">No host here has the same name as one in the file.</p>
      {/if}
    </div>
  {:else}
    <div class="space-y-4">
      <p class="text-sm text-fg-muted">Choose a file saved with Export, enter its passphrase, and the hosts are added to this vault with their groups, logins and keys.</p>
      <div>
        <p class="label">File</p>
        <div class="flex items-center gap-2">
          <button class="btn-secondary" onclick={chooseFile}><FileUp size={14} /> Choose file…</button>
          <span class="min-w-0 truncate font-mono text-xs text-fg-muted" title={file ?? ""}>{file ? baseName(file) : "No file chosen"}</span>
        </div>
      </div>
      <div>
        <label class="label" for="hx-import-pass">Passphrase of the file</label>
        <input id="hx-import-pass" class="input" type="password" autocomplete="off" bind:value={importPass} onkeydown={(e) => e.key === "Enter" && readFile()} />
      </div>
    </div>
  {/if}

  {#snippet footer()}
    {#if tab === "export" && !exported}
      <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
      <button class="btn-primary" disabled={!!exportProblem || busy} onclick={doExport}>
        {#if busy}<Loader2 size={14} class="animate-spin" /> Encrypting…{:else}Export…{/if}
      </button>
    {:else if tab === "import" && !result}
      {#if preview}
        <button class="btn-ghost" disabled={busy} onclick={() => { preview = null; error = null; }}>Back</button>
        <button class="btn-primary" disabled={blocked || busy} onclick={doImport}>
          {#if busy}<Loader2 size={14} class="animate-spin" /> Importing…{:else}Import {plural(preview.hosts - skipping, "host")}{/if}
        </button>
      {:else}
        <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
        <button class="btn-primary" disabled={!file || !importPass || busy} onclick={readFile}>
          {#if busy}<Loader2 size={14} class="animate-spin" /> Opening…{:else}Open file{/if}
        </button>
      {/if}
    {:else}
      <button class="btn-primary" onclick={() => (ui.modal = null)}>Done</button>
    {/if}
  {/snippet}
</Modal>
