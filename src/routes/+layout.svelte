<script lang="ts">
  import "../app.css";
  import { settings } from "$lib/stores/settings.svelte";
  let { children } = $props();

  // Apply the app colour theme, following the OS when set to "system".
  $effect(() => {
    const pref = settings.prefs.appTheme;
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const light = pref === "light" || (pref === "system" && media.matches);
      document.documentElement.dataset.theme = light ? "light" : "dark";
    };
    apply();
    if (pref !== "system") return;
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  });
</script>

{@render children()}
