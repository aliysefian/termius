import { ChevronDown, ChevronRight, Cpu, Link2, Plus, RefreshCw, Search, Server, Skull } from "lucide-react";
import { cn } from "@/lib/utils";
import { demoTmux } from "@/data/demo";
import { demo, hl, SimBadge, SimButton, StatusDot, type SimProps } from "./primitives";

const TABS = ["Panes Grid", "Panes List", "Windows", "Session Info", "Resources", "Terminal"];

export function MockTmuxManager({ state }: SimProps) {
  const session = demoTmux.find((s) => s.id === state.selected.session);
  const win = session?.windows.find((w) => w.id === state.selected.window) ?? session?.windows[0];
  const tab = state.selected.tmuxTab ?? "Panes Grid";
  const pane = win?.panes.find((p) => p.id === state.selected.pane);
  const windows = demoTmux.reduce((n, s) => n + s.windows.length, 0);
  const panes = demoTmux.reduce((n, s) => n + s.windows.reduce((m, w) => m + w.panes.length, 0), 0);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-tl-xl bg-sim-base">
      <div className="flex h-11 shrink-0 items-center gap-4 border-b border-sim-line px-4">
        <span className="text-[14px] font-semibold text-sim-fg">Tmux</span>
        <span className="text-[11px] text-sim-muted">
          1 host · {demoTmux.length} sessions · {windows} windows · {panes} panes
        </span>
        <span className="ml-auto flex items-center gap-1.5 text-[11px] text-sim-success">
          <RefreshCw size={12} /> Live · 5s
        </span>
        <SimButton primary id="tmux-new">
          <Plus size={12} /> New Session
        </SimButton>
      </div>
      <div className="flex min-h-0 flex-1">
        {/* Explorer */}
        <aside className="flex w-[238px] shrink-0 flex-col border-r border-sim-line bg-sim-panel/40 p-2.5">
          <span className="mb-2 flex h-7 items-center gap-2 rounded-md border border-sim-line bg-sim-base px-2 text-[11px] text-sim-muted">
            <Search size={12} /> Filter
          </span>
          <div className="mb-1 flex items-center gap-1.5 px-1 text-[12px] font-medium text-sim-fg">
            <Server size={13} className="text-sim-muted" /> Production API <StatusDot status="connected" />
          </div>
          <div className="ml-3 min-h-0 flex-1 space-y-px overflow-hidden border-l border-sim-line pl-2">
            {demoTmux.map((s) => {
              const sel = s.id === state.selected.session;
              return (
                <div key={s.id}>
                  <div
                    {...demo(`tmux-session-${s.name}`)}
                    className={cn("flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[12px]", sel ? "bg-sim-solid/20 text-sim-fg" : state.hover === `tmux-session-${s.name}` ? "bg-sim-hover text-sim-fg" : "text-sim-fg", hl(state, `tmux-session-${s.name}`))}
                  >
                    {sel ? <ChevronDown size={12} /> : <ChevronRight size={12} className="text-sim-muted" />}
                    <span className="flex-1 truncate font-medium">{s.name}</span>
                    {s.attached && <SimBadge tone="success">attached</SimBadge>}
                  </div>
                  {sel &&
                    s.windows.map((w) => (
                      <div
                        key={w.id}
                        {...demo(`tmux-win-${w.name}`)}
                        className={cn("ml-4 flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[11.5px]", w.id === win?.id ? "bg-sim-solid/10 text-sim-accent" : "text-sim-muted", state.hover === `tmux-win-${w.name}` && "bg-sim-hover")}
                      >
                        <span className="font-mono text-[10px] opacity-70">{w.id.replace("@", "")}:</span>
                        {w.name}
                        <span className="ml-auto text-[10px] opacity-70">{w.panes.length}p</span>
                      </div>
                    ))}
                </div>
              );
            })}
          </div>
        </aside>

        {/* Main */}
        <section className="flex min-w-0 flex-1 flex-col">
          {session && win ? (
            <>
              <div className="flex items-center gap-1.5 border-b border-sim-line px-4 py-2 text-[12px] text-sim-muted">
                Production API <ChevronRight size={11} /> <span>{session.name}</span> <ChevronRight size={11} /> <span className="text-sim-fg">{win.name}</span>
              </div>
              <div className="flex gap-1 border-b border-sim-line px-3 pt-1.5 text-[12px]">
                {TABS.map((t) => (
                  <span key={t} {...demo(`tmux-tab-${t.split(" ")[1] ?? t}`)} className={cn("rounded-t-md px-3 py-1.5", t === tab ? "border-b-2 border-sim-accent text-sim-fg" : "text-sim-muted")}>
                    {t}
                  </span>
                ))}
              </div>
              <div className="min-h-0 flex-1 overflow-hidden p-3">
                {tab === "Windows" ? (
                  <div className="space-y-1.5">
                    {session.windows.map((w) => (
                      <div key={w.id} className="flex items-center gap-3 rounded-lg border border-sim-line bg-sim-panel px-3 py-2 text-[12px] text-sim-fg">
                        <span className="font-mono text-sim-muted">{w.id}</span>
                        <span className="font-medium">{w.name}</span>
                        <span className="text-sim-muted">{w.panes.length} panes</span>
                        {w.id === win.id && <SimBadge tone="accent">active</SimBadge>}
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className={cn("grid h-full gap-2", win.panes.length > 1 ? "grid-cols-2" : "grid-cols-1")}>
                    {win.panes.map((p) => (
                      <div
                        key={p.id}
                        {...demo(`tmux-pane-${p.id.replace("%", "")}`)}
                        className={cn("flex min-h-0 flex-col overflow-hidden rounded-lg border bg-sim-term", p.id === state.selected.pane ? "border-sim-accent ring-1 ring-sim-accent/40" : "border-sim-line", hl(state, `tmux-pane-${p.id.replace("%", "")}`))}
                      >
                        <div className="flex items-center justify-between border-b border-sim-line px-2.5 py-1 text-[10.5px] text-sim-muted">
                          <span className="font-mono">
                            {p.id} · {p.cmd}
                          </span>
                          <span>Live Preview · Read Only</span>
                        </div>
                        <div className="min-h-0 flex-1 overflow-hidden whitespace-pre p-2.5 font-mono text-[11px] leading-[16px] text-sim-fg">
                          {p.lines.map((l, i) => (
                            <div key={i}>{l || " "}</div>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 text-sim-muted">
              <Server size={28} className="opacity-50" />
              <p className="text-[13px] text-sim-fg">Select a session</p>
              <p className="text-[12px]">Sessions belong to the host's tmux server, not to a tab.</p>
            </div>
          )}
        </section>

        {/* Inspector */}
        <aside className="w-[214px] shrink-0 space-y-3 border-l border-sim-line bg-sim-panel/40 p-3 text-[12px]">
          <div className="text-[10.5px] font-semibold uppercase tracking-wide text-sim-muted">Inspector</div>
          {session ? (
            <>
              <div>
                <div className="text-[13px] font-semibold text-sim-fg">{pane ? `Pane ${pane.id}` : session.name}</div>
                <div className="text-sim-muted">{pane ? pane.cmd : `${session.windows.length} windows · ${session.attached ? "attached" : "detached"}`}</div>
              </div>
              <div className="flex items-center gap-1.5 rounded-md border border-sim-line bg-sim-base px-2 py-1.5 text-sim-muted">
                <Cpu size={12} /> CPU 3% · Mem 112 MB
              </div>
              <div className="space-y-1.5">
                <SimButton id="tmux-attach" primary className={cn("w-full", hl(state, "tmux-attach"))}>
                  <Link2 size={12} /> Attach
                </SimButton>
                <SimButton className="w-full" id="tmux-live">
                  Live Terminal
                </SimButton>
                <SimButton danger className="w-full">
                  <Skull size={12} /> Kill Session
                </SimButton>
              </div>
            </>
          ) : (
            <p className="text-sim-muted">Details and actions for what you select appear here.</p>
          )}
        </aside>
      </div>
    </div>
  );
}
