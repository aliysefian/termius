// @ts-nocheck: runs in the browser under test, not in the app.
// A stand-in for the Tauri backend, enough to open a local terminal in a browser: an unlocked vault
// and a tiny fake shell. The shell prints a prompt with OSC 133 marks, echoes what is typed, "runs"
// a line on Enter, and records every byte the terminal sends it in window.__sent.
(() => {
  let cbId = 1;
  const sent = [];
  window.__sent = sent;
  const enc = new TextEncoder();
  const listeners = {};
  let channel = null;
  let index = 0;
  let line = "";
  Object.defineProperty(window, "__line", { get: () => line });
  const out = (text) => channel && window["_" + channel.id]({ index: index++, message: Array.from(enc.encode(text)) });
  const prompt = () => out("\x1b]133;A\x07$ \x1b]133;B\x07");

  function shell(bytes) {
    for (const ch of new TextDecoder().decode(bytes)) {
      if (ch === "\r") {
        out("\r\n\x1b]133;C\x07");
        out(`ran: ${line}\r\n\x1b]133;D;0\x07`);
        line = "";
        prompt();
      } else if (ch === "\x15") {
        line = "";
        out("\r\x1b[2K");
        prompt();
      } else if (ch === "\x7f") {
        if (line) {
          line = line.slice(0, -1);
          out("\b \b");
        }
      } else if (ch >= " " && ch !== "\x1b") {
        line += ch;
        out(ch);
      }
    }
  }

  async function invoke(cmd, args = {}) {
    switch (cmd) {
      case "get_status": return { state: "unlocked", path: "/v", vault_id: "v" };
      case "get_vault_settings": return { rev: 0, settings: { backup_retention: 10, destructive_patterns: [], paste_confirm_lines: 0, clipboard_clear_secs: 30 } };
      case "plugin:event|listen":
        (listeners[args.event] ??= []).push(args.handler);
        return args.handler;
      case "local_spawn":
        channel = args.onData;
        // The backend reports the pane as connected, then the shell prints its prompt.
        setTimeout(() => {
          for (const h of listeners["ssh:status"] ?? []) window["_" + h]({ event: "ssh:status", id: 0, payload: { pane_id: args.paneId, status: { kind: "connected" } } });
          prompt();
        }, 50);
        return null;
      case "local_write": {
        const bytes = Uint8Array.from(args.data);
        sent.push(Array.from(bytes));
        // A cursor-key sequence is not text; the fake shell has no cursor to move.
        if (bytes[0] !== 0x1b) shell(bytes);
        return null;
      }
      case "local_shells": return [];
      case "local_resize":
      case "local_close": return null;
      default:
        if (cmd.startsWith("list_")) return [];
        if (/statuses$/.test(cmd)) return {};
        return null;
    }
  }
  window.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback: (cb) => { const id = cbId++; window["_" + id] = cb; return id; },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
