<script lang="ts">
  import { Check, Shield, ShieldAlert } from "lucide-svelte";
  import { keepInView } from "$lib/actions";
  import { formatCountdown } from "$lib/nethops";
  import { networkStore } from "$lib/stores/network.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  // Agent forwarding for one session: off, on, or on for five minutes. Shown only for a session that asked for the
  // agent to be forwarded (the server makes its agent socket only then); the rest have nothing to switch.
  let { paneId }: { paneId: string } = $props();

  const gate = $derived(networkStore.agent[paneId]);
  const connected = $derived(ui.paneInfo[paneId]?.status === "connected");

  let now = $state(Date.now());
  const left = $derived(networkStore.agentSecondsLeft(paneId, now));

  // Learn the state when the session connects; tick while a timed grant runs, and re-read when it runs out.
  $effect(() => {
    if (connected) void networkStore.loadAgent(paneId);
  });
  $effect(() => {
    if (gate?.state.state !== "timed") return;
    const t = setInterval(() => {
      now = Date.now();
      if ((networkStore.agentSecondsLeft(paneId, Date.now()) ?? 0) <= 0) {
        clearInterval(t);
        void networkStore.loadAgent(paneId);
      }
    }, 1000);
    return () => clearInterval(t);
  });

  let menu = $state<{ x: number; y: number } | null>(null);

  const mode = $derived(gate?.state.state ?? "off");
  const label = $derived(mode === "off" ? "OFF" : mode === "on" ? "ON" : `ON ${formatCountdown(left ?? 0)}`);

  async function choose(mode: "off" | "on" | "timed") {
    menu = null;
    await networkStore.setAgent(paneId, mode, mode === "timed" ? 300 : undefined);
  }
</script>

<svelte:window onkeydown={(e) => { if (menu && e.key === "Escape") menu = null; }} />

{#if gate && connected}
  <button
    type="button"
    class="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 font-mono text-[11px] tabular-nums transition-colors hover:bg-panel-hover
      {mode === 'off' ? 'text-fg-muted' : 'text-warning'}"
    title={mode === "off"
      ? "Agent forwarding is off: this host cannot use your SSH keys"
      : "Agent forwarding is on: this host can ask your SSH agent to sign. Click to change."}
    aria-haspopup="menu"
    aria-expanded={menu !== null}
    onclick={(e) => {
      const r = e.currentTarget.getBoundingClientRect();
      menu = menu ? null : { x: r.left, y: r.bottom + 4 };
    }}
    data-testid="agent-toggle"
    data-state={mode}
  >
    {#if mode === "off"}<Shield size={12} />{:else}<ShieldAlert size={12} />{/if}
    Agent: {label}
  </button>
{/if}

{#if menu}
  <button type="button" class="fixed inset-0 z-40 cursor-default" aria-label="Close menu" onclick={() => (menu = null)}></button>
  <div class="fixed z-50 w-60 rounded-md border border-line bg-panel py-1 text-xs shadow-lg" role="menu" use:keepInView={menu} data-testid="agent-menu">
    {#each [{ mode: "off", text: "Off", hint: "Hosts can't use your keys" }, { mode: "timed", text: "On for 5 minutes", hint: "Switches itself off" }, { mode: "on", text: "On until I switch it off", hint: "For this session" }] as o (o.mode)}
      <button class="flex w-full items-start gap-2 px-3 py-1.5 text-left hover:bg-panel-hover" role="menuitemradio" aria-checked={o.mode === mode} onclick={() => choose(o.mode as "off" | "on" | "timed")}>
        <span class="mt-0.5 w-3 shrink-0">{#if o.mode === mode}<Check size={12} />{/if}</span>
        <span class="min-w-0"><span class="block text-sm">{o.text}</span><span class="block text-fg-muted">{o.hint}</span></span>
      </button>
    {/each}
  </div>
{/if}
