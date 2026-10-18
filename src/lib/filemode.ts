/** POSIX mode bits as `rw-r--r--` (the lower nine; empty where unknown). */
export function modeString(m: number | null | undefined): string {
  if (m === null || m === undefined) return "";
  const bit = (mask: number, c: string) => (m & mask ? c : "-");
  return [0o400, 0o200, 0o100, 0o40, 0o20, 0o10, 0o4, 0o2, 0o1].map((b, i) => bit(b, "rwx"[i % 3])).join("");
}
