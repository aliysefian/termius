<script lang="ts">
  import { ArrowRight, Loader2, TriangleAlert } from "lucide-svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { ask } from "$lib/dialogs.svelte";
  import { PROVIDER_NAMES, applyChange, parseAws, parseAzure, parseDigitalOcean, parseGcp, parseHetzner, parseKubernetes, parseScan, parseTailscale, parseTerraform, plan, toHost, type Found, type Plan, type Provider } from "$lib/inventory";
  import { ui } from "$lib/stores/ui.svelte";
  import { vaultStore } from "$lib/stores/vault.svelte";
  import { errorMessage } from "$lib/types";

  type Key = "tailscale" | "aws" | "gcp" | "azure" | "digitalocean" | "hetzner" | "kubernetes" | "terraform" | "terraform-file" | "scan";
  interface Source {
    key: Key;
    label: string;
    provider: Provider;
    program: InventoryProgram | null;
    /** What the optional value is, and an example. */
    option?: { label: string; placeholder: string };
    /** The exact program line, shown before anything is run. */
    line: (option: string) => string;
    note?: string;
  }
  type InventoryProgram = api.InventoryProgram;

  const SOURCES: Source[] = [
    { key: "tailscale", label: "Tailscale", provider: "tailscale", program: "tailscale", line: () => "tailscale status --json", note: "Your tailnet's machines, by MagicDNS name." },
    { key: "aws", label: "AWS EC2", provider: "aws", program: "aws", option: { label: "Region", placeholder: "default from your AWS settings" }, line: (o) => `aws ec2 describe-instances --output json${o ? ` --region ${o}` : ""}`, note: "Uses the AWS CLI you are signed in with. For Session Manager instead of SSH, make the host a Command host with `aws ssm start-session --target {id}`." },
    { key: "gcp", label: "Google Cloud", provider: "gcp", program: "gcp", option: { label: "Project", placeholder: "default from gcloud" }, line: (o) => `gcloud compute instances list --format=json${o ? ` --project=${o}` : ""}` },
    { key: "azure", label: "Azure", provider: "azure", program: "azure", option: { label: "Subscription", placeholder: "default from az" }, line: (o) => `az vm list -d -o json${o ? ` --subscription ${o}` : ""}` },
    { key: "digitalocean", label: "DigitalOcean", provider: "digitalocean", program: "digital_ocean", option: { label: "doctl context", placeholder: "default" }, line: (o) => `doctl compute droplet list -o json${o ? ` --context ${o}` : ""}` },
    { key: "hetzner", label: "Hetzner", provider: "hetzner", program: "hetzner", option: { label: "hcloud context", placeholder: "default" }, line: (o) => `hcloud server list -o json${o ? ` --context ${o}` : ""}` },
    { key: "kubernetes", label: "Kubernetes nodes", provider: "kubernetes", program: "kubernetes", option: { label: "kubectl context", placeholder: "current context" }, line: (o) => `kubectl get nodes -o json${o ? ` --context ${o}` : ""}` },
    { key: "terraform", label: "Terraform (a folder)", provider: "terraform", program: "terraform", option: { label: "Folder", placeholder: "choose…" }, line: () => "terraform show -json   (run in that folder)", note: "Reads the state of the configuration there. Reads only; nothing is applied." },
    { key: "terraform-file", label: "Terraform (a state file)", provider: "terraform", program: null, line: () => "reads a terraform.tfstate file", note: "Nothing is run: the file is read." },
    { key: "scan", label: "Scan a network for SSH servers", provider: "scan", program: null, line: (o) => `connect to port ${port} of every address in ${o || "the range"} and read the SSH banner`, note: "Only private networks (10.x, 172.16–31.x, 192.168.x, 100.64–127.x). It makes one short connection to each address." },
  ];

  // svelte-ignore state_referenced_locally
  const start = (ui.modal?.kind === "import-inventory" && ui.modal.source) || "tailscale";
  let key = $state<Key>((SOURCES.find((s) => s.key === start)?.key ?? "tailscale") as Key);
  let option = $state("");
  let port = $state(22);
  let preferPrivate = $state(false);
  let identityId = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let found = $state<Found[] | null>(null);
  let thePlan = $state<Plan | null>(null);
  let addPicked = $state<Set<string>>(new Set());
  let updatePicked = $state<Set<string>>(new Set());
  let removePicked = $state<Set<string>>(new Set());
  let done = $state<string | null>(null);

  const source = $derived(SOURCES.find((s) => s.key === key)!);
  const scope = $derived(source.program === null || key === "terraform" ? "" : option.trim());
  const identities = $derived(vaultStore.identities.filter((i) => !i.deleted && i.data));

  function reset() {
    found = null;
    thePlan = null;
    error = null;
    done = null;
  }

  async function chooseFolder() {
    const f = await open({ directory: true, multiple: false, title: "Folder with the Terraform configuration" });
    if (typeof f === "string") option = f;
  }

  async function run() {
    reset();
    busy = true;
    try {
      const o = { preferPrivate, scope: option.trim() };
      let list: Found[];
      if (key === "scan") {
        list = parseScan(await api.inventoryScan(option.trim(), port));
      } else if (key === "terraform-file") {
        const f = await open({ multiple: false, directory: false, title: "Choose a terraform.tfstate file", filters: [{ name: "Terraform state", extensions: ["tfstate", "json"] }] });
        if (typeof f !== "string") return;
        list = parseTerraform(await api.readTextFile(f), o);
      } else {
        if (key === "terraform" && !option.trim()) throw new Error("Choose the folder with the Terraform configuration first.");
        const text = await api.inventoryRun(source.program!, option.trim() || null);
        const parsers: Record<string, (t: string, o: { preferPrivate?: boolean; scope?: string }) => Found[]> = { tailscale: parseTailscale, aws: parseAws, gcp: parseGcp, azure: parseAzure, digitalocean: parseDigitalOcean, hetzner: parseHetzner, kubernetes: parseKubernetes, terraform: parseTerraform };
        list = parsers[key](text, o);
      }
      found = list;
      const existing = vaultStore.hosts.filter((r) => !r.deleted && r.data).map((r) => ({ id: r.id, data: $state.snapshot(r.data!) }));
      const p = plan(existing, list, source.provider, scope);
      // A scan only finds what answers now: a machine that doesn't is not "gone".
      if (key === "scan") p.gone = [];
      thePlan = p;
      addPicked = new Set(p.add.map((f) => f.id));
      updatePicked = new Set(p.update.map((c) => c.existing.id));
      removePicked = new Set();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  const toggle = (set: Set<string>, id: string) => {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  };

  async function apply() {
    if (!thePlan) return;
    const removing = thePlan.gone.filter((g) => removePicked.has(g.id));
    if (removing.length && !(await ask(`Remove ${removing.length} host${removing.length === 1 ? "" : "s"} from your list? They are no longer listed by ${source.label}.`, { title: "Remove hosts", confirm: "Remove", danger: true }))) return;
    busy = true;
    error = null;
    let added = 0;
    let changed = 0;
    let removed = 0;
    try {
      for (const f of thePlan.add.filter((x) => addPicked.has(x.id))) {
        await vaultStore.saveHost(null, toHost(f, scope, identityId || undefined));
        added++;
      }
      for (const c of thePlan.update.filter((x) => updatePicked.has(x.existing.id))) {
        await vaultStore.saveHost(c.existing.id, applyChange(c));
        changed++;
      }
      for (const g of removing) {
        await vaultStore.deleteHost(g.id);
        removed++;
      }
      done = [added && `${added} added`, changed && `${changed} updated`, removed && `${removed} removed`].filter(Boolean).join(", ") || "Nothing to do.";
      thePlan = null;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  const label = (id: string) => vaultStore.hostById.get(id)?.data?.label ?? id;
</script>

<Modal title="Import hosts from a cloud or tool" onclose={() => (ui.modal = null)} width="max-w-2xl">
  <div class="space-y-4 text-sm" data-testid="import-inventory">
    <div class="grid grid-cols-2 gap-3">
      <div class="col-span-2">
        <label class="label" for="inv-source">Source</label>
        <select id="inv-source" class="input" bind:value={key} onchange={() => { option = ""; reset(); }}>
          {#each SOURCES as s (s.key)}<option value={s.key}>{s.label}</option>{/each}
        </select>
      </div>
      {#if source.option}
        <div class={key === "scan" ? "" : "col-span-2"}>
          <label class="label" for="inv-option">{source.option.label} <span class="font-normal text-fg-muted">(optional)</span></label>
          <div class="flex gap-2">
            <input id="inv-option" class="input min-w-0 flex-1 font-mono text-xs" bind:value={option} placeholder={source.option.placeholder} spellcheck="false" autocomplete="off" readonly={key === "terraform"} />
            {#if key === "terraform"}<button type="button" class="btn-secondary" onclick={chooseFolder}>Choose…</button>{/if}
          </div>
        </div>
      {/if}
      {#if key === "scan"}
        <div>
          <label class="label" for="inv-range">Network</label>
          <input id="inv-range" class="input font-mono text-xs" bind:value={option} placeholder="192.168.1.0/24" spellcheck="false" autocomplete="off" />
        </div>
        <div>
          <label class="label" for="inv-port">SSH port</label>
          <input id="inv-port" class="input font-mono text-xs" type="number" min="1" max="65535" bind:value={port} />
        </div>
      {/if}
    </div>
    <div class="rounded-md border border-line bg-base/40 p-3 text-xs text-fg-muted">
      <div class="mb-1 font-medium text-fg">What will run</div>
      <code class="break-all font-mono text-fg" data-testid="inventory-line">{source.line(option.trim())}</code>
      {#if source.note}<p class="mt-1">{source.note}</p>{/if}
      {#if source.program}<p class="mt-1">The program is run on this computer with its own sign-in; this app never sees the credentials. What it prints is read as data, never run.</p>{/if}
    </div>
    {#if source.provider !== "scan" && source.provider !== "tailscale"}
      <label class="flex items-center gap-2 text-xs text-fg-muted"><input type="checkbox" class="accent-input" bind:checked={preferPrivate} /> Use private addresses when there are both (for a VPN or bastion)</label>
    {/if}

    {#if error}<p class="flex gap-2 rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-danger whitespace-pre-wrap" role="alert"><TriangleAlert size={15} class="mt-0.5 shrink-0" /> {error}</p>{/if}
    {#if done}<p class="rounded-md border border-success/30 bg-success/10 px-3 py-2 text-success" role="status" data-testid="inventory-done">{done}</p>{/if}

    {#if thePlan}
      <div class="space-y-3" data-testid="inventory-plan">
        <div>
          <label class="label" for="inv-identity">New hosts sign in with</label>
          <select id="inv-identity" class="input" bind:value={identityId}>
            <option value="">Ask when connecting</option>
            {#each identities as i (i.id)}<option value={i.id}>{i.data!.label} ({i.data!.username})</option>{/each}
          </select>
        </div>
        {#if found && found.length === 0}<p class="text-fg-muted">{PROVIDER_NAMES[source.provider]} listed no machines with an address.</p>{/if}
        {#if thePlan.add.length}
          <div>
            <div class="mb-1 text-xs font-medium text-fg-muted">To add ({addPicked.size} of {thePlan.add.length})</div>
            <ul class="max-h-44 space-y-0.5 overflow-auto rounded-md border border-line p-1">
              {#each thePlan.add as f (f.id)}
                <li><label class="flex items-center gap-2 rounded px-2 py-0.5 hover:bg-hover"><input type="checkbox" class="accent-input" checked={addPicked.has(f.id)} onchange={() => (addPicked = toggle(addPicked, f.id))} /> <span class="min-w-0 flex-1 truncate">{f.label}</span> <span class="truncate font-mono text-xs text-fg-muted">{f.hostname}</span> <span class="text-xs text-fg-muted">{f.group}</span></label></li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if thePlan.update.length}
          <div>
            <div class="mb-1 text-xs font-medium text-fg-muted">Changed since the last import ({updatePicked.size} of {thePlan.update.length}). Only what you haven't edited is updated.</div>
            <ul class="space-y-0.5 rounded-md border border-line p-1">
              {#each thePlan.update as c (c.existing.id)}
                <li><label class="flex items-center gap-2 rounded px-2 py-0.5 hover:bg-hover"><input type="checkbox" class="accent-input" checked={updatePicked.has(c.existing.id)} onchange={() => (updatePicked = toggle(updatePicked, c.existing.id))} /> <span class="min-w-0 flex-1 truncate">{c.existing.data.label}</span>
                  <span class="flex items-center gap-1 truncate font-mono text-xs text-fg-muted">{#each c.fields as f, i (f)}{#if i}, {/if}{f === "hostname" ? c.existing.data.hostname : c.existing.data.label} <ArrowRight size={11} /> {c.found[f]}{/each}</span></label></li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if thePlan.gone.length}
          <div>
            <div class="mb-1 text-xs font-medium text-fg-muted">No longer listed ({thePlan.gone.length}). Kept unless you tick them.</div>
            <ul class="space-y-0.5 rounded-md border border-line p-1">
              {#each thePlan.gone as g (g.id)}
                <li><label class="flex items-center gap-2 rounded px-2 py-0.5 hover:bg-hover"><input type="checkbox" class="accent-input" checked={removePicked.has(g.id)} onchange={() => (removePicked = toggle(removePicked, g.id))} /> <span class="min-w-0 flex-1 truncate">{label(g.id)}</span> <span class="text-xs text-fg-muted">remove from my list</span></label></li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if thePlan.duplicate.length}<p class="text-xs text-fg-muted">{thePlan.duplicate.length} already in your list with the same name and address; not added again.</p>{/if}
        {#if thePlan.unchanged}<p class="text-xs text-fg-muted">{thePlan.unchanged} unchanged.</p>{/if}
      </div>
    {/if}
  </div>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Close</button>
    <button class="btn-secondary" disabled={busy} onclick={run} data-testid="inventory-run">
      {#if busy && !thePlan}<Loader2 size={14} class="animate-spin" />{/if} {thePlan || done ? "Run again" : key === "scan" ? "Scan" : "List machines"}
    </button>
    {#if thePlan && (addPicked.size || updatePicked.size || removePicked.size)}
      <button class="btn-primary" disabled={busy} onclick={apply} data-testid="inventory-apply">Apply</button>
    {/if}
  {/snippet}
</Modal>
