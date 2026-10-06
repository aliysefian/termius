// Asking about a server's certificate, for protocols that pin it (Remote Desktop, FTP).
import { ask } from "$lib/dialogs.svelte";
import { fingerprintLines } from "$lib/rdp";
import { isApiError, type RdpCertificate } from "$lib/types";

const text = (c: RdpCertificate) => `Subject: ${c.subject}\nIssued by: ${c.issuer}\nValid: ${c.not_before} to ${c.not_after}\nSHA-256:\n${fingerprintLines(c.fingerprint)}`;

/**
 * If `e` is "the server's certificate is new" or "has changed", ask whether to trust it and return the
 * fingerprint to connect again with; null if it is some other error or the person said no.
 */
export async function askAboutCertificate(label: string, e: unknown): Promise<string | null> {
  if (!isApiError(e)) return null;
  if (e.code.endsWith("_certificate_unknown")) {
    const c = e.details as RdpCertificate;
    const ok = await ask(
      `${label} has not been seen before. Check that this is the certificate you expect before trusting it; it is remembered for this host, and a different one later is refused until you say otherwise.\n\n${text(c)}`,
      { title: "Trust this server's certificate?", confirm: "Trust and connect" },
    );
    return ok ? c.fingerprint : null;
  }
  if (e.code.endsWith("_certificate_changed")) {
    const d = e.details as { expected: string; found: RdpCertificate };
    const ok = await ask(
      `The certificate ${label} presents is not the one trusted before. This happens when the server's certificate is renewed or the server is reinstalled, but it is also what someone intercepting the connection looks like. Nothing was sent to it.\n\nTrusted before:\n${fingerprintLines(d.expected)}\n\nNow presented:\n${text(d.found)}`,
      { title: "The server's certificate changed", confirm: "Trust the new certificate", danger: true },
    );
    return ok ? d.found.fingerprint : null;
  }
  return null;
}
