import { ArrowLeft, ArrowRight, ChevronRight, Check, Eye, File, Folder, Laptop, Server } from "lucide-react";
import { cn } from "@/lib/utils";
import { localFiles, remoteReleases, remoteRoot, type DemoFile } from "@/data/demo";
import { demo, hl, SimBadge, SimButton, type SimProps } from "./primitives";
import type { DemoState } from "@/engine/types";

export function remoteListing(state: DemoState): { path: string; files: DemoFile[] } {
  const inReleases = state.selected.remoteDir === "releases";
  const base = inReleases ? remoteReleases : remoteRoot;
  const files = [...base];
  if (state.flags.uploaded && !inReleases) files.push({ name: "release-notes.md", kind: "file", size: "4.2 KB", modified: "Nov 16 14:20", mode: "-rw-r--r--" });
  return { path: inReleases ? "/srv/app/releases" : "/srv/app", files };
}

function FileRow({ file, id, selected, state }: { file: DemoFile; id: string; selected: boolean; state: DemoState }) {
  const hovered = state.hover === id;
  return (
    <div {...demo(id)} className={cn("grid grid-cols-[1fr_72px_96px] items-center gap-2 px-3 py-[5px] text-[12px]", selected ? "bg-sim-solid/20 text-sim-fg" : hovered ? "bg-sim-hover" : "text-sim-fg", hl(state, id))}>
      <span className="flex min-w-0 items-center gap-2">
        {file.kind === "dir" ? <Folder size={14} className="shrink-0 text-sim-accent" /> : <File size={14} className="shrink-0 text-sim-muted" />}
        <span className="truncate">{file.name}</span>
      </span>
      <span className="text-right text-[11px] text-sim-muted">{file.size ?? ""}</span>
      <span className="text-right text-[11px] text-sim-muted">{file.modified}</span>
    </div>
  );
}

function Pane({ title, icon, path, children, id }: { title: string; icon: React.ReactNode; path: string; children: React.ReactNode; id: string }) {
  return (
    <section {...demo(id)} className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-sim-line bg-sim-panel/50">
      <div className="flex items-center gap-2 border-b border-sim-line px-3 py-2 text-[12px] font-medium text-sim-fg">
        {icon}
        {title}
        <Eye size={13} className="ml-auto text-sim-muted" />
      </div>
      <div className="flex items-center gap-1 border-b border-sim-line px-3 py-1.5 font-mono text-[11px] text-sim-muted">
        {path.split("/").filter(Boolean).map((seg, i, a) => (
          <span key={i} className="flex items-center gap-1">
            <span className={i === a.length - 1 ? "text-sim-fg" : ""}>{seg}</span>
            {i < a.length - 1 && <ChevronRight size={10} />}
          </span>
        ))}
      </div>
      <div className="grid grid-cols-[1fr_72px_96px] gap-2 border-b border-sim-line px-3 py-1 text-[10px] font-medium uppercase tracking-wide text-sim-muted">
        <span>Name</span>
        <span className="text-right">Size</span>
        <span className="text-right">Modified</span>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
    </section>
  );
}

const MENU = ["Open", "Download", "Copy path", "Rename", "Permissions…", "Delete"];

export function MockSFTPExplorer({ state }: SimProps) {
  const connected = Boolean(state.flags.sftpConnected);
  const remote = remoteListing(state);
  const up = state.progress.upload;
  const transferring = up !== undefined && up > 0;
  return (
    <div className="relative flex min-h-0 flex-1 flex-col gap-2.5 rounded-tl-xl bg-sim-base p-3">
      <div className="flex min-h-0 flex-1 items-stretch gap-2">
        <Pane id="sftp-local" title="This computer" icon={<Laptop size={14} className="text-sim-muted" />} path="/home/demo/Documents">
          {localFiles.map((f) => (
            <FileRow key={f.name} file={f} id={`local-${f.name}`} selected={state.selected.local === f.name} state={state} />
          ))}
        </Pane>
        <div className="flex w-[104px] shrink-0 flex-col items-center justify-center gap-2">
          <SimButton id="sftp-upload" primary={Boolean(state.selected.local) && connected} className={cn("w-full", hl(state, "sftp-upload"))}>
            Upload <ArrowRight size={12} />
          </SimButton>
          <SimButton id="sftp-download" className="w-full">
            <ArrowLeft size={12} /> Download
          </SimButton>
        </div>
        <Pane id="sftp-remote" title={connected ? "Production API" : "Remote host"} icon={<Server size={14} className="text-sim-muted" />} path={connected ? remote.path : "/"}>
          {connected ? (
            remote.files.map((f) => <FileRow key={f.name} file={f} id={`remote-${f.name}`} selected={state.selected.remote === f.name} state={state} />)
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-[12px] text-sim-muted">
              <p>Choose a host and connect to browse its files.</p>
              <div className="flex items-center gap-2">
                <span {...demo("sftp-host-picker")} className="flex h-8 w-44 items-center justify-between rounded-md border border-sim-line bg-sim-base px-2.5 text-sim-fg">
                  {state.selected.sftpHost ?? "Choose a host"} <ChevronRight size={12} className="rotate-90" />
                </span>
                <SimButton id="sftp-connect" primary className={hl(state, "sftp-connect")}>
                  Connect
                </SimButton>
              </div>
            </div>
          )}
        </Pane>
      </div>
      <div className="h-[104px] shrink-0 rounded-lg border border-sim-line bg-sim-panel/50 p-3">
        <div className="mb-1.5 flex items-center gap-2 text-[11px] font-medium uppercase tracking-wide text-sim-muted">
          Transfers {transferring && <SimBadge tone={up >= 100 ? "success" : "accent"}>{up >= 100 ? "Done" : "Uploading"}</SimBadge>}
        </div>
        {transferring ? (
          <div className="space-y-1.5" {...demo("sftp-transfer")}>
            <div className="flex items-center gap-2 text-[12px] text-sim-fg">
              {up >= 100 ? <Check size={13} className="text-sim-success" /> : <ArrowRight size={13} className="text-sim-accent" />}
              <span className="font-medium">release-notes.md</span>
              <span className="text-sim-muted">→ /srv/app</span>
              <span className="ml-auto tabular-nums text-sim-muted">{up}%</span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-sim-base">
              <div className={cn("h-full rounded-full", up >= 100 ? "bg-sim-success" : "bg-sim-solid")} style={{ width: `${up}%` }} />
            </div>
            <div className="text-[11px] text-sim-muted">{up >= 100 ? "4.2 KB uploaded" : `${(4.2 * up / 100).toFixed(1)} of 4.2 KB`}</div>
          </div>
        ) : (
          <p className="text-[12px] text-sim-muted">Uploads and downloads appear here with progress. You can pause, resume or cancel each one.</p>
        )}
      </div>
      {state.menu === "file-menu" && (
        <div className="sim-pop absolute right-[150px] top-[132px] z-20 w-44 rounded-lg border border-sim-line bg-sim-panel p-1 text-[12px] shadow-xl shadow-black/50">
          {MENU.map((m) => (
            <div key={m} {...demo(`menu-${m.split(/[ …]/)[0]!.toLowerCase()}`)} className={cn("rounded px-2.5 py-1.5", m === "Delete" ? "text-sim-danger" : "text-sim-fg", state.hover === `menu-${m.split(/[ …]/)[0]!.toLowerCase()}` && "bg-sim-hover")}>
              {m}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
