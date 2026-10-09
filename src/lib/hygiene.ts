// Pure checks for the Security review page: weak or ageing keys, and
// certificates nearing (or past) their expiry. Kept free of the vault
// store so the rules themselves are unit-testable; the page wires the
// results to "fix" actions (open the key, go to Settings, …).
import { parseOpenSshCertificate } from "./sshcert";
import { parseSshPublicKey } from "./sshkeyinfo";
import type { Host, SshKey, Uuid, VaultRecord } from "./types";

export type Severity = "danger" | "warning";

export interface Finding {
  /** Stable id, so the UI can key a list and a "fix" action to one finding. */
  id: string;
  severity: Severity;
  subjectId: Uuid;
  subjectLabel: string;
  message: string;
}

const DAY_MS = 24 * 60 * 60 * 1000;
export const CERT_WARN_DAYS = 14;
export const KEY_MAX_AGE_YEARS = 5;

export function checkKeyHygiene(keys: VaultRecord<SshKey>[], now = Date.now()): Finding[] {
  const findings: Finding[] = [];
  for (const rec of keys) {
    const k = rec.data;
    if (!k) continue;

    const info = parseSshPublicKey(k.public_key);
    if (info?.algorithm === "ssh-rsa" && info.bits != null && info.bits < 3072) {
      findings.push({ id: `${rec.id}:weak-rsa`, severity: "warning", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: RSA-${info.bits} is below the 3072-bit minimum generally recommended today.` });
    } else if (info?.algorithm === "ssh-dss") {
      findings.push({ id: `${rec.id}:dsa`, severity: "warning", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: DSA keys are deprecated and no longer accepted by most servers.` });
    }

    const ageYears = (now - k.created_at) / (DAY_MS * 365);
    if (ageYears >= KEY_MAX_AGE_YEARS) {
      findings.push({ id: `${rec.id}:age`, severity: "warning", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: created over ${KEY_MAX_AGE_YEARS} years ago. Consider rotating it.` });
    }

    if (k.certificate) {
      const cert = parseOpenSshCertificate(k.certificate);
      if (cert?.validBefore != null) {
        const msLeft = cert.validBefore * 1000 - now;
        if (msLeft < 0) {
          // Whole days since it expired: 3.5 days is "3 days ago" (flooring the negative number made it 4).
          const daysAgo = Math.floor(-msLeft / DAY_MS);
          const when = daysAgo === 0 ? "today" : `${daysAgo} day${daysAgo === 1 ? "" : "s"} ago`;
          findings.push({ id: `${rec.id}:cert-expired`, severity: "danger", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: certificate expired ${when}.` });
        } else {
          const daysLeft = Math.floor(msLeft / DAY_MS);
          if (daysLeft <= CERT_WARN_DAYS) {
            const when = daysLeft === 0 ? "today" : `in ${daysLeft} day${daysLeft === 1 ? "" : "s"}`;
            findings.push({ id: `${rec.id}:cert-expiring`, severity: "warning", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: certificate expires ${when}.` });
          }
        }
      }
    }
  }
  return findings;
}

/**
 * Could this host log in with an SSH key instead of a password? SSH hosts (Mosh too, it logs in over SSH) and VNC
 * reached through SSH can; Remote Desktop, FTP, Telnet and command hosts can't, so suggesting a key there is wrong.
 */
export function canUseSshKey(host: Host): boolean {
  switch (host.protocol ?? "") {
    case "":
    case "ssh":
      return true;
    case "vnc":
      return host.vnc?.ssh_tunnel ?? true;
    default:
      return false;
  }
}
