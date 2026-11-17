import { Plus, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { MockTerminal } from "./MockTerminal";
import { demo, hl, StatusDot, type SimProps } from "./primitives";

export const TAB_DEFS = [
  { id: "prod-api", label: "Production API", prompt: "demo@prod-api:~$ ", dot: "connected" as const },
  { id: "db", label: "Database Server", prompt: "admin@db:~$ ", dot: "connected" as const },
  { id: "new", label: "New host", prompt: "demo@new-host:~$ ", dot: "connected" as const },
  { id: "local", label: "Local", prompt: "demo@laptop:~$ ", dot: "connected" as const },
];

export function MockTerminalView({ state }: SimProps) {
  const open = TAB_DEFS.filter((t) => state.flags[`tab-${t.id}`]);
  const active = open.find((t) => t.id === state.selected.tab) ?? open[0];
  const split = Boolean(state.flags.split);
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-tl-xl border-l border-t border-sim-line bg-sim-term">
      <div className="flex h-9 shrink-0 items-end gap-0.5 border-b border-sim-line bg-sim-base px-2 pt-1.5">
        {open.map((t) => {
          const on = t.id === active?.id;
          const label = t.id === "new" ? state.fields["f-label"] || t.label : state.fields[`rename-${t.id}`] || t.label;
          return (
            <span
              key={t.id}
              {...demo(`tab-${t.id}`)}
              className={cn("flex h-[30px] max-w-[190px] items-center gap-2 rounded-t-lg border border-b-0 px-3 text-[12px]", on ? "border-sim-line bg-sim-term text-sim-fg" : "border-transparent text-sim-muted", hl(state, `tab-${t.id}`))}
            >
              <StatusDot status={t.dot} />
              <span className="truncate">{label}</span>
              <X size={11} className="shrink-0 opacity-60" />
            </span>
          );
        })}
        <span {...demo("tab-new")} role="img" aria-label="New tab" className="mb-1 ml-1 inline-flex h-6 w-6 items-center justify-center rounded-md text-sim-muted">
          <Plus size={14} />
        </span>
      </div>
      {active ? (
        <div className="flex min-h-0 flex-1">
          <MockTerminal
            id={`term-${active.id}`}
            lines={state.terminals[active.id] ?? []}
            input={state.fields[`term-input-${active.id}`] ?? ""}
            prompt={active.prompt}
            focused={!split || state.selected.pane !== "2"}
            className="min-w-0"
          />
          {split && (
            <>
              <div className="w-px bg-sim-line" />
              <MockTerminal
                id={`term-${active.id}-2`}
                lines={state.terminals[`${active.id}-2`] ?? []}
                input={state.fields[`term-input-${active.id}-2`] ?? ""}
                prompt={active.prompt}
                focused={state.selected.pane === "2"}
                className="min-w-0"
              />
            </>
          )}
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center text-[12px] text-sim-muted">No open tabs</div>
      )}
    </div>
  );
}
