// The link between the app's keyboard shortcuts and the terminal pane showing
// a suggestion or popup. A shortcut asks; the pane answers whether it used the key. A
// key nobody used goes on to the shell unchanged.

export interface InlineControls {
  /** Take the whole suggestion. Returns false when there is none, so the key is left alone. */
  accept(): boolean;
  /** Take one word of it. */
  acceptWord(): boolean;
  /** Hide it until the line changes. Closes the popup first when that is open. */
  dismiss(): boolean;
  /** Open the popup for the line as typed. Returns false when it can't be opened here, so the key is left alone. */
  openMenu(): boolean;
  /** A navigation key while the popup is open. Returns false when it isn't, so the key goes to the shell. */
  menuKey(key: "up" | "down" | "choose"): boolean;
}

let current: InlineControls | null = null;

export const completionBridge = {
  /** The pane that has the keyboard (and has smart completion on); only one at a time. */
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
  openMenu: () => current?.openMenu() ?? false,
  menuKey: (key: "up" | "down" | "choose") => current?.menuKey(key) ?? false,
};
