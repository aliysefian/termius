<script lang="ts">
  import type { ITheme } from "@xterm/xterm";

  // A miniature terminal drawn in a colour scheme's own colours: prompt, path, keyword, error and success,
  // so a scheme is judged by how its text reads and not by a row of colour chips. Plain markup, not an xterm
  // instance: a settings page lists dozens of these.
  let { theme }: { theme: ITheme } = $props();
</script>

<span data-scheme-sample class="block select-none overflow-hidden whitespace-nowrap px-2 py-1.5 font-mono text-[10px] leading-[14px]" style:background={theme.background} style:color={theme.foreground} aria-hidden="true">
  <span class="block"><span style:color={theme.green}>$</span> <span style:color={theme.magenta}>git</span> push <span style:color={theme.blue}>~/app</span></span>
  <span class="block" style:color={theme.red}>error: rejected</span>
  <span class="block" style:color={theme.green}>✓ 3 files changed</span>
  <span class="block"><span style:color={theme.green}>$</span> <span class="inline-block h-[10px] w-[5px] translate-y-px" style:background={theme.cursor ?? theme.foreground}></span></span>
</span>
