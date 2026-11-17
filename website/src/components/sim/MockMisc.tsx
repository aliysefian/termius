import { ArrowLeftRight, Check, Globe, KeyRound, Play, Plus, Shield, Terminal, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import { demoKeys, demoSnippets, demoTunnels, demoHosts } from "@/data/demo";
import { demo, hl, SimBadge, SimButton, SimDialog, SimInput, SimLabel, StatusDot, type SimProps } from "./primitives";

function PageHeader({ title, sub, children }: { title: string; sub: string; children?: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3 border-b border-sim-line px-5 py-3">
      <div>
        <h3 className="text-[15px] font-semibold text-sim-fg">{title}</h3>
        <p className="text-[11.5px] text-sim-muted">{sub}</p>
      </div>
      <div className="ml-auto flex items-center gap-2">{children}</div>
    </div>
  );
}

/* ---------- Tunnels ---------- */
export function MockTunnels({ state }: SimProps) {
  const rules = [...demoTunnels];
  if (state.flags.tunnelCreated) {
    rules.unshift({
      id: "t-new",
      name: state.fields["tn-name"] || "Admin UI",
      type: "Local",
      mapping: `127.0.0.1:${state.fields["tn-lport"] || "8080"} → ${state.fields["tn-dest"] || "localhost"}:${state.fields["tn-dport"] || "80"}`,
      host: "Production API",
      state: state.flags.tunnelRunning ? "running" : "stopped",
      uptime: state.flags.tunnelRunning ? "0m" : "",
    });
  }
  const active = rules.filter((r) => r.state === "running").length;
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-tl-xl bg-sim-base">
      <PageHeader title="Tunnels" sub="Local, remote and SOCKS port forwards over your SSH hosts.">
        {state.selected.tunnel === "t-new" && !state.flags.tunnelRunning && (
          <SimButton id="tunnel-start" className={hl(state, "tunnel-start")}>
            <Play size={11} /> Start
          </SimButton>
        )}
        <SimButton id="tunnel-create" primary className={hl(state, "tunnel-create")}>
          <Plus size={12} /> Create Tunnel
        </SimButton>
      </PageHeader>
      <div className="grid grid-cols-3 gap-3 px-5 py-3">
        {[
          ["Active", active],
          ["Stopped", rules.length - active],
          ["Failed", 0],
        ].map(([k, v]) => (
          <div key={k} className="rounded-lg border border-sim-line bg-sim-panel px-3 py-2">
            <div className="text-[10px] uppercase tracking-wide text-sim-muted">{k}</div>
            <div className="text-[20px] font-semibold text-sim-fg">{v}</div>
          </div>
        ))}
      </div>
      <div className="px-5">
        <div className="grid grid-cols-[1.2fr_70px_2fr_1fr_90px_80px] gap-3 border-b border-sim-line pb-1.5 text-[10px] font-medium uppercase tracking-wide text-sim-muted">
          <span>Name</span>
          <span>Type</span>
          <span>Mapping</span>
          <span>Host</span>
          <span>State</span>
          <span>Uptime</span>
        </div>
        {rules.map((r) => (
          <div key={r.id} {...demo(`tunnel-row-${r.id}`)} className={cn("grid grid-cols-[1.2fr_70px_2fr_1fr_90px_80px] items-center gap-3 border-b border-sim-line/60 py-2.5 text-[12px]", state.selected.tunnel === r.id && "bg-sim-solid/10", hl(state, `tunnel-row-${r.id}`))}>
            <span className="truncate font-medium text-sim-fg">{r.name}</span>
            <SimBadge tone="accent">{r.type}</SimBadge>
            <span className="truncate font-mono text-[11px] text-sim-muted">{r.mapping}</span>
            <span className="truncate text-sim-fg">{r.host}</span>
            <span className={cn("flex items-center gap-1.5", r.state === "running" ? "text-sim-success" : "text-sim-muted")}>
              <StatusDot status={r.state === "running" ? "connected" : "disconnected"} />
              {r.state === "running" ? "Running" : "Stopped"}
            </span>
            <span className="text-sim-muted">{r.uptime || "—"}</span>
          </div>
        ))}
      </div>
      {state.flags.tunnelRunning && (
        <div {...demo("tunnel-open")} className="mx-5 mt-4 flex items-center gap-2 rounded-lg border border-sim-line bg-sim-panel px-3 py-2 text-[12px] text-sim-muted">
          <Globe size={13} className="text-sim-accent" /> Listening on <span className="font-mono text-sim-fg">127.0.0.1:{state.fields["tn-lport"] || "8080"}</span>
          <SimButton className="ml-auto">Open in browser</SimButton>
        </div>
      )}
    </div>
  );
}

