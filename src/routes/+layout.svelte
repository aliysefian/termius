<script lang="ts">
  import "../app.css";
  import { settings } from "$lib/stores/settings.svelte";
  import DialogHost from "$lib/components/DialogHost.svelte";
  import { CONTRAST_TOKENS } from "$lib/contrasttheme";
  import { locale } from "$lib/i18n/index.svelte";
  let { children } = $props();

  // The page language, for screen readers and spell-checkers.
  $effect(() => {
    document.documentElement.lang = locale.code;
  });

  // Apply the app colour theme, following the OS when set to "system".
  $effect(() => {
    const pref = settings.prefs.appTheme;
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const light = pref === "light" || (pref === "system" && media.matches);
      const root = document.documentElement;
      root.dataset.theme = pref === "contrast" ? "contrast" : light ? "light" : "dark";
      for (const [name, value] of Object.entries(CONTRAST_TOKENS)) {
        if (pref === "contrast") root.style.setProperty(`--color-${name}`, value);
        else root.style.removeProperty(`--color-${name}`);
      }
    };
    apply();
    if (pref !== "system") return;
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  });
</script>

{@render children()}
<DialogHost />
