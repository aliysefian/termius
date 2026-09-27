<script lang="ts">
  import { onMount } from "svelte";
  import { RefreshCw, Usb } from "lucide-svelte";
  import Modal from "./Modal.svelte";
  import * as api from "$lib/api";
  import { ui } from "$lib/stores/ui.svelte";
  import { errorMessage, type SerialConfig } from "$lib/types";

  const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
  const KEY = "sshvault.serial.v1";

  function loadLast(): SerialConfig {
    try {
      const raw = localStorage.getItem(KEY);
      if (raw) return { ...defaults(), ...JSON.parse(raw) };
    } catch {
      // No storage; use defaults.
    }
    return defaults();
  }
  function defaults(): SerialConfig {
    return { path: "", baud: 9600, data_bits: 8, parity: "none", stop_bits: 1, flow: "none" };
  }

  let cfg = $state<SerialConfig>(loadLast());
  let ports = $state<string[]>([]);
  let error = $state<string | null>(null);

  async function refresh() {
    try {
      ports = await api.raw.ports();
      if (!cfg.path && ports.length) cfg.path = ports[0];
    } catch (e) {
      error = errorMessage(e);
    }
  }
  onMount(refresh);

  function open(e: SubmitEvent) {
    e.preventDefault();
    const c = $state.snapshot(cfg) as SerialConfig;
    try {
      localStorage.setItem(KEY, JSON.stringify(c));
    } catch {
      // Remembering the settings is a convenience only.
    }
    ui.modal = null;
    ui.openSerial(c);
  }
</script>

<Modal title="Serial console" onclose={() => (ui.modal = null)} width="max-w-md">
  <form id="serial-form" class="space-y-3" onsubmit={open}>
    <div>
      <div class="mb-1 flex items-end justify-between">
        <label class="label mb-0" for="sr-port">Port</label>
        <button type="button" class="btn-ghost py-0.5 text-xs" onclick={refresh}><RefreshCw size={12} /> Rescan</button>
      </div>
      <input id="sr-port" class="input font-mono" list="sr-ports" bind:value={cfg.path} required placeholder={navigator.userAgent.includes("Windows") ? "COM3" : "/dev/ttyUSB0"} spellcheck="false" />
      <datalist id="sr-ports">{#each ports as p (p)}<option value={p}></option>{/each}</datalist>
      {#if ports.length === 0}
        <p class="mt-1 text-xs text-fg-muted">No ports found. Plug in the adapter and rescan, or type the device path. On Linux your user may need to be in the <code>dialout</code> group.</p>
      {/if}
    </div>
    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label" for="sr-baud">Baud rate</label>
        <input id="sr-baud" class="input font-mono" list="sr-bauds" type="number" min="50" bind:value={cfg.baud} required />
        <datalist id="sr-bauds">{#each BAUDS as b (b)}<option value={b}></option>{/each}</datalist>
      </div>
      <div>
        <label class="label" for="sr-bits">Data bits</label>
        <select id="sr-bits" class="input" bind:value={cfg.data_bits}>{#each [8, 7, 6, 5] as b (b)}<option value={b}>{b}</option>{/each}</select>
      </div>
      <div>
        <label class="label" for="sr-par">Parity</label>
        <select id="sr-par" class="input" bind:value={cfg.parity}>
          <option value="none">None</option><option value="even">Even</option><option value="odd">Odd</option>
        </select>
      </div>
      <div>
        <label class="label" for="sr-stop">Stop bits</label>
        <select id="sr-stop" class="input" bind:value={cfg.stop_bits}><option value={1}>1</option><option value={2}>2</option></select>
      </div>
      <div class="col-span-2">
        <label class="label" for="sr-flow">Flow control</label>
        <select id="sr-flow" class="input" bind:value={cfg.flow}>
          <option value="none">None</option><option value="software">XON/XOFF</option><option value="hardware">RTS/CTS</option>
        </select>
      </div>
    </div>
    <p class="flex items-center gap-1.5 text-xs text-fg-muted"><Usb size={12} /> Most network gear uses 9600 8N1; newer devices often 115200.</p>
    {#if error}<p class="text-sm text-danger">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <button class="btn-ghost" onclick={() => (ui.modal = null)}>Cancel</button>
    <button class="btn-primary" type="submit" form="serial-form" disabled={!cfg.path.trim()}>Open</button>
  {/snippet}
</Modal>
