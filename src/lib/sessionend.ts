// How a terminal session ended, and what to do about it. The backend says `ended` when the other end finished the
// session itself (the shell exited, or the server closed the channel, with or without an exit status) and leaves it
// off when the link died under it. Only a dead link is worth reconnecting.

/** The `disconnected` notice's fields that decide this. */
export interface SessionEnd {
  code: number | null;
  ended?: boolean;
}

export const MAX_RECONNECTS = 3;

/** A connection that stayed up this long counts as having worked, so the next drop starts a fresh set of tries. */
export const STABLE_MS = 30_000;

/** The connection died; the shell did not end. A status code or an `ended` flag means the other end finished. */
export function wasDropped(end: SessionEnd): boolean {
  return end.code == null && !end.ended;
}

/** The attempts already used once the connection has dropped: forgotten only after a long enough run, so a server
 *  that accepts and then cuts every login is given up on after MAX_RECONNECTS instead of being retried for ever. */
export function attemptsAfterDrop(used: number, upMs: number): number {
  return upMs >= STABLE_MS ? 0 : used;
}
