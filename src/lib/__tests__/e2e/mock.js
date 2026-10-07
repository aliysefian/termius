// @ts-nocheck: runs in the browser under test, not in the app.
// A stand-in for the Tauri backend, enough to open a local terminal in a browser: an unlocked vault
// and a tiny fake shell. The shell prints a prompt with OSC 133 marks, echoes what is typed, "runs"
// a line on Enter, and records every byte the terminal sends it in window.__sent.
(() => {
  let cbId = 1;
  let monitorId = 0;
  const sent = [];
  window.__sent = sent;
  const lookups = [];
  window.__lookups = lookups;
  const enc = new TextEncoder();
  const listeners = {};
  let lastPane = null;
  // End the session the way the backend does: a status event (the shell is not told anything).
  window.__emitStatus = (kind) => {
    for (const h of listeners["ssh:status"] ?? []) window["_" + h]({ event: "ssh:status", id: 0, payload: { pane_id: lastPane, status: kind === "disconnected" ? { kind, code: 0 } : { kind } } });
  };
  let channel = null;
  let index = 0;
  let line = "";
  Object.defineProperty(window, "__line", { get: () => line });
  // With real shells (shells.py) the page is given a pty: bytes go to it, and what it prints comes back here.
  const real = () => typeof window.__ptySpawn === "function";
  window.__deliver = (bytes) => channel && window["_" + channel.id]({ index: index++, message: bytes });
  const out = (text) => channel && window["_" + channel.id]({ index: index++, message: Array.from(enc.encode(text)) });
  // The prompt reports the folder (OSC 7), like a shell with integration, so relative paths can be resolved.
  const prompt = () => out("\x1b]7;file://mockhost/work/app\x07\x1b]133;A\x07$ \x1b]133;B\x07");

  // "secret": a command is waiting for a password (nothing echoed). "alt": a full-screen program.
  let mode = null;

  function shell(bytes) {
    for (const ch of new TextDecoder().decode(bytes)) {
      if (mode) {
        if (ch === "\r") {
          if (mode === "alt") out("\x1b[?1049l");
          out("\r\n\x1b]133;D;0\x07");
          mode = null;
          line = "";
          prompt();
        }
        continue;
      }
      if (ch === "\r") {
        out("\r\n\x1b]133;C\x07");
        if (line.startsWith("sudo")) {
          mode = "secret";
          out("[sudo] password for me: ");
          continue;
        }
        if (line.startsWith("mouse-on")) {
          // A program that asks for every mouse event, in the SGR encoding, and stays running.
          mode = "secret";
          out("\x1b[?1003h\x1b[?1006h");
          continue;
        }
        if (line.startsWith("vim")) {
          mode = "alt";
          out("\x1b[?1049h\x1b[2J\x1b[Hfull screen program");
          continue;
        }
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
    // A page can answer some commands itself (a screenshot script gives the vault some hosts).
    if (typeof window.__invoke === "function") {
      const r = await window.__invoke(cmd, args);
      if (r !== undefined) return r;
    }
    switch (cmd) {
      case "get_status": return { state: "unlocked", path: "/v", vault_id: "v" };
      case "get_vault_settings": return { rev: 0, settings: { backup_retention: 10, destructive_patterns: [], paste_confirm_lines: 0, clipboard_clear_secs: 30 } };
      case "plugin:event|listen":
        (listeners[args.event] ??= []).push(args.handler);
        return args.handler;
      case "kube_open":
        window.__kubeOpened = [...(window.__kubeOpened ?? []), args.hostId];
        return { session_id: "k-" + (++monitorId), info: { version: "v1.30.2", contexts: ["prod", "staging"], current: "prod" } };
      case "kube_close": return null;
      case "kube_namespaces": return ["jobs", "shop"];
      case "kube_pods": {
        window.__kubePodCalls = [...(window.__kubePodCalls ?? []), { context: args.context, scope: args.scope }];
        const all = [
          { name: "web-7d9f-abc", namespace: "shop", status: "Running", ready: "2/2", restarts: 0, node: "node-1", ip: "10.1.2.3", created: new Date(Date.now() - 3 * 3600_000).toISOString(), containers: ["app", "sidecar"], owner: "ReplicaSet/web-7d9f" },
          { name: "api-0", namespace: "shop", status: "CrashLoopBackOff", ready: "0/1", restarts: 12, node: "node-2", ip: null, created: new Date(Date.now() - 2 * 86400_000).toISOString(), containers: ["api"], owner: "StatefulSet/api" },
          { name: "batch-1", namespace: "jobs", status: "Succeeded", ready: "0/1", restarts: 0, node: "node-1", ip: null, created: new Date(Date.now() - 600_000).toISOString(), containers: ["c"], owner: null },
        ];
        return args.scope.scope === "namespace" ? all.filter((p) => p.namespace === args.scope.name) : all;
      }
      case "kube_describe": return "Name: " + args.pod + "\nNamespace: " + args.namespace + "\nStatus: Running";
      case "kube_delete_pod":
        window.__kubeDeleted = [...(window.__kubeDeleted ?? []), args.pod];
        return null;
      case "kube_logs_start": {
        window.__kubeLogs = [...(window.__kubeLogs ?? []), { pod: args.pod, container: args.container, options: args.options }];
        const ch = args.onEvent;
        let idx = 0;
        const emit = (m) => window["_" + ch.id]({ index: idx++, message: m });
        setTimeout(() => emit({ event: "chunk", text: "starting app\r\nlistening on :8080\r\nERROR upstream timed out\r\n" }), 50);
        return "ks-" + (++monitorId);
      }
      case "kube_forward_start": {
        window.__kubeForwards = [...(window.__kubeForwards ?? []), { to: args.to, local: args.localPort, remote: args.remotePort }];
        const ch = args.onEvent;
        let idx = 0;
        const emit = (m) => window["_" + ch.id]({ index: idx++, message: m });
        setTimeout(() => emit({ event: "chunk", text: "Forwarding from 127.0.0.1:" + args.localPort + " -> " + args.remotePort + "\n" }), 50);
        return "kf-" + (++monitorId);
      }
      case "kube_stop":
        window.__kubeStopped = (window.__kubeStopped ?? 0) + 1;
        return null;
      case "save_snippet": {
        window.__savedSnippets = [...(window.__savedSnippets ?? []), args.snippet];
        return { id: "sn-" + (++monitorId), rev: 1, updated_at: 1, deleted: false, data: args.snippet };
      }
      case "plugin:dialog|open": return window.__pickedFile ?? null;
      case "read_text_file": return window.__fileText ?? "";
      case "known_hosts_list": return [];
      case "monitor_open": return "m-" + (++monitorId);
      case "monitor_close": return null;
      case "monitor_exec": {
        window.__execs = [...(window.__execs ?? []), args.script];
        if (args.script.includes("list-units")) return { stdout: window.__unitsOut ?? "", stderr: "", code: 0 };
        if (args.script.includes("systemctl status")) return { stdout: "● nginx.service - web\n   Active: active (running)", stderr: "", code: 0 };
        if (args.script.includes("systemctl ")) return { stdout: "", stderr: "", code: window.__actionCode ?? 0 };
        return { stdout: "", stderr: "", code: 0 };
      }
      case "monitor_stream_start": {
        window.__streams = [...(window.__streams ?? []), args.script];
        const ch = args.onEvent;
        let idx = 0;
        const emit = (m) => window["_" + ch.id]({ index: idx++, message: m });
        const id = "stream-" + (++monitorId);
        const lines = ["Oct 07 10:00:01 web sshd[1]: Accepted publickey for ops", "Oct 07 10:00:02 web nginx[2]: [warn] slow upstream", "Oct 07 10:00:03 web app[3]: ERROR database timeout", "Oct 07 10:00:04 web app[3]: started"];
        setTimeout(() => emit({ event: "chunk", text: lines.slice(0, 2).join("\r\n") + "\r\n" + lines[2].slice(0, 20) }), 50);
        setTimeout(() => emit({ event: "chunk", text: lines[2].slice(20) + "\r\n" + lines[3] + "\r\n" }), 150);
        window.__stopStream = () => emit({ event: "end", code: null, error: null });
        return id;
      }
      case "monitor_stream_stop":
        window.__stopStream?.();
        return null;
      case "completion_lookup_local":
      case "completion_lookup": {
        if (cmd === "completion_lookup_local") (window.__localLookups = window.__localLookups ?? []).push(JSON.parse(JSON.stringify(args)));
        else
        lookups.push(JSON.parse(JSON.stringify(args)));
        const r = args.request;
        if (window.__lookupMode === "refuse") return Promise.reject({ code: "completion_refused", message: "the host does not allow lookups on this connection" });
        if (r.kind === "generator") {
          const names = { "git-branches": ["main", "feature/x", "origin/main"], "systemd-units": ["nginx.service", "ssh.service"] }[r.id] ?? [];
          return { entries: names.map((name) => ({ name, dir: false })), truncated: false };
        }
        const dirs = {
          "/work/app": [["README.md", 0], ["src", 1], [".env", 0], ["my notes.txt", 0], ["it's.txt", 0]],
          "/work/app/src": [["main.rs", 0], ["lib", 1]],
          "~": [["docs", 1], ["notes.md", 0]],
          "/etc": [["hosts", 0], ["hostname", 0], ["ssh", 1]],
        };
        const list = (dirs[r.dir] ?? []).filter(([n]) => n.startsWith(r.prefix));
        return { entries: list.map(([name, dir]) => ({ name, dir: !!dir })), truncated: r.dir === "/etc" && !r.prefix };
      }
      case "ssh_connect":
      case "ssh_connect_adhoc":
      case "local_spawn":
        channel = args.onData;
        lastPane = args.paneId;
        if (real()) {
          await window.__ptySpawn(args.cols, args.rows);
          setTimeout(() => {
            for (const h of listeners["ssh:status"] ?? []) window["_" + h]({ event: "ssh:status", id: 0, payload: { pane_id: args.paneId, status: { kind: "connected" } } });
          }, 50);
          return null;
        }
        // The backend reports the pane as connected, then the shell prints its prompt.
        setTimeout(() => {
          for (const h of listeners["ssh:status"] ?? []) window["_" + h]({ event: "ssh:status", id: 0, payload: { pane_id: args.paneId, status: { kind: "connected" } } });
          prompt();
        }, 50);
        return null;
      case "ssh_write":
      case "local_write": {
        const bytes = Uint8Array.from(args.data);
        sent.push(Array.from(bytes));
        if (real()) {
          window.__ptyWrite(Array.from(bytes));
          return null;
        }
        // A cursor-key sequence is not text; the fake shell has no cursor to move.
        if (bytes[0] !== 0x1b) shell(bytes);
        return null;
      }
      case "local_shells": return [];
      case "list_snippets": return [
        { id: "sn1", rev: 1, updated_at: 1, deleted: false, data: { label: "Disk usage", command: "df -h", description: "Free space", abbreviation: "dfh" } },
        { id: "sn2", rev: 1, updated_at: 1, deleted: false, data: { label: "Tail log", command: "tail -f /var/log/{{file}}.log", description: "" } },
        { id: "sn3", rev: 1, updated_at: 1, deleted: false, data: { label: "Two lines", command: "cd /srv\nls", description: "" } },
      ];
      case "local_resize":
        if (real()) window.__ptyResize(args.cols, args.rows);
        return null;
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
