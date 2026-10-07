// Wake-on-LAN addresses: the same spellings the backend accepts (src-tauri/src/wol.rs), so the form can say
// what is wrong before anything is sent.

/** `00:1A:2B:3C:4D:5E` for any of the usual spellings, or null when it isn't a MAC address. */
export function normalizeMac(text: string): string | null {
  const t = text.trim();
  const digits = t.replace(/[:.-]/g, "");
  if (!/^[0-9a-fA-F]{12}$/.test(digits)) return null;
  const count = (c: string) => t.split(c).length - 1;
  const shape = [count(":"), count("-"), count(".")].join(",");
  if (!["0,0,0", "5,0,0", "0,5,0", "0,0,2"].includes(shape)) return null;
  return digits.toUpperCase().match(/../g)!.join(":");
}

/** A broadcast address as the backend takes it: empty (the whole network), an address, or address:port. */
export function validBroadcast(text: string): boolean {
  const t = text.trim();
  if (t === "") return true;
  return /^(\d{1,3}\.){3}\d{1,3}(:\d{1,5})?$/.test(t) && t.split(":")[0].split(".").every((n) => Number(n) <= 255);
}
