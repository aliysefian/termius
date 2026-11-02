// Control characters in names that a server chose. A shell's line editor acts on them (Ctrl-U erases what was typed,
// Ctrl-A and Ctrl-E move the cursor, ESC starts a sequence), so a folder called `x<Ctrl-U>touch y #` turns a typed
// `cd 'x...'` into a different command. Quoting stops the shell parser, not the line editor.

// C0, DEL and C1 (a terminal treats C1 as controls too).
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
// eslint-disable-next-line no-control-regex
const CONTROL_ALL = /[\u0000-\u001f\u007f-\u009f]/g;

/** Whether `s` holds a control character. Such a name is never typed into a terminal. */
export function hasControlChars(s: string): boolean {
  return CONTROL.test(s);
}

/** `s` as text for the screen: each control character shown as `\x15`, so a name can't hide what it is. */
export function showControlChars(s: string): string {
  return s.replace(CONTROL_ALL, (c) => `\\x${c.charCodeAt(0).toString(16).padStart(2, "0")}`);
}
