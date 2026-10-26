// One side of the Files view: either this computer's disk or any host's files, with everything it takes to get there
// (choosing, connecting, asking for credentials, falling back to SCP) and where the pane is now. Both sides are the
// same thing, so either can be this computer or a host, and a copy between two hosts is just a copy between two panes.

import { askAboutCertificate } from "$lib/certprompt";
import { local, LOCAL_ID, remote, sftp, type FileSource } from "$lib/sftp";
import { connectionLog } from "$lib/stores/connectionlog.svelte";
import { ui } from "$lib/stores/ui.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { errorMessage, isApiError, type Uuid } from "$lib/types";

/** The value the host picker has for "this computer". */
export const THIS_COMPUTER = "local";

type Opened = { home: string; new_host_keys: { host: string; fingerprint: string }[] };

export class PaneConn {
  readonly side: "left" | "right";
  /** The id of this pane's own connection, whatever it is connected to; stable for the pane's life. */
  readonly sessionId = `sftp-${crypto.randomUUID()}`;

  /** What the picker shows: `local`, a host's id, or nothing yet. */
  choice = $state("");
  isLocal = $state(false);
  hostId = $state<Uuid | "">("");
  connectedHost = $state<Uuid | null>(null);
  connecting = $state(false);
  connectError = $state<string | null>(null);
  /** The last connect failed because the server has SFTP turned off. */
  sftpOff = $state(false);
  askCreds = $state(false);
  username = $state("");
  password = $state("");
  path = $state("");
  /** Bump to reload the folder. */
  refresh = $state(0);

  #retryCreds: { username: string; password: string } | null = null;
  #localPath = "";

  constructor(side: "left" | "right", startLocal: boolean) {
    this.side = side;
    if (startLocal) {
      this.isLocal = true;
      this.choice = THIS_COMPUTER;
      void local.home().then((h) => {
        if (!this.path) this.path = h;
      });
    }
  }

  /** The id of what this pane shows, for the backend (`local` or its session). */
  get id(): string {
    return this.isLocal ? LOCAL_ID : this.sessionId;
  }

  /** Where files can be listed and changed now, or none while nothing is connected. */
  get source(): FileSource | null {
    if (this.isLocal) return local;
    return this.connectedHost ? remote(this.sessionId) : null;
  }

  /** Whether the pane shows something (this computer, or a connected host). */
  get open(): boolean {
    return this.isLocal || this.connectedHost !== null;
  }

  get host() {
    return this.hostId ? vaultStore.hostById.get(this.hostId)?.data : undefined;
  }

  get connectedData() {
    return this.connectedHost ? vaultStore.hostById.get(this.connectedHost)?.data : undefined;
  }

  get label(): string {
    if (this.isLocal) return "This computer";
    return this.connectedData?.label ?? "";
  }

  /** The picker was changed to `value`: show this computer, or connect to that host. */
  async pick(value: string) {
    this.choice = value;
    if (!value) return;
    if (value === THIS_COMPUTER) {
      await this.useLocal();
      return;
    }
    if (!this.isLocal && this.connectedHost === value) return;
    if (this.connectedHost) await this.disconnect();
    this.isLocal = false;
    this.hostId = value;
    this.connectError = null;
    await this.connect();
  }

  /** Show this computer's disk (leaving a host's connection, which is closed). */
  async useLocal() {
    if (this.connectedHost) await this.disconnect();
    this.isLocal = true;
    this.choice = THIS_COMPUTER;
    this.hostId = "";
    this.askCreds = false;
    this.connectError = null;
    this.path = this.#localPath || (await local.home());
  }

  async connect(creds: { username: string; password: string } | null = null) {
    if (!this.hostId) return;
    const host = this.host;
    if (!creds && host && !vaultStore.effectiveIdentity(host) && !host.ftp?.anonymous) {
      this.askCreds = true;
      return;
    }
    // Leaving this computer's disk: remember where it was.
    if (this.isLocal) {
      this.#localPath = this.path;
      this.isLocal = false;
    }
    this.connecting = true;
    this.connectError = null;
    this.sftpOff = false;
    this.askCreds = false;
    const wanted = this.hostId;
    try {
      const res = await this.#openFiles(creds);
      this.connectedHost = wanted;
      this.#logId = connectionLog.start(wanted, host?.label ?? "host", "files");
      this.path = res.home;
      this.refresh++;
      for (const k of res.new_host_keys) ui.notify("info", `New host key recorded for ${k.host}: ${k.fingerprint}`);
    } catch (e) {
      this.connectError = errorMessage(e);
      this.sftpOff = isApiError(e) && e.code === "sftp_unavailable";
      // Keep what was typed so "Use SCP" can go straight on without asking again.
      if (this.sftpOff && creds) this.#retryCreds = creds;
    } finally {
      this.connecting = false;
      this.password = "";
    }
  }

  /** Open the host's files by the protocol it uses; an FTP certificate that needs a decision is asked about and retried. */
  async #openFiles(creds: { username: string; password: string } | null, accept: string | null = null): Promise<Opened> {
    const host = this.host;
    const hostId = this.hostId as Uuid;
    if (host?.protocol === "ftp") {
      try {
        const r = await sftp.openFtp(this.sessionId, hostId, creds, accept);
        return { home: r.home, new_host_keys: [] };
      } catch (e) {
        const fingerprint = await askAboutCertificate(host.label, e);
        if (fingerprint) return this.#openFiles(creds, fingerprint);
        throw e;
      }
    }
    return host?.file_protocol === "scp" ? sftp.openScp(this.sessionId, hostId, creds) : sftp.open(this.sessionId, hostId, creds);
  }

  /** Remember on the host that its files go over SCP, and connect that way. */
  async useScp() {
    const rec = this.hostId ? vaultStore.hostById.get(this.hostId) : undefined;
    if (!rec?.data) return;
    try {
      await vaultStore.saveHost(rec.id, { ...structuredClone($state.snapshot(rec.data)), file_protocol: "scp" }, rec.rev);
    } catch (e) {
      ui.notify("error", errorMessage(e));
      return;
    }
    const creds = this.#retryCreds;
    this.#retryCreds = null;
    await this.connect(creds);
  }

  /** Close the connection and leave the pane with nothing in it. */
  async disconnect() {
    this.#endLog();
    await sftp.close(this.sessionId);
    this.connectedHost = null;
    this.path = "";
  }

  /** The connection log's line for this connection, closed once when the connection is. */
  #logId: string | null = null;
  #endLog() {
    if (this.#logId) connectionLog.end(this.#logId, null, "closed");
    this.#logId = null;
  }

  cancelCreds() {
    this.askCreds = false;
    this.hostId = "";
    this.choice = "";
    this.username = "";
    this.password = "";
  }

  /** Free the connection when the view goes away. */
  async dispose() {
    this.#endLog();
    if (this.connectedHost) await sftp.close(this.sessionId).catch(() => {});
  }
}
