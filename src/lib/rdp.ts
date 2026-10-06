// Remote desktop (RDP) helpers: reading the backend's byte messages, and
// turning what the browser reports (pointer position, wheel) into what RDP wants.

export type RdpMessage =
  | { kind: "frame"; x: number; y: number; width: number; height: number; rgba: Uint8ClampedArray<ArrayBuffer> }
  | { kind: "size"; width: number; height: number }
  | { kind: "cursor-default" }
  | { kind: "cursor-hidden" }
  | { kind: "cursor"; hotX: number; hotY: number; width: number; height: number; rgba: Uint8ClampedArray<ArrayBuffer> }
  | { kind: "clipboard"; text: string }
  | { kind: "ended"; error: string | null };

const TAG = { frame: 0, size: 1, cursorDefault: 2, cursorHidden: 3, cursor: 4, clipboard: 5, ended: 6 } as const;

/** A message from the backend: a tag byte, 16-bit little-endian numbers, then bytes. `null` for anything malformed. */
export function decodeMessage(bytes: Uint8Array): RdpMessage | null {
  if (bytes.length === 0) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u16 = (i: number) => view.getUint16(1 + i * 2, true);
  const pixels = (offset: number, w: number, h: number) => {
    const length = w * h * 4;
    if (w === 0 || h === 0 || bytes.length !== offset + length) return null;
    // A copy: the message's buffer is not ours to keep.
    return new Uint8ClampedArray(bytes.slice(offset, offset + length).buffer);
  };
  const text = (offset: number) => new TextDecoder().decode(bytes.subarray(offset));
  switch (bytes[0]) {
    case TAG.frame: {
      if (bytes.length < 9) return null;
      const [x, y, width, height] = [u16(0), u16(1), u16(2), u16(3)];
      const rgba = pixels(9, width, height);
      return rgba ? { kind: "frame", x, y, width, height, rgba } : null;
    }
    case TAG.size:
      return bytes.length === 5 && u16(0) > 0 && u16(1) > 0 ? { kind: "size", width: u16(0), height: u16(1) } : null;
    case TAG.cursorDefault:
      return { kind: "cursor-default" };
    case TAG.cursorHidden:
      return { kind: "cursor-hidden" };
    case TAG.cursor: {
      if (bytes.length < 9) return null;
      const [hotX, hotY, width, height] = [u16(0), u16(1), u16(2), u16(3)];
      const rgba = pixels(9, width, height);
      return rgba ? { kind: "cursor", hotX, hotY, width, height, rgba } : null;
    }
    case TAG.clipboard:
      return { kind: "clipboard", text: text(1) };
    case TAG.ended:
      return { kind: "ended", error: bytes.length > 1 ? text(1) : null };
    default:
      return null;
  }
}

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Where a pointer at `clientX, clientY` is on the remote screen, kept inside it. */
export function remotePoint(clientX: number, clientY: number, box: Box, remote: { width: number; height: number }): { x: number; y: number } {
  if (box.width <= 0 || box.height <= 0) return { x: 0, y: 0 };
  const x = Math.floor(((clientX - box.left) / box.width) * remote.width);
  const y = Math.floor(((clientY - box.top) / box.height) * remote.height);
  return { x: Math.min(Math.max(x, 0), remote.width - 1), y: Math.min(Math.max(y, 0), remote.height - 1) };
}

/** RDP counts a wheel notch as 120, positive away from the person; browsers report down as positive. */
export function wheelUnits(delta: number, mode: number): number {
  if (delta === 0 || !Number.isFinite(delta)) return 0;
  const perUnit = mode === 1 ? 40 : mode === 2 ? 360 : 1.2; // lines, pages, pixels
  const units = Math.round(-delta * perUnit);
  // A small scroll still moves one step.
  const sized = units === 0 ? (delta > 0 ? -1 : 1) : units;
  return Math.max(-32768, Math.min(32767, sized));
}

/** Window sizes to offer; 0 × 0 means "fit the tab". */
export const RDP_SIZES: { label: string; width: number; height: number }[] = [
  { label: "Fit the tab", width: 0, height: 0 },
  { label: "1280 × 720", width: 1280, height: 720 },
  { label: "1366 × 768", width: 1366, height: 768 },
  { label: "1600 × 900", width: 1600, height: 900 },
  { label: "1920 × 1080", width: 1920, height: 1080 },
  { label: "2560 × 1440", width: 2560, height: 1440 },
];

/** A size the server accepts: even sides within its limits. */
export function usableSize(width: number, height: number): { width: number; height: number } {
  const fit = (v: number) => Math.min(Math.max(Math.floor(v), 200), 8192) & ~1;
  return { width: fit(width), height: fit(height) };
}

/** What to show for a certificate fingerprint: pairs of digits. */
export function formatFingerprint(hex: string): string {
  return (hex.match(/.{1,2}/g) ?? []).join(":");
}

/** The same, in lines of eight pairs, which fit a dialog. */
export function fingerprintLines(hex: string): string {
  const pairs = hex.match(/.{1,2}/g) ?? [];
  const lines: string[] = [];
  for (let i = 0; i < pairs.length; i += 8) lines.push(pairs.slice(i, i + 8).join(":"));
  return lines.join("\n");
}
