// Revealing and copying stored secrets. The Rust side decides whether the
// master password is needed (see src-tauri/src/reveal.rs); this module asks
// for it when told to, and clears copied secrets from the clipboard.
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import * as api from "$lib/api";
import { ui } from "$lib/stores/ui.svelte";
import { errorMessage, isApiError, type Revealed, type Uuid } from "$lib/types";

export const CLIPBOARD_CLEAR_SECS = 30;

interface Prompt {
  reason: string;
  error: string | null;
  busy: boolean;
  resolve: (password: string | null) => void;
}

class SecretsStore {
  prompt = $state<Prompt | null>(null);

  /** Ask for the master password. Resolves to null if cancelled. */
  ask(reason: string, error: string | null = null): Promise<string | null> {
    return new Promise((resolve) => {
      this.prompt = { reason, error, busy: false, resolve };
    });
  }

  submit(password: string) {
    const p = this.prompt;
    if (!p) return;
    p.busy = true;
    p.resolve(password);
  }

  cancel() {
    this.prompt?.resolve(null);
    this.prompt = null;
  }
}

export const secrets = new SecretsStore();

/**
 * Fetch an identity's secrets, asking for the master password when needed
 * and re-asking after a wrong one. Returns null if the user cancels or the
 * reveal is refused (the reason is shown as a toast).
 */
export async function revealIdentity(id: Uuid, reason: string): Promise<Revealed | null> {
  let password: string | null = null;
  let error: string | null = null;
  for (;;) {
    try {
      const res = await api.identities.reveal(id, password);
      secrets.prompt = null;
      return res.secret;
    } catch (e) {
      if (!isApiError(e) || !["reauth_required", "wrong_password", "locked_out"].includes(e.code)) {
        secrets.prompt = null;
        ui.notify("error", errorMessage(e));
        return null;
      }
      if (e.code !== "reauth_required") error = e.message;
      password = await secrets.ask(reason, error);
      if (password === null) return null;
    }
  }
}

/** Copy a secret, then clear the clipboard if it still holds it later. */
export async function copySecret(text: string, what: string) {
  try {
    await writeText(text);
    ui.notify("info", `${what} copied. The clipboard clears in ${CLIPBOARD_CLEAR_SECS} seconds.`);
    setTimeout(async () => {
      try {
        if ((await readText()) === text) await writeText("");
      } catch {
        // Clipboard unavailable; nothing to clear.
      }
    }, CLIPBOARD_CLEAR_SECS * 1000);
  } catch (e) {
    ui.notify("error", `Copy failed: ${errorMessage(e)}`);
  }
}
