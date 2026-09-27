// The app's own confirm and text-input dialogs. The webview's built-in
// window.confirm()/prompt() aren't reliably shown in the packaged app (they
// can return immediately without asking), so nothing may rely on them.
export interface AskOptions {
  title?: string;
  /** Label of the button that says yes. */
  confirm?: string;
  /** Red confirm button; Cancel keeps the focus. */
  danger?: boolean;
}

interface Pending {
  kind: "confirm" | "text";
  title: string;
  message: string;
  confirm: string;
  danger: boolean;
  value: string;
  placeholder: string;
  resolve: (v: boolean | string | null) => void;
}

/** "Delete snippet …?" → a red "Delete" button. */
const DANGER = /^(Delete|Remove|Stop trusting|Replace|Restore|Export)\b/;

class DialogStore {
  pending = $state<Pending | null>(null);
  #queue: Pending[] = [];

  #open(p: Pending) {
    if (this.pending) this.#queue.push(p);
    else this.pending = p;
  }

  close(answer: boolean | string | null) {
    const p = this.pending;
    this.pending = this.#queue.shift() ?? null;
    p?.resolve(answer);
  }

  ask(message: string, opts: AskOptions = {}): Promise<boolean> {
    const verb = DANGER.exec(message)?.[1];
    const danger = opts.danger ?? !!verb;
    return new Promise((resolve) =>
      this.#open({
        kind: "confirm",
        title: opts.title ?? (danger ? "Are you sure?" : "Please confirm"),
        message,
        confirm: opts.confirm ?? verb ?? "Continue",
        danger,
        value: "",
        placeholder: "",
        resolve: (v) => resolve(v === true),
      }),
    );
  }

  askText(message: string, initial = "", opts: AskOptions & { placeholder?: string } = {}): Promise<string | null> {
    return new Promise((resolve) =>
      this.#open({
        kind: "text",
        title: opts.title ?? message,
        message: opts.title ? message : "",
        confirm: opts.confirm ?? "OK",
        danger: false,
        value: initial,
        placeholder: opts.placeholder ?? "",
        resolve: (v) => resolve(typeof v === "string" ? v : null),
      }),
    );
  }
}

export const dialogs = new DialogStore();
export const ask = (message: string, opts?: AskOptions) => dialogs.ask(message, opts);
export const askText = (message: string, initial?: string, opts?: AskOptions & { placeholder?: string }) =>
  dialogs.askText(message, initial, opts);
