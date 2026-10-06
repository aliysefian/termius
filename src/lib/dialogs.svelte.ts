// The app's own confirm and text-input dialogs. The webview's built-in
// window.confirm()/prompt() aren't reliably shown in the packaged app (they
// can return immediately without asking), so nothing may rely on them.
export interface AskOptions {
  title?: string;
  /** Label of the button that says yes. */
  confirm?: string;
  /** Red confirm button; Cancel keeps the focus. */
  danger?: boolean;
  /** Label of an optional checkbox ("Don't ask again"); see askRemember. */
  checkbox?: string;
  /** The confirm button stays disabled until this exact text is typed (for production-grade actions). */
  requireText?: string;
}

interface Pending {
  kind: "confirm" | "text" | "choice";
  title: string;
  message: string;
  confirm: string;
  danger: boolean;
  value: string;
  placeholder: string;
  checkbox: string;
  requireText: string;
  /** For `choice`: one button each. */
  choices: { value: string; label: string }[];
  resolve: (v: boolean | string | null, checked: boolean) => void;
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

  close(answer: boolean | string | null, checked = false) {
    const p = this.pending;
    this.pending = this.#queue.shift() ?? null;
    p?.resolve(answer, checked);
  }

  ask(message: string, opts: AskOptions = {}): Promise<boolean> {
    return this.askRemember(message, opts).then((r) => r.ok);
  }

  /** Like ask(), and also reports whether the optional checkbox was ticked. */
  askRemember(message: string, opts: AskOptions = {}): Promise<{ ok: boolean; checked: boolean }> {
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
        checkbox: opts.checkbox ?? "",
        requireText: opts.requireText ?? "",
        choices: [],
        resolve: (v, checked) => resolve({ ok: v === true, checked }),
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
        checkbox: "",
        requireText: "",
        choices: [],
        resolve: (v) => resolve(typeof v === "string" ? v : null),
      }),
    );
  }

  /** A question with several answers, one button each. Resolves to the chosen `value`, or null if dismissed. */
  choose(message: string, choices: { value: string; label: string }[], opts: { title?: string } = {}): Promise<string | null> {
    return new Promise((resolve) =>
      this.#open({
        kind: "choice",
        title: opts.title ?? "Choose",
        message,
        confirm: "",
        danger: false,
        value: "",
        placeholder: "",
        checkbox: "",
        requireText: "",
        choices,
        resolve: (v) => resolve(typeof v === "string" ? v : null),
      }),
    );
  }
}

export const dialogs = new DialogStore();
export const ask = (message: string, opts?: AskOptions) => dialogs.ask(message, opts);
export const askRemember = (message: string, opts?: AskOptions) => dialogs.askRemember(message, opts);
export const choose = (message: string, choices: { value: string; label: string }[], opts?: { title?: string }) => dialogs.choose(message, choices, opts);
export const askText = (message: string, initial?: string, opts?: AskOptions & { placeholder?: string }) =>
  dialogs.askText(message, initial, opts);