export function TunnelDialog({ state }: SimProps) {
  const type = state.selected.tunnelType ?? "Local";
  return (
    <SimDialog
      title="Create Tunnel"
      width={500}
      footer={
        <>
          <SimButton>Cancel</SimButton>
          <SimButton id="tunnel-save" primary className={hl(state, "tunnel-save")}>
            Create
          </SimButton>
        </>
      }
    >
      <div>
        <SimLabel>Type</SimLabel>
        <div className="flex gap-1.5">
          {["Local", "Remote", "Dynamic"].map((t) => (
            <span key={t} {...demo(`tunnel-type-${t}`)} className={cn("flex-1 rounded-md border px-2.5 py-1.5 text-center text-[12px]", type === t ? "border-sim-solid bg-sim-solid/15 text-sim-fg" : "border-sim-line text-sim-muted")}>
              {t}
            </span>
          ))}
        </div>
        <p className="mt-1.5 font-mono text-[11px] text-sim-muted">
          ssh -L 127.0.0.1:{state.fields["tn-lport"] || "8080"}:{state.fields["tn-dest"] || "localhost"}:{state.fields["tn-dport"] || "80"} host
        </p>
      </div>
      <div>
        <SimLabel>Name</SimLabel>
        <SimInput id="tn-name" state={state} placeholder="Admin UI" />
      </div>
      <div className="grid grid-cols-3 gap-3">
        <div>
          <SimLabel>Local port</SimLabel>
          <SimInput id="tn-lport" state={state} placeholder="8080" mono />
        </div>
        <div>
          <SimLabel>Destination</SimLabel>
          <SimInput id="tn-dest" state={state} placeholder="localhost" mono />
        </div>
        <div>
          <SimLabel>Dest. port</SimLabel>
          <SimInput id="tn-dport" state={state} placeholder="80" mono />
        </div>
      </div>
      <p className="text-[11px] text-sim-muted">The tunnel is created stopped. Starting it is a separate step.</p>
    </SimDialog>
  );
}

