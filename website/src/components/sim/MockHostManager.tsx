import { ChevronDown, Copy, FolderSync, Monitor, Pencil, Play, Plus, Search, Star } from "lucide-react";
import { cn } from "@/lib/utils";
import { demoHosts, type DemoHost } from "@/data/demo";
import type { DemoState } from "@/engine/types";
import { demo, hl, SimBadge, SimButton, SimDialog, SimInput, SimLabel, StatusDot, type SimProps } from "./primitives";

/** The host list as the demo currently shows it: a created host is added and an edited one is overridden. */
export function visibleHosts(state: DemoState): DemoHost[] {
  const list = demoHosts.map((h) => (state.flags[`edited-${h.id}`] ? overrideHost(h, state) : h));
  if (state.flags.created) {
    list.splice(3, 0, {
      id: "new",
      name: state.fields["f-label"] || "New host",
      hostname: state.fields["f-hostname"] || "203.0.113.50",
      username: state.fields["f-username"] || "demo",
      port: Number(state.fields["f-port"]) || 22,
      status: state.flags.connectedNew ? "connected" : "unknown",
      group: "Production",
      tags: [],
    });
  }
  return list;
}

function overrideHost(h: DemoHost, s: DemoState): DemoHost {
  return {
    ...h,
    name: s.fields["f-label"] || h.name,
    hostname: s.fields["f-hostname"] || h.hostname,
    username: s.fields["f-username"] || h.username,
    port: Number(s.fields["f-port"]) || h.port,
    tags: s.fields["f-tags"] ? s.fields["f-tags"].split(",").map((t) => t.trim()) : h.tags,
  };
}

export function hostMatches(h: DemoHost, q: string) {
  const s = q.trim().toLowerCase();
  if (!s) return true;
  return [h.name, h.hostname, h.username, ...(h.tags ?? [])].some((v) => v.toLowerCase().includes(s));
}

function HostRowSim({ host, state }: { host: DemoHost; state: DemoState }) {
  const selected = state.selected.host === host.id;
  const hovered = state.hover === `host-row-${host.id}`;
  return (
    <div
      {...demo(`host-row-${host.id}`)}
      className={cn(
        "relative flex items-center gap-2 rounded-lg py-1.5 pl-3 pr-2",
        selected ? "bg-sim-solid/15 ring-1 ring-inset ring-sim-solid/40" : hovered ? "bg-sim-hover" : "",
        hl(state, `host-row-${host.id}`),
      )}
    >
      <StatusDot status={host.status} />
      <Monitor size={14} className="shrink-0 text-sim-muted" />
      <span className={cn("min-w-0 flex-1 truncate text-[12.5px]", selected ? "font-medium text-sim-fg" : "text-sim-fg")}>{host.name}</span>
      {host.env && <SimBadge tone={host.env === "PROD" ? "danger" : host.env === "STG" ? "warning" : "success"}>{host.env}</SimBadge>}
      {host.favorite && <Star size={11} className="fill-sim-warning text-sim-warning" />}
    </div>
  );
}

export function HostList({ state }: SimProps) {
  const q = state.fields["host-search"] ?? "";
  const hosts = visibleHosts(state).filter((h) => hostMatches(h, q));
  const groups = [...new Set(hosts.map((h) => h.group))];
  return (
    <aside className="flex w-[290px] shrink-0 flex-col rounded-tl-xl border-r border-sim-line bg-sim-panel/60">
      <div className="flex items-center gap-2 px-3 pb-2 pt-3">
        <span className="text-[13px] font-semibold text-sim-fg">Hosts</span>
        <span className="rounded bg-sim-muted/15 px-1.5 text-[10px] text-sim-muted">{hosts.length}</span>
        <span {...demo("host-add")} className={cn("ml-auto inline-flex h-7 w-7 items-center justify-center rounded-lg bg-sim-solid text-white", hl(state, "host-add"))} role="img" aria-label="Add host">
          <Plus size={15} />
        </span>
      </div>
      <div className="px-3 pb-2">
        <SimInputWithIcon id="host-search" state={state} placeholder="Search hosts" />
      </div>
      <div className="min-h-0 flex-1 overflow-hidden px-1.5">
        {groups.map((g) => (
          <div key={g} className="mb-1">
            <div className="flex items-center gap-1 px-2 py-1 text-[10.5px] font-semibold uppercase tracking-wide text-sim-muted">
              <ChevronDown size={11} />
              {g}
            </div>
            {hosts
              .filter((h) => h.group === g)
              .map((h) => (
                <HostRowSim key={h.id} host={h} state={state} />
              ))}
          </div>
        ))}
        {hosts.length === 0 && <p className="px-3 py-6 text-center text-[12px] text-sim-muted">No hosts match “{q}”.</p>}
      </div>
    </aside>
  );
}

