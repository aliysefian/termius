// Send snippets to open terminals, filling in {{variables}} per pane.
import { writeToPane } from "$lib/terminalio";
import { promptedVariables, render, type HostContext } from "$lib/snippetvars";
import { adhocLabel, ui, type PaneTarget, type SnippetRunOpts } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, type Uuid } from "$lib/types";

/** Built-in variable values for a saved host. */
export function hostContextFor(hostId: Uuid): HostContext {
  const h = vaultStore.hostById.get(hostId)?.data;
  const identity = h?.identity_id ? vaultStore.identityById.get(h.identity_id)?.data : undefined;
  return {
    host: h?.label ?? "",
    hostname: h?.hostname ?? "",
    port: h?.port ?? 22,
    user: identity?.username ?? "",
  };
}

function contextFor(target: PaneTarget): HostContext {
  if (target.kind === "host") return hostContextFor(target.hostId);
  if (target.kind === "local") return { host: "local", hostname: "localhost", port: 0, user: "" };
  if (target.kind === "telnet") return { host: target.host, hostname: target.host, port: target.port, user: "" };
  if (target.kind === "serial") return { host: target.config.path, hostname: target.config.path, port: 0, user: "" };
  const a = target.adhoc;
  return { host: adhocLabel(a), hostname: a.hostname, port: a.port, user: a.username };
}

/**
 * Send a snippet to terminals. `execute` appends Enter; `scope: "tab"`
 * broadcasts to every pane in the active tab. If the snippet has variables
 * that need values, a dialog asks for them first and calls back with them.
 */
export async function runSnippet(command: string, opts: SnippetRunOpts, values?: Record<string, string>) {
  const tab = ui.activeTab;
  if (!tab) {
    ui.notify("error", "Open a terminal first, then run the snippet.");
    return;
  }
  const names = promptedVariables(command);
  if (names.length && !values) {
    ui.modal = { kind: "snippet-vars", command, names, opts };
    return;
  }
  const panes = opts.scope === "tab" ? tab.panes : tab.panes.filter((p) => p.id === tab.activePaneId);
  const results = await Promise.allSettled(
    panes.map((p) => {
      // Terminals expect CR for Enter; normalise multi-line snippets.
      let text = render(command, contextFor(p.target), values ?? {}).replace(/\r?\n/g, "\r");
      if (opts.execute && !text.endsWith("\r")) text += "\r";
      if (!opts.execute) text = text.replace(/\r$/, "");
      return writeToPane(p, text);
    }),
  );
  const failed = results.filter((r) => r.status === "rejected") as PromiseRejectedResult[];
  if (failed.length === panes.length) {
    ui.notify("error", `Snippet not sent: ${errorMessage(failed[0].reason)}`);
  } else if (failed.length) {
    ui.notify("info", `Sent to ${panes.length - failed.length} of ${panes.length} panes.`);
  }
}
