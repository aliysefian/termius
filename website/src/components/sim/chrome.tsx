import { Activity, ArrowLeftRight, Bell, Code, Columns2, Container, Database, FolderSync, HeartPulse, KeyRound, LayoutDashboard, ListChecks, Lock, Plus, Search, Server, Settings, SquareTerminal, Star, Wrench } from "lucide-react";
import { cn } from "@/lib/utils";
import type { DemoState } from "@/engine/types";
import { demo, SimIconButton, StatusDot } from "./primitives";

export const LOGO = "bg-gradient-to-br from-[#8f72ff] to-[#4b32e8]";

const GROUPS = [
  [
    { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
    { id: "hosts", label: "Hosts", icon: Server },
    { id: "favorites", label: "Favorites", icon: Star },
    { id: "fleet", label: "Fleet", icon: Activity },
  ],
  [
    { id: "snippets", label: "Snippets", icon: Code },
    { id: "sftp", label: "Files", icon: FolderSync },
    { id: "tunnels", label: "Tunnels", icon: ArrowLeftRight },
  ],
  [
    { id: "databases", label: "Databases", icon: Database },
    { id: "containers", label: "Containers", icon: Container },
    { id: "ops", label: "Ops", icon: Wrench },
    { id: "tmux", label: "Tmux", icon: SquareTerminal },
    { id: "runbooks", label: "Runbooks", icon: ListChecks },
    { id: "monitoring", label: "Monitoring", icon: HeartPulse },
  ],
] as const;

/** The activity bar on the left, in the same groups and order as the desktop app. */
export function SimRail({ state }: { state: DemoState }) {
  const current = state.view === "terminal" ? "hosts" : state.view === "keys" ? "manage" : state.view;
  return (
    <nav aria-hidden="true" className="flex w-[64px] shrink-0 flex-col items-center bg-sim-base py-1.5">
      {GROUPS.map((g, gi) => (
        <div key={gi} className="flex flex-col items-center">
          {gi > 0 && <div className="my-1 h-px w-8 bg-sim-line" />}
          {g.map((item) => {
            const on = current === item.id;
            return (
              <span
                key={item.id}
                {...demo(`rail-${item.id}`)}
                className={cn("mb-px flex h-[35px] w-[54px] flex-col items-center justify-center gap-0.5 rounded-lg", on ? "bg-sim-solid/20 text-sim-accent" : "text-sim-muted")}
              >
                <item.icon size={16} />
                <span className="text-[9px] font-medium leading-none">{item.label}</span>
              </span>
            );
          })}
        </div>
      ))}
      <div className="my-1 h-px w-8 bg-sim-line" />
      <span {...demo("rail-keys")} className={cn("flex h-[35px] w-[54px] flex-col items-center justify-center gap-0.5 rounded-lg", current === "manage" ? "bg-sim-solid/20 text-sim-accent" : "text-sim-muted")}>
        <KeyRound size={16} />
        <span className="text-[9px] font-medium leading-none">Keys</span>
      </span>
      <div className="flex-1" />
      <div className="mb-1 flex gap-3 text-sim-muted">
        <Lock size={14} />
        <Settings size={14} />
      </div>
    </nav>
  );
}

export function SimTopBar({ state }: { state: DemoState }) {
  return (
    <header aria-hidden="true" className="flex h-11 shrink-0 items-center gap-3 bg-sim-base px-3">
      <div className="flex w-[148px] items-center gap-2">
        <span className={cn("flex h-6 w-6 items-center justify-center rounded-[7px] text-white", LOGO)}>
          <SquareTerminal size={13} />
        </span>
        <span className="text-[14px] font-semibold tracking-tight text-sim-fg">SSHVault</span>
      </div>
      <span {...demo("topbar-search")} className="mx-auto flex h-8 w-full max-w-[420px] items-center gap-2 rounded-xl border border-sim-line bg-sim-panel px-3 text-[12px] text-sim-muted">
        <Search size={13} />
        <span className="flex-1">{state.fields["topbar-search"] || "Search"}</span>
        <kbd className="rounded border border-sim-line px-1 text-[10px]">Ctrl+K</kbd>
      </span>
      <div className="flex items-center gap-0.5">
        <SimIconButton id="topbar-new" label="New connection">
          <Plus size={16} />
        </SimIconButton>
        <SimIconButton label="Local terminal">
          <SquareTerminal size={16} />
        </SimIconButton>
        <SimIconButton id="topbar-split" label="Split right" active={state.flags.split}>
          <Columns2 size={16} />
        </SimIconButton>
        <SimIconButton label="Alerts">
          <Bell size={16} />
        </SimIconButton>
      </div>
    </header>
  );
}

export function SimStatusBar({ state }: { state: DemoState }) {
  const connected = state.view === "terminal" && Boolean(state.selected.tab);
  return (
    <footer aria-hidden="true" className="flex h-7 shrink-0 items-center gap-3 border-t border-sim-line bg-sim-base px-3 text-[11px] text-sim-muted">
      <span className="flex items-center gap-2">
        <StatusDot status={connected ? "connected" : "unknown"} />
        {connected ? <span className="text-sim-fg">{state.selected.tab}</span> : "No open connection"}
      </span>
      {connected && <span className="rounded bg-sim-muted/10 px-1.5 py-0.5 text-sim-success">SSH · UTF-8</span>}
      <span className="ml-auto flex items-center gap-1.5">
        <Lock size={11} /> Vault unlocked
      </span>
    </footer>
  );
}
