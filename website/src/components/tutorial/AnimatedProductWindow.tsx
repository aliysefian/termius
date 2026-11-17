import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** Window chrome around the simulated app, with an honest "simulated" label. */
export function AnimatedProductWindow({ children, title = "SSHVault", className, label }: { children: ReactNode; title?: string; className?: string; label?: string }) {
  return (
    <div className={cn("overflow-hidden rounded-2xl border border-line bg-sim-base shadow-[0_30px_80px_-30px_var(--glow),0_0_0_1px_rgba(255,255,255,0.03)]", className)}>
      <div className="flex h-9 items-center gap-3 border-b border-sim-line bg-[#0a101a] px-3.5">
        <span className="flex gap-1.5" aria-hidden="true">
          <span className="size-2.5 rounded-full bg-[#f5707f]/80" />
          <span className="size-2.5 rounded-full bg-[#f5b942]/80" />
          <span className="size-2.5 rounded-full bg-[#2dd59a]/80" />
        </span>
        <span className="mx-auto text-[12px] font-medium text-sim-muted">{title}</span>
        <span className="rounded-full border border-sim-line px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-sim-muted" title="This is an animated simulation built with fictional data, not a screen recording.">
          {label ?? "Simulated demo"}
        </span>
      </div>
      {children}
    </div>
  );
}
