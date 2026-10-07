// What the Kubernetes page has open, kept while the person goes to another page (opening a shell in a pod
// switches to a terminal tab) and comes back. Closed when the vault locks.
import * as api from "$lib/api";
import type { KubeInfo } from "$lib/kubedata";
import { vaultStore } from "$lib/stores/vault.svelte";
import type { Uuid } from "$lib/types";

export interface KubeSource {
  key: string;
  label: string;
  hostId: Uuid | null;
  session: Uuid;
  info: KubeInfo;
}

export interface Forward {
  stream: Uuid;
  label: string;
  state: "starting" | "listening" | "ended" | "error";
  message: string;
}

class KubeState {
  source = $state<KubeSource | null>(null);
  context = $state("");
  namespace = $state("");
  forwards = $state<Forward[]>([]);

  constructor() {
    $effect.root(() => {
      $effect(() => {
        if (!vaultStore.unlocked && this.source) void this.close();
      });
    });
  }

  async close() {
    for (const f of this.forwards) if (f.stream) void api.kube.stop(f.stream).catch(() => {});
    this.forwards = [];
    const s = this.source;
    this.source = null;
    if (s) await api.kube.close(s.session).catch(() => {});
  }
}

export const kubeState = new KubeState();
