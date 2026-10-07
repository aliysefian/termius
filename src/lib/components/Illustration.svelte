<script lang="ts">
  // Small friendly scenes for the places that have nothing in them yet. Drawn in the app's own colours
  // (so they follow the theme), a little alive (LEDs blink, a link pulses), and all of it stops under
  // reduced motion. Decorative: hidden from screen readers.
  export type Scene = "hosts" | "terminal" | "snippets" | "tunnels" | "files" | "databases" | "containers" | "security" | "keys";
  let { scene, size = 132 }: { scene: Scene; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 160 160" fill="none" aria-hidden="true" class="mx-auto block shrink-0 overflow-visible">
  <!-- the soft disc every scene sits on -->
  <circle cx="80" cy="84" r="62" fill="var(--color-accent)" fill-opacity="0.09" />
  <circle cx="80" cy="84" r="46" fill="var(--color-accent)" fill-opacity="0.07" />
  <circle cx="132" cy="38" r="5" fill="var(--color-accent)" fill-opacity="0.35" class="il-sparkle" style="--i:1" />
  <circle cx="26" cy="116" r="4" fill="var(--color-accent)" fill-opacity="0.3" class="il-sparkle" style="--i:3" />

  {#if scene === "hosts"}
    <g class="anim-float">
      {#each [0, 1, 2] as i (i)}
        <rect x="44" y={50 + i * 26} width="72" height="20" rx="6" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
        <circle cx="56" cy={60 + i * 26} r="3.2" fill="var(--color-success)" class="il-blink" style="--i:{i}" />
        <circle cx="67" cy={60 + i * 26} r="3.2" fill="var(--color-accent)" class="il-blink" style="--i:{i + 1}" />
        <rect x="84" y={58 + i * 26} width="22" height="4" rx="2" fill="var(--color-line)" />
      {/each}
    </g>
  {:else if scene === "terminal"}
    <g class="anim-float">
      <rect x="34" y="48" width="92" height="68" rx="10" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <path d="M34 62h92" stroke="var(--color-line)" stroke-width="2" />
      <circle cx="46" cy="55" r="2.6" fill="var(--color-danger)" fill-opacity="0.8" />
      <circle cx="55" cy="55" r="2.6" fill="var(--color-warning)" fill-opacity="0.8" />
      <circle cx="64" cy="55" r="2.6" fill="var(--color-success)" fill-opacity="0.8" />
      <path d="m48 78 11 8-11 8" stroke="var(--color-accent)" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" />
      <rect x="68" y="92" width="18" height="5" rx="2.5" fill="var(--color-accent)" class="il-blink" />
    </g>
  {:else if scene === "snippets"}
    <g class="anim-float">
      <rect x="40" y="46" width="80" height="72" rx="10" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <path d="m66 68-12 14 12 14M94 68l12 14-12 14" stroke="var(--color-accent)" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" />
      <path d="m84 64-8 36" stroke="var(--color-fg-muted)" stroke-width="4" stroke-linecap="round" />
    </g>
    <path d="M128 70v10M123 75h10" stroke="var(--color-warning)" stroke-width="3" stroke-linecap="round" class="il-sparkle" style="--i:0" />
  {:else if scene === "tunnels"}
    <g class="anim-float">
      <rect x="22" y="64" width="38" height="38" rx="10" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <rect x="100" y="64" width="38" height="38" rx="10" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <path d="M60 83h40" stroke="var(--color-accent)" stroke-width="4" stroke-linecap="round" class="il-dash" />
      <circle cx="41" cy="83" r="6" fill="var(--color-accent)" />
      <circle cx="119" cy="83" r="6" fill="var(--color-success)" />
    </g>
  {:else if scene === "files"}
    <g class="anim-float">
      <path d="M38 66a8 8 0 0 1 8-8h22l8 10h38a8 8 0 0 1 8 8v36a8 8 0 0 1-8 8H46a8 8 0 0 1-8-8z" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" stroke-linejoin="round" />
      <path d="M38 78h84" stroke="var(--color-line)" stroke-width="2" />
      <path d="M80 112V92m0 0-8 8m8-8 8 8" stroke="var(--color-accent)" stroke-width="4.5" stroke-linecap="round" stroke-linejoin="round" />
    </g>
    <path d="M124 52v8M120 56h8" stroke="var(--color-warning)" stroke-width="3" stroke-linecap="round" class="il-sparkle" style="--i:2" />
  {:else if scene === "databases"}
    <g class="anim-float">
      <ellipse cx="80" cy="60" rx="30" ry="11" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <path d="M50 60v22c0 6 13 11 30 11s30-5 30-11V60M50 82v22c0 6 13 11 30 11s30-5 30-11V82" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <path d="M50 71c0 6 13 11 30 11s30-5 30-11M50 93c0 6 13 11 30 11s30-5 30-11" stroke="var(--color-accent)" stroke-width="2.5" stroke-opacity="0.7" />
      <circle cx="98" cy="86" r="3" fill="var(--color-success)" class="il-blink" />
    </g>
  {:else if scene === "containers"}
    <g class="anim-float">
      <path d="m80 46 32 16v34L80 112 48 96V62z" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" stroke-linejoin="round" />
      <path d="m48 62 32 16 32-16M80 78v34" stroke="var(--color-accent)" stroke-width="3" stroke-linejoin="round" stroke-opacity="0.8" />
    </g>
    <circle cx="80" cy="80" r="30" stroke="var(--color-accent)" stroke-width="2" class="il-ring" style="--i:0" />
  {:else if scene === "security"}
    <g class="anim-float">
      <path d="M80 44 116 58v26c0 20-14 34-36 42-22-8-36-22-36-42V58z" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" stroke-linejoin="round" />
      <path d="m64 84 12 12 22-24" stroke="var(--color-success)" stroke-width="6" stroke-linecap="round" stroke-linejoin="round" />
    </g>
    <circle cx="80" cy="84" r="34" stroke="var(--color-success)" stroke-width="2" class="il-ring" style="--i:0" />
  {:else if scene === "keys"}
    <g class="anim-float">
      <circle cx="62" cy="82" r="20" fill="var(--color-panel)" stroke="var(--color-line)" stroke-width="2" />
      <circle cx="62" cy="82" r="7" fill="var(--color-accent)" fill-opacity="0.85" />
      <path d="M82 82h46M112 82v14M124 82v10" stroke="var(--color-accent)" stroke-width="6" stroke-linecap="round" />
    </g>
    <path d="M118 52v8M114 56h8" stroke="var(--color-warning)" stroke-width="3" stroke-linecap="round" class="il-sparkle" style="--i:1" />
  {/if}
</svg>
