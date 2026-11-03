// The icons of the step kinds (lucide), by the names the schema gives them.
import { Container, Download, Globe, Hourglass, Plug, Terminal, Timer, Upload } from "lucide-svelte";
import type { StepKind } from "$lib/runbookdoc";

export const KIND_ICON: Record<StepKind, typeof Terminal> = {
  run: Terminal,
  wait: Hourglass,
  upload: Upload,
  download: Download,
  delay: Timer,
  http: Globe,
  tcp: Plug,
  docker: Container,
};
