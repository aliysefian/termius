// Operations (logs, services, alerts) run small scripts on a host. Whatever a person typed reaches the
// script only as a single-quoted word, so it is data, never part of the command.

/** One word for a POSIX shell: single quotes, with a quote inside written as '\''. */
export function shq(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}
