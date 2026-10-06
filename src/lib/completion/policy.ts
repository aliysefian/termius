// Which parts of Smart completion run, for one host: the settings, narrowed by
// the host's own choice. The master switch always wins, and a host can only
// narrow or widen within it (a host set to "on" does nothing while the master
// is off).
import type { HostCompletion } from "$lib/types";

export interface CompletionPrefs {
  smartCompletion: boolean;
  acInline: boolean;
  acMenu: boolean;
  acSnippets: boolean;
  acOptions: boolean;
  acRemotePaths: boolean;
}

export interface Policy {
  /** Anything visible at all: the inline suggestion or the popup. */
  enabled: boolean;
  /** Suggestions from the commands typed before. */
  history: boolean;
  inline: boolean;
  menu: boolean;
  snippets: boolean;
  options: boolean;
  /** Asking the host for file names over an extra SSH channel. */
  remotePaths: boolean;
}

/** Nothing runs. */
export const OFF: Policy = Object.freeze({ enabled: false, history: false, inline: false, menu: false, snippets: false, options: false, remotePaths: false });

const MODES: readonly string[] = ["on", "off", "history"];

/** The host's choice, or undefined for "follow the settings" (anything unrecognised counts as that). */
export function hostMode(value: unknown): HostCompletion | undefined {
  return typeof value === "string" && MODES.includes(value) ? (value as HostCompletion) : undefined;
}

/**
 * @param environment the host's effective environment (own or inherited from its group); production
 * hosts with no choice of their own get history only, because what is typed there is the most sensitive.
 */
export function policyFor(prefs: CompletionPrefs, host?: { completion?: string } | null, environment?: string | null): Policy {
  if (!prefs.smartCompletion) return OFF;
  const mode = hostMode(host?.completion) ?? (environment === "production" ? "history" : undefined);
  if (mode === "off") return OFF;
  const historyOnly = mode === "history";
  const p: Policy = {
    enabled: false,
    history: true,
    inline: prefs.acInline,
    menu: prefs.acMenu,
    snippets: !historyOnly && prefs.acSnippets,
    options: !historyOnly && prefs.acOptions,
    remotePaths: !historyOnly && prefs.acRemotePaths,
  };
  // With neither the inline suggestion nor the popup there is nothing to show.
  p.enabled = p.inline || p.menu;
  return p.enabled ? p : OFF;
}