function SimInputWithIcon({ id, state, placeholder }: { id: string; state: DemoState; placeholder: string }) {
  const value = state.fields[id] ?? "";
  const focused = state.hover === id;
  return (
    <span {...demo(id)} className={cn("flex h-8 items-center gap-2 rounded-lg border bg-sim-base px-2.5 text-[12px]", focused ? "border-sim-accent ring-1 ring-sim-accent/50" : "border-sim-line")}>
      <Search size={13} className="shrink-0 text-sim-muted" />
      {value ? <span className="text-sim-fg">{value}</span> : <span className="text-sim-muted">{placeholder}</span>}
      {focused && <span className="sim-caret inline-block h-3.5 w-px bg-sim-fg" />}
    </span>
  );
}

function Info({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="rounded-lg border border-sim-line bg-sim-panel px-3 py-2">
      <div className="text-[10px] font-medium uppercase tracking-wide text-sim-muted">{label}</div>
      <div className={cn("mt-0.5 truncate text-[12.5px] text-sim-fg", mono && "font-mono")}>{value}</div>
    </div>
  );
}

export function HostDetails({ state }: SimProps) {
  const host = visibleHosts(state).find((h) => h.id === state.selected.host);
  if (!host) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center text-sim-muted">
        <Monitor size={30} className="opacity-50" />
        <p className="text-[13px] font-medium text-sim-fg">Select a host</p>
        <p className="max-w-[240px] text-[12px]">Double-click a host to connect, or press + to add one.</p>
      </div>
    );
  }
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-4 p-5">
      <div className="flex items-start gap-3">
        <span className="flex h-11 w-11 items-center justify-center rounded-xl bg-sim-solid/20 text-sim-accent">
          <Monitor size={22} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-[17px] font-semibold text-sim-fg">{host.name}</h3>
            {host.env && <SimBadge tone={host.env === "PROD" ? "danger" : host.env === "STG" ? "warning" : "success"}>{host.env}</SimBadge>}
          </div>
          <p className="font-mono text-[12px] text-sim-muted">
            {host.username}@{host.hostname}:{host.port}
          </p>
        </div>
        <div className="flex gap-2">
          <SimButton id="host-edit" className={hl(state, "host-edit")}>
            <Pencil size={12} /> Edit
          </SimButton>
          <SimButton id="host-sftp">
            <FolderSync size={12} /> SFTP
          </SimButton>
          <SimButton id="host-connect" primary className={hl(state, "host-connect")}>
            <Play size={12} /> Connect
          </SimButton>
        </div>
      </div>
      <div className="grid grid-cols-3 gap-2.5">
        <Info label="Address" value={host.hostname} mono />
        <Info label="Port" value={String(host.port)} mono />
        <Info label="User" value={host.username} mono />
        <Info label="Group" value={host.group} />
        <Info label="Authentication" value="SSH key (Ed25519)" />
        <Info label="Status" value={host.status === "connected" ? "Reachable · 24 ms" : host.status === "disconnected" ? "Not connected" : "Not checked yet"} />
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] text-sim-muted">Tags</span>
        {(host.tags?.length ? host.tags : ["none"]).map((t) => (
          <SimBadge key={t} tone="accent">
            {t}
          </SimBadge>
        ))}
      </div>
      <div className="rounded-lg border border-sim-line bg-sim-panel p-3 text-[12px] text-sim-muted">
        <div className="mb-1 flex items-center gap-1.5 font-medium text-sim-fg">
          <Copy size={12} /> Copy as ssh command
        </div>
        <code className="font-mono text-sim-accent">
          ssh {host.port !== 22 ? `-p ${host.port} ` : ""}
          {host.username}@{host.hostname}
        </code>
      </div>
    </div>
  );
}

export function MockHostManager({ state }: SimProps) {
  return (
    <div className="flex min-h-0 flex-1 overflow-hidden rounded-tl-xl bg-sim-base">
      <HostList state={state} />
      <HostDetails state={state} />
    </div>
  );
}

const AUTH = [
  { id: "password", label: "Password" },
  { id: "key", label: "SSH key" },
  { id: "credential", label: "Credential" },
  { id: "ask", label: "Ask" },
];

