/**
 * Declarative tutorial model. A tutorial is a list of steps; the engine compiles them to a timeline
 * and derives the whole UI state from the playhead time, so seeking and replaying are deterministic.
 */

export type Toast = { text: string; kind: "success" | "info" | "error" };

/** Everything the simulated UI can show. The mock components render this and nothing else. */
export interface DemoState {
  /** Which page of the app is showing: hosts, terminal, sftp, tmux, tunnels, keys, snippets. */
  view: string;
  /** An open dialog or panel, if any. */
  panel: string | null;
  /** An open context menu, if any. */
  menu: string | null;
  /** Text typed into inputs, by field id. */
  fields: Record<string, string>;
  /** The selected item in each list, by list id. */
  selected: Record<string, string>;
  /** Terminal scrollback (already "printed" lines), by terminal id. */
  terminals: Record<string, string[]>;
  /** Animated numbers 0..100 (transfers, connecting), by key. */
  progress: Record<string, number>;
  /** Free toggles the scene uses (a split is open, a tab is renamed, …). */
  flags: Record<string, boolean>;
  toast: Toast | null;
  /** Target id with a highlight ring. */
  highlight: string | null;
  /** Target the cursor is hovering. */
  hover: string | null;
}

/** A partial update: the map-like fields merge key by key, everything else replaces. */
export type Patch = Partial<Omit<DemoState, "fields" | "selected" | "terminals" | "progress" | "flags">> & {
  fields?: Record<string, string>;
  selected?: Record<string, string>;
  terminals?: Record<string, string[]>;
  progress?: Record<string, number>;
  flags?: Record<string, boolean>;
};

interface StepBase {
  /** Starts a named chapter at this step (shown in the written steps and the timeline). */
  chapter?: string;
  /** Caption shown until the next caption. */
  note?: string;
}

export type TutorialStep = StepBase &
  (
    | { type: "moveCursor"; target: string; duration: number }
    | { type: "click"; target: string; duration?: number; effect?: Patch }
    | { type: "doubleClick"; target: string; duration?: number; effect?: Patch }
    | { type: "type"; target: string; text: string; duration: number; submit?: { terminal: string; prompt: string } }
    | { type: "wait"; duration: number }
    | { type: "showPanel"; panel: string | null }
    | { type: "terminalOutput"; terminal: string; lines: string[]; duration: number }
    | { type: "highlight"; target: string | null; duration?: number }
    | { type: "set"; patch: Patch }
    | { type: "toast"; text: string; kind?: Toast["kind"]; duration: number }
    | { type: "progress"; key: string; from: number; to: number; duration: number }
  );

export type StepType = TutorialStep["type"];

export interface TutorialDefinition {
  id: string;
  /** The state at time zero. */
  initial: Patch;
  /** Where the cursor starts, normalised 0..1 over the stage. */
  cursorStart?: { x: number; y: number };
  steps: TutorialStep[];
  /** Time (ms) shown as the static preview when motion is reduced. Defaults to 70% of the duration. */
  posterTime?: number;
}

export interface TimelineItem {
  index: number;
  step: TutorialStep;
  start: number;
  end: number;
  /** Cursor target before this step starts (null: still at the start position). */
  cursorFrom: string | null;
}

export interface Chapter {
  id: string;
  title: string;
  start: number;
  end: number;
  firstStep: number;
}

export interface CursorFrame {
  /** Normalised position, or null when it is not known yet. */
  x: number;
  y: number;
  pressed: boolean;
  visible: boolean;
  /** Target the cursor sits on (for hover) and the index of the latest click (for the ripple). */
  target: string | null;
  clickIndex: number;
  clickAge: number;
  clickDouble: boolean;
}
