// How a container log line reads. The runtime merges stdout and stderr into one stream, so the two can't be told
// apart; a line that says it is an error or a warning is coloured as one, which is all that can honestly be done.

/** "error" for a line with an error word, "warn" for a warning word, else "info". */
export function lineTone(line: string): "error" | "warn" | "info" {
  if (/\b(error|fatal|panic|exception|traceback|critical|failed|segfault)\b|\blevel=(error|fatal)\b|\[(error|fatal|crit)\]/i.test(line)) return "error";
  if (/\b(warn|warning|deprecated)\b|\blevel=warn\b|\[warn(ing)?\]/i.test(line)) return "warn";
  return "info";
}