export function HostFormDialog({ state }: SimProps) {
  const editing = Boolean(state.flags.editingHost);
  const auth = state.selected.auth ?? "key";
  return (
    <SimDialog
      id="host-form"
      title={editing ? "Edit host" : "New host"}
      width={520}
      footer={
        <>
          <SimButton id="f-test">Test connection</SimButton>
          <SimButton id="f-cancel">Cancel</SimButton>
          <SimButton id="f-save" primary className={hl(state, "f-save")}>
            Save
          </SimButton>
        </>
      }
    >
      <div className="flex gap-1 border-b border-sim-line pb-2 text-[12px]">
        {["Connection", "Route", "Organise", "Automation"].map((t, i) => (
          <span key={t} className={cn("rounded-md px-2.5 py-1", i === 0 ? "bg-sim-solid/20 text-sim-accent" : "text-sim-muted")}>
            {t}
          </span>
        ))}
      </div>
      <div>
        <SimLabel>Label</SimLabel>
        <SimInput id="f-label" state={state} placeholder="Production API" />
      </div>
      <div className="grid grid-cols-[1fr_96px] gap-3">
        <div>
          <SimLabel>Hostname</SimLabel>
          <SimInput id="f-hostname" state={state} placeholder="192.0.2.10 or host.example.com" mono />
        </div>
        <div>
          <SimLabel>Port</SimLabel>
          <SimInput id="f-port" state={state} placeholder="22" mono />
        </div>
      </div>
      <div>
        <SimLabel>Credentials</SimLabel>
        <div className="flex gap-1.5">
          {AUTH.map((a) => (
            <span key={a.id} {...demo(`f-auth-${a.id}`)} className={cn("rounded-md border px-2.5 py-1 text-[12px]", auth === a.id ? "border-sim-solid bg-sim-solid/15 text-sim-fg" : "border-sim-line text-sim-muted")}>
              {a.label}
            </span>
          ))}
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div>
          <SimLabel>Username</SimLabel>
          <SimInput id="f-username" state={state} placeholder="demo" mono />
        </div>
        <div>
          <SimLabel>Tags</SimLabel>
          <SimInput id="f-tags" state={state} placeholder="api, eu" />
        </div>
      </div>
    </SimDialog>
  );
}

export function ConnectingDialog({ state }: SimProps) {
  const host = visibleHosts(state).find((h) => h.id === state.selected.host);
  const p = state.progress.connect ?? 0;
  const steps = ["Resolving address", "Opening connection", "Verifying host key", "Authenticating with key"];
  const at = Math.min(steps.length - 1, Math.floor((p / 100) * steps.length));
  return (
    <SimDialog title={`Connecting to ${host?.name ?? "host"}`} width={400}>
      <div className="space-y-2.5">
        {steps.map((s, i) => (
          <div key={s} className={cn("flex items-center gap-2 text-[12px]", i <= at ? "text-sim-fg" : "text-sim-muted")}>
            <span className={cn("h-1.5 w-1.5 rounded-full", i < at || p >= 100 ? "bg-sim-success" : i === at ? "bg-sim-warning" : "bg-sim-muted/40")} />
            {s}
          </div>
        ))}
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-sim-base">
        <div className="h-full rounded-full bg-sim-solid" style={{ width: `${p}%` }} />
      </div>
    </SimDialog>
  );
}

export function HostKeyDialog({ state }: SimProps) {
  const host = visibleHosts(state).find((h) => h.id === state.selected.host);
  return (
    <SimDialog
      title="Trust this server?"
      width={480}
      footer={
        <>
          <SimButton id="hostkey-cancel">Cancel</SimButton>
          <SimButton id="trust-key" primary className={hl(state, "trust-key")}>
            Trust and connect
          </SimButton>
        </>
      }
    >
      <p className="text-[12px] text-sim-muted">
        SSHVault has not seen <span className="font-mono text-sim-fg">{host?.hostname}</span> before. Check the fingerprint with the server's owner, then trust it. Later connections are compared against it.
      </p>
      <div className="rounded-lg border border-sim-line bg-sim-base p-3 font-mono text-[11px] leading-5">
        <div className="text-sim-muted">ED25519</div>
        <div className="break-all text-sim-fg">SHA256:3x4mpl3F1ngerpr1ntD0cum3ntat10n0nlyAbCdEf</div>
      </div>
    </SimDialog>
  );
}