/* ---------- Keys + SSH agent ---------- */
export function MockKeys({ state }: SimProps) {
  const keys = [...demoKeys];
  if (state.flags.keyCreated) keys.unshift({ id: "ed-new", name: state.fields["k-name"] || "new-key", type: "Ed25519", fingerprint: "SHA256:N3wEXAMPLEonly00000000000000000000000000fp0", used: 0 });
  const agentOn = Boolean(state.flags.agentOn);
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-tl-xl bg-sim-base">
      <PageHeader title="Keys" sub="Generate and import keys; the built-in agent serves them to ssh and git.">
        <SimButton id="key-generate" primary className={hl(state, "key-generate")}>
          <Plus size={12} /> Generate key
        </SimButton>
      </PageHeader>
      <div className="grid min-h-0 flex-1 grid-cols-[1.3fr_1fr] gap-4 p-5">
        <div className="space-y-2">
          {keys.map((k) => (
            <div key={k.id} {...demo(`key-row-${k.id}`)} className={cn("flex items-center gap-3 rounded-lg border bg-sim-panel px-3 py-2.5", state.selected.key === k.id ? "border-sim-solid" : "border-sim-line")}>
              <KeyRound size={16} className="text-sim-accent" />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 text-[12.5px] font-medium text-sim-fg">
                  {k.name} <SimBadge>{k.type}</SimBadge>
                </div>
                <div className="truncate font-mono text-[10.5px] text-sim-muted">{k.fingerprint}</div>
              </div>
              {agentOn && state.flags[`offer-${k.id}`] && <SimBadge tone="success">in agent</SimBadge>}
              <span className="text-[11px] text-sim-muted">{k.used} hosts</span>
            </div>
          ))}
        </div>
        <div className="space-y-3">
          <div className="rounded-xl border border-sim-line bg-sim-panel p-4">
            <div className="flex items-center gap-2 text-[13px] font-semibold text-sim-fg">
              <Shield size={14} className="text-sim-accent" /> SSH agent
              <span {...demo("agent-toggle")} className={cn("ml-auto rounded-md px-2.5 py-1 text-[11px] font-medium", agentOn ? "bg-sim-success/15 text-sim-success" : "bg-sim-solid text-white", hl(state, "agent-toggle"))}>
                {agentOn ? "On" : "Turn on"}
              </span>
            </div>
            <p className="mt-2 text-[11.5px] leading-relaxed text-sim-muted">Keys are offered only after you switch them on, one by one. No private key file is ever written to disk.</p>
            {agentOn && (
              <div {...demo("agent-sock")} className="mt-3 flex items-center gap-2 rounded-md border border-sim-line bg-sim-term px-2.5 py-2 font-mono text-[10.5px] text-sim-fg">
                <span className="min-w-0 flex-1 truncate">export SSH_AUTH_SOCK=/run/user/1000/sshvault-agent.sock</span>
                <Copy size={12} className="shrink-0 text-sim-muted" />
              </div>
            )}
          </div>
          {agentOn && (
            <div className="rounded-xl border border-sim-line bg-sim-panel p-4 text-[12px]">
              <div className="mb-2 font-medium text-sim-fg">Offer to the agent</div>
              {["Ask every time", "Allow while unlocked"].map((m) => (
                <div key={m} {...demo(`offer-mode-${m.split(" ")[0]}`)} className={cn("mb-1.5 flex items-center gap-2 rounded-md border px-2.5 py-1.5", state.selected.offerMode === m ? "border-sim-solid bg-sim-solid/10 text-sim-fg" : "border-sim-line text-sim-muted")}>
                  {state.selected.offerMode === m && <Check size={12} className="text-sim-accent" />} {m}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function KeyDialog({ state }: SimProps) {
  return (
    <SimDialog
      title="Generate key"
      width={440}
      footer={
        <>
          <SimButton>Cancel</SimButton>
          <SimButton id="key-save" primary className={hl(state, "key-save")}>
            Generate
          </SimButton>
        </>
      }
    >
      <div>
        <SimLabel>Name</SimLabel>
        <SimInput id="k-name" state={state} placeholder="demo-laptop" />
      </div>
      <div>
        <SimLabel>Type</SimLabel>
        <div className="flex gap-1.5">
          {["Ed25519", "ECDSA", "RSA"].map((t, i) => (
            <span key={t} className={cn("flex-1 rounded-md border px-2.5 py-1.5 text-center text-[12px]", i === 0 ? "border-sim-solid bg-sim-solid/15 text-sim-fg" : "border-sim-line text-sim-muted")}>
              {t}
            </span>
          ))}
        </div>
      </div>
      <p className="text-[11px] text-sim-muted">The public half is shown after saving, ready for authorized_keys. The private key stays encrypted in the vault.</p>
    </SimDialog>
  );
}

/* ---------- Snippets + run on several hosts ---------- */
export function MockSnippets({ state }: SimProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-tl-xl bg-sim-base">
      <PageHeader title="Snippets" sub="Saved commands with {{variables}}. Run them in a terminal or on many hosts.">
        <SimButton primary>
          <Plus size={12} /> New snippet
        </SimButton>
      </PageHeader>
      <div className="space-y-2 p-5">
        {demoSnippets.map((s) => (
          <div key={s.id} {...demo(`snippet-${s.id}`)} className={cn("flex items-center gap-3 rounded-lg border bg-sim-panel px-3 py-2.5", state.selected.snippet === s.id ? "border-sim-solid" : "border-sim-line", hl(state, `snippet-${s.id}`))}>
            <Terminal size={15} className="text-sim-accent" />
            <div className="min-w-0 flex-1">
              <div className="text-[12.5px] font-medium text-sim-fg">{s.name}</div>
              <div className="truncate font-mono text-[11px] text-sim-muted">{s.command}</div>
            </div>
            {s.tags.map((t) => (
              <SimBadge key={t}>{t}</SimBadge>
            ))}
            {state.selected.snippet === s.id && (
              <>
                <SimButton>
                  <Play size={11} /> Run
                </SimButton>
                <SimButton id="snippet-run-hosts" primary className={hl(state, "snippet-run-hosts")}>
                  Run on several hosts
                </SimButton>
              </>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

const RUN_HOSTS = ["prod-api", "web-01", "db"];

export function RunHostsDialog({ state }: SimProps) {
  const done = Boolean(state.flags.runDone);
  const results: Record<string, string> = {
    "prod-api": " 14:20:01 up 41 days,  3:12,  0 users,  load average: 0.42, 0.38, 0.31",
    "web-01": " 14:20:01 up 12 days, 22:07,  0 users,  load average: 0.11, 0.09, 0.08",
    db: " 14:20:02 up 88 days,  6:45,  0 users,  load average: 1.04, 0.97, 0.90",
  };
  return (
    <SimDialog
      title="Run on several hosts"
      width={640}
      footer={
        done ? (
          <SimButton>Close</SimButton>
        ) : (
          <>
            <SimButton>Cancel</SimButton>
            <SimButton id="run-go" primary className={hl(state, "run-go")}>
              Run on {RUN_HOSTS.filter((h) => state.flags[`pick-${h}`]).length} hosts
            </SimButton>
          </>
        )
      }
    >
      <code className="block rounded-md border border-sim-line bg-sim-term px-3 py-2 font-mono text-[12px] text-sim-fg">$ uptime</code>
      <div className="space-y-1.5">
        {RUN_HOSTS.map((id) => {
          const h = demoHosts.find((x) => x.id === id)!;
          const picked = Boolean(state.flags[`pick-${id}`]);
          return (
            <div key={id} {...demo(`pick-${id}`)} className="rounded-lg border border-sim-line bg-sim-base px-3 py-2 text-[12px]">
              <div className="flex items-center gap-2">
                <span className={cn("flex h-4 w-4 items-center justify-center rounded border", picked ? "border-sim-solid bg-sim-solid text-white" : "border-sim-line")}>{picked && <Check size={10} />}</span>
                <span className="font-medium text-sim-fg">{h.name}</span>
                <span className="font-mono text-[11px] text-sim-muted">{h.hostname}</span>
                {done && picked && (
                  <span className="ml-auto flex items-center gap-2 text-[11px]">
                    <span className="text-sim-success">exit 0</span>
                    <span className="text-sim-muted">0.{3 + RUN_HOSTS.indexOf(id)}s</span>
                  </span>
                )}
              </div>
              {done && picked && <div className="mt-1.5 whitespace-pre truncate font-mono text-[11px] text-sim-muted">{results[id]}</div>}
            </div>
          );
        })}
      </div>
    </SimDialog>
  );
}

export { ArrowLeftRight };
