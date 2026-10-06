// The link between the app's keyboard shortcuts and the terminal pane showing
// a suggestion. A shortcut asks; the pane answers whether it used the key. A
// key nobody used goes on to the shell unchanged.

export interface InlineControls {
  /** Take the whole suggestion. Returns false when there is none, so the key is left alone. */
  accept(): boolean;
  /** Take one word of it. */
  acceptWord(): boolean;
  /** Hide it until the line changes. */
  dismiss(): boolean;
}

let current: InlineControls | null = null;

export const completionBridge = {
  /** The pane that has the keyboard; only one at a time. */
  set(c: InlineControls) {
    current = c;
  },
  /** Only clears if `c` is still the current one, so a pane that lost focus can't clear its successor. */
  clear(c: InlineControls) {
    if (current === c) current = null;
  },
  accept: () => current?.accept() ?? false,
  acceptWord: () => current?.acceptWord() ?? false,
  dismiss: () => current?.dismiss() ?? false,
};
