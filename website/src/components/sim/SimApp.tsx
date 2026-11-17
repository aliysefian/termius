import type { DemoState } from "@/engine/types";
import { SimRail, SimStatusBar, SimTopBar } from "./chrome";
import { ConnectingDialog, HostFormDialog, HostKeyDialog, MockHostManager } from "./MockHostManager";
import { KeyDialog, MockKeys, MockSnippets, MockTunnels, RunHostsDialog, TunnelDialog } from "./MockMisc";
import { MockSFTPExplorer } from "./MockSFTPExplorer";
import { MockTerminalView } from "./MockTerminalView";
import { MockTmuxManager } from "./MockTmuxManager";
import { SimToast } from "./primitives";

/** Logical size of the simulated window. The stage scales this to whatever width it is given. */
export const STAGE_W = 1100;
export const STAGE_H = 640;

function View({ state }: { state: DemoState }) {
  switch (state.view) {
    case "terminal":
      return <MockTerminalView state={state} />;
    case "sftp":
      return <MockSFTPExplorer state={state} />;
    case "tmux":
      return <MockTmuxManager state={state} />;
    case "tunnels":
      return <MockTunnels state={state} />;
    case "keys":
      return <MockKeys state={state} />;
    case "snippets":
      return <MockSnippets state={state} />;
    default:
      return <MockHostManager state={state} />;
  }
}

function Panel({ state }: { state: DemoState }) {
  switch (state.panel) {
    case "host-form":
      return <HostFormDialog state={state} />;
    case "connecting":
      return <ConnectingDialog state={state} />;
    case "hostkey":
      return <HostKeyDialog state={state} />;
    case "tunnel-form":
      return <TunnelDialog state={state} />;
    case "key-form":
      return <KeyDialog state={state} />;
    case "run-hosts":
      return <RunHostsDialog state={state} />;
    default:
      return null;
  }
}

/** The whole simulated SSHVault window, rendered from a DemoState. Pure: same state, same pixels. */
export function SimApp({ state }: { state: DemoState }) {
  return (
    <div aria-hidden="true" className="relative flex flex-col overflow-hidden bg-sim-base font-sans text-sim-fg" style={{ width: STAGE_W, height: STAGE_H }}>
      <SimTopBar state={state} />
      <div className="flex min-h-0 flex-1">
        <SimRail state={state} />
        <main className="flex min-w-0 flex-1 flex-col">
          <View state={state} />
        </main>
      </div>
      <SimStatusBar state={state} />
      <Panel state={state} />
      <SimToast state={state} />
    </div>
  );
}
