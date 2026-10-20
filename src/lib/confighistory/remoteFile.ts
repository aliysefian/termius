// Reading and writing one remote file for the Time-Machine's rollback: scripts for the cockpit's sudo-capable
// connection, what they print, and the command that makes a service take the restored file.
import { shq } from "$lib/ops/quote";

/** `~/x` is expanded on the host. The path is always an argument, never part of the script text. */
const EXPAND = 'case "$1" in "~/"*) f=$HOME/${1#"~/"};; *) f=$1;; esac';

export const validRemotePath = (p: string) => p.length > 0 && p.length <= 4096 && (p.startsWith("/") || p.startsWith("~/")) && !/[\0\r\n]/.test(p);

/** The script as one command: `sh -c '…' sh <path>`, the path being `$1`. */
const command = (script: string, path: string) => `sh -c ${shq(script)} sh ${shq(path)}`;

/** Prints `@@OK <size> <mode> <owner>` or `@@MISSING`, then the file. */
export function readScript(path: string): string {
  if (!validRemotePath(path)) throw new Error("That isn't a remote path.");
  return command(
    [
      EXPAND,
      '[ -f "$f" ] || { echo @@MISSING; exit 0; }',
      "m=$(stat -c '%s %a %U:%G' \"$f\" 2>/dev/null || stat -f '%z %Lp %Su:%Sg' \"$f\" 2>/dev/null) || m='0 - -'",
      'echo "@@OK $m"',
      'cat "$f"',
    ].join("; "),
    path,
  );
}

export interface LiveFile {
  exists: boolean;
  text: string;
  /** `0644` or empty. */
  permissions: string;
  /** `root:root` or empty. */
  owner: string;
}

export function parseRead(out: string): LiveFile {
  const nl = out.indexOf("\n");
  const head = nl < 0 ? out : out.slice(0, nl);
  if (head.startsWith("@@MISSING") || !head.startsWith("@@OK ")) return { exists: false, text: "", permissions: "", owner: "" };
  const f = head.slice(5).trim().split(/\s+/);
  const mode = /^\d+$/.test(f[1] ?? "") ? f[1].padStart(4, "0") : "";
  return { exists: true, text: nl < 0 ? "" : out.slice(nl + 1), permissions: mode, owner: f[2] && f[2] !== "-" ? f[2] : "" };
}

const b64 = (text: string) => {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
};

export const validMode = (m: string) => /^[0-7]{3,4}$/.test(m);
export const validOwner = (o: string) => /^[A-Za-z0-9._-]+:[A-Za-z0-9._-]+$/.test(o);

/**
 * Put `text` in the file. The text goes to a temporary file beside it first and is copied over the old one (not moved
 * over it, which would change its inode, ACLs and SELinux label), then the mode and owner it had are set again.
 * Runs as root where the folder needs it; the sudo wrapper is the caller's.
 */
export function writeScript(path: string, text: string, permissions: string, owner: string): string {
  if (!validRemotePath(path)) throw new Error("That isn't a remote path.");
  const perm = validMode(permissions) ? `chmod ${permissions} "$f" || exit 6; ` : "";
  const own = validOwner(owner) ? `chown ${owner} "$f" || exit 7; ` : "";
  return command(
    [
      EXPAND,
      'd=$(dirname "$f")',
      '[ -d "$d" ] || { echo "no such folder: $d" >&2; exit 2; }',
      'tmp=$(mktemp "$d/.sshvault-restore.XXXXXX") || exit 3',
      `printf %s ${shq(b64(text))} | base64 -d > "$tmp" || { rm -f "$tmp"; exit 4; }`,
      'cat "$tmp" > "$f"; rc=$?; rm -f "$tmp"; [ $rc -eq 0 ] || exit 5',
      `${perm}${own}true`,
    ].join("; "),
    path,
  );
}

/** Which command makes the running service read the file again, for the files people most often break. */
export function reloadCommand(path: string, asRoot: boolean): string | null {
  const sudo = asRoot ? "" : "sudo ";
  const rules: [RegExp, string][] = [
    [/^\/etc\/nginx\//, `${sudo}nginx -t && ${sudo}systemctl reload nginx`],
    [/^\/etc\/ssh\/sshd_config/, `${sudo}sshd -t && (${sudo}systemctl reload ssh || ${sudo}systemctl reload sshd)`],
    [/^\/etc\/apache2\//, `${sudo}apachectl configtest && ${sudo}systemctl reload apache2`],
    [/^\/etc\/httpd\//, `${sudo}apachectl configtest && ${sudo}systemctl reload httpd`],
    [/^\/etc\/haproxy\//, `${sudo}haproxy -c -f /etc/haproxy/haproxy.cfg && ${sudo}systemctl reload haproxy`],
    [/^\/etc\/fail2ban\//, `${sudo}fail2ban-client reload`],
    [/^\/(etc|lib|usr\/lib)\/systemd\/system\//, `${sudo}systemctl daemon-reload`],
  ];
  const hit = rules.find(([re]) => re.test(path));
  return hit ? hit[1] : null;
}
