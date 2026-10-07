<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { settings } from "$lib/stores/settings.svelte";
  import { themeById } from "$lib/themes";

  let container = $state<HTMLDivElement>();
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let resizeObserver: ResizeObserver | undefined;

  function write() {
    if (!term) return;
    term.clear();
    term.write("\x1b[38;2;120;200;120muser@host\x1b[0m:\x1b[38;2;120;160;255m~/sshvault\x1b[0m$ ls\r\n");
    term.write("\x1b[1;34msrc\x1b[0m  \x1b[1;32mbuild.sh\x1b[0m  \x1b[1;36mREADME.md\x1b[0m  \x1b[1;31merror.log\x1b[0m\r\n");
    term.write("\x1b[38;2;120;200;120muser@host\x1b[0m:\x1b[38;2;120;160;255m~/sshvault\x1b[0m$ git diff\r\n");
    term.write("\x1b[32m+ a line that was added\x1b[0m\r\n");
    term.write("\x1b[31m- a line that was removed\x1b[0m\r\n");
    term.write("\x1b[1mBold text\x1b[0m stays readable at any contrast setting.\r\n");
    term.write("\x1b[38;2;120;200;120muser@host\x1b[0m:\x1b[38;2;120;160;255m~/sshvault\x1b[0m$ ");
  }

  function safeFit() {
    if (container && container.clientWidth > 0 && container.clientHeight > 0) fit?.fit();
  }

  onMount(() => {
    const p = settings.prefs;
    term = new Terminal({
      theme: themeById(p.themeId, p.customThemes).theme,
      fontFamily: p.fontFamily,
      fontSize: p.fontSize,
      lineHeight: p.lineHeight,
      cursorStyle: p.cursorStyle,
      cursorBlink: p.cursorBlink,
      letterSpacing: p.letterSpacing,
      minimumContrastRatio: p.minimumContrastRatio,
      drawBoldTextInBrightColors: p.boldAsBright,
      disableStdin: true,
      allowProposedApi: true,
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container!);
    write();
    safeFit();
    resizeObserver = new ResizeObserver(safeFit);
    resizeObserver.observe(container!);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    term?.dispose();
  });

  // Every change in Settings reaches this preview the same way it reaches a
  // real session, so what's shown here is what a terminal will actually do.
  $effect(() => {
    const p = settings.prefs;
    if (!term) return;
    term.options.theme = themeById(p.themeId, p.customThemes).theme;
    term.options.fontFamily = p.fontFamily;
    term.options.fontSize = p.fontSize;
    term.options.lineHeight = p.lineHeight;
    term.options.cursorStyle = p.cursorStyle;
    term.options.cursorBlink = p.cursorBlink;
    term.options.letterSpacing = p.letterSpacing;
    term.options.minimumContrastRatio = p.minimumContrastRatio;
    term.options.drawBoldTextInBrightColors = p.boldAsBright;
    if (p.cursorColor) term.options.theme = { ...term.options.theme, cursor: p.cursorColor };
    write();
    safeFit();
  });
</script>

<div
  bind:this={container}
  class="h-40 overflow-hidden rounded-md border border-line"
  style:padding="{settings.prefs.terminalPadding}px"
  style:background={themeById(settings.prefs.themeId, settings.prefs.customThemes).theme.background}
  inert
></div>
