// The sample the Settings preview shows. Redrawn in one write that starts by clearing the screen in-band:
// `term.clear()` runs at once while earlier writes are still queued, and keeps the prompt line, so the preview showed
// its sample twice ("user@host:~/sshvault$ user@host:~/sshvault$ ls").

const PROMPT = "\x1b[38;2;120;200;120muser@host\x1b[0m:\x1b[38;2;120;160;255m~/sshvault\x1b[0m$ ";

export const PREVIEW_SAMPLE =
  `${PROMPT}ls\r\n` +
  "\x1b[1;34msrc\x1b[0m  \x1b[1;32mbuild.sh\x1b[0m  \x1b[1;36mREADME.md\x1b[0m  \x1b[1;31merror.log\x1b[0m\r\n" +
  `${PROMPT}git diff\r\n` +
  "\x1b[32m+ a line that was added\x1b[0m\r\n" +
  "\x1b[31m- a line that was removed\x1b[0m\r\n" +
  "\x1b[1mBold text\x1b[0m stays readable at any contrast setting.\r\n" +
  PROMPT;

/** Home, erase the screen and the scrollback, then the sample: ordered with any write still waiting in the queue. */
export function redrawPreview(term: { write(data: string, callback?: () => void): void }, done?: () => void) {
  term.write(`\x1b[H\x1b[2J\x1b[3J${PREVIEW_SAMPLE}`, done);
}
