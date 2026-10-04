// Pure checks for the Security review page: weak or ageing keys, and
// certificates nearing (or past) their expiry. Kept free of the vault
// store so the rules themselves are unit-testable; the page wires the
// results to "fix" actions (open the key, go to Settings, …).
import { parseOpenSshCertificate } from "./sshcert";
import { parseSshPublicKey } from "./sshkeyinfo";
import type { SshKey, Uuid, VaultRecord } from "./types";

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
        const daysLeft = Math.floor((cert.validBefore * 1000 - now) / DAY_MS);
        if (daysLeft < 0) {
          findings.push({ id: `${rec.id}:cert-expired`, severity: "danger", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: certificate expired ${-daysLeft} day${daysLeft === -1 ? "" : "s"} ago.` });
        } else if (daysLeft <= CERT_WARN_DAYS) {
          findings.push({ id: `${rec.id}:cert-expiring`, severity: "warning", subjectId: rec.id, subjectLabel: k.name, message: `${k.name}: certificate expires in ${daysLeft} day${daysLeft === 1 ? "" : "s"}.` });
        }
      }
    }
  }
  return findings;
}
