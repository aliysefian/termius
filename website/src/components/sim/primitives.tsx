import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import type { DemoState } from "@/engine/types";

/** Marks an element as something the simulated cursor can point at. */
export const demo = (id: string) => ({ "data-demo": id });

export interface SimProps {
  state: DemoState;
}

export function SimButton({ id, children, primary, danger, className, active }: { id?: string; children: ReactNode; primary?: boolean; danger?: boolean; className?: string; active?: boolean }) {
  return (
    <span
      {...(id ? demo(id) : {})}
      className={cn(
        "inline-flex select-none items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-[12px] font-medium transition-colors",
        primary ? "bg-sim-solid text-white" : danger ? "text-sim-danger" : "border border-sim-line text-sim-muted",
        active && "bg-sim-hover text-sim-fg",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function SimIconButton({ id, children, active, label }: { id?: string; children: ReactNode; active?: boolean; label?: string }) {
  return (
    <span
      {...(id ? demo(id) : {})}
      role="img"
      aria-label={label}
      className={cn("inline-flex h-8 w-8 items-center justify-center rounded-lg text-sim-muted", active && "bg-sim-solid/20 text-sim-accent")}
    >
      {children}
    </span>
  );
}

/** A text input that shows what the demo has typed so far, with a blinking caret while it is the active target. */
export function SimInput({ id, state, placeholder, className, mono, secret }: { id: string; state: DemoState; placeholder?: string; className?: string; mono?: boolean; secret?: boolean }) {
  const value = state.fields[id] ?? "";
  const focused = state.hover === id;
  return (
    <span
      {...demo(id)}
      className={cn(
        "flex h-8 w-full items-center rounded-md border bg-sim-base px-2.5 text-[12px] text-sim-fg",
        focused ? "border-sim-accent ring-1 ring-sim-accent/50" : "border-sim-line",
        mono && "font-mono",
        className,
      )}
    >
      {value ? <span className="truncate">{secret ? "•".repeat(value.length) : value}</span> : <span className="truncate text-sim-muted">{placeholder}</span>}
      {focused && <span className="sim-caret ml-px inline-block h-3.5 w-px bg-sim-fg" />}
    </span>
  );
}

export function SimLabel({ children }: { children: ReactNode }) {
  return <span className="mb-1 block text-[10px] font-medium uppercase tracking-wide text-sim-muted">{children}</span>;
}

export function SimBadge({ tone = "neutral", children }: { tone?: "neutral" | "danger" | "warning" | "success" | "accent"; children: ReactNode }) {
  const tones = {
    neutral: "bg-sim-muted/15 text-sim-muted",
    danger: "bg-sim-danger/15 text-sim-danger",
    warning: "bg-sim-warning/15 text-sim-warning",
    success: "bg-sim-success/15 text-sim-success",
    accent: "bg-sim-accent/15 text-sim-accent",
  };
  return <span className={cn("inline-flex shrink-0 items-center rounded px-1 py-0.5 text-[10px] font-bold leading-none", tones[tone])}>{children}</span>;
}

export function StatusDot({ status }: { status: "connected" | "disconnected" | "unknown" | "connecting" }) {
  const color = { connected: "bg-sim-success", disconnected: "bg-sim-muted/50", unknown: "bg-sim-warning/70", connecting: "bg-sim-warning" }[status];
  return <span className={cn("h-2 w-2 shrink-0 rounded-full", color)} aria-hidden="true" />;
}

/** A centred modal over the stage. */
export function SimDialog({ title, width = 460, children, footer, id }: { title: string; width?: number; children: ReactNode; footer?: ReactNode; id?: string }) {
  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/50">
      <div {...(id ? demo(id) : {})} style={{ width }} className="sim-pop rounded-xl border border-sim-line bg-sim-panel shadow-2xl shadow-black/60">
        <div className="flex items-center justify-between border-b border-sim-line px-4 py-3">
          <span className="text-[13px] font-semibold text-sim-fg">{title}</span>
          <span className="text-sim-muted" aria-hidden="true">
            ×
          </span>
        </div>
        <div className="space-y-3 px-4 py-3.5">{children}</div>
        {footer && <div className="flex items-center justify-end gap-2 border-t border-sim-line px-4 py-3">{footer}</div>}
      </div>
    </div>
  );
}

export function SimToast({ state }: SimProps) {
  const t = state.toast;
  if (!t) return null;
  const color = t.kind === "success" ? "text-sim-success" : t.kind === "error" ? "text-sim-danger" : "text-sim-accent";
  return (
    <div role="status" className="sim-pop absolute bottom-10 right-4 z-30 flex items-center gap-2 rounded-lg border border-sim-line bg-sim-panel px-3.5 py-2.5 text-[12px] text-sim-fg shadow-xl shadow-black/50">
      <span className={cn("h-2 w-2 rounded-full bg-current", color)} />
      {t.text}
    </div>
  );
}

/** Wraps an element so a `highlight` target gets the pulsing ring. */
export function hl(state: DemoState, id: string) {
  return state.highlight === id ? "sim-highlight" : "";
}

export function hov(state: DemoState, id: string) {
  return state.hover === id;
}
