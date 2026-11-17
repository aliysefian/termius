import type { TutorialDefinition } from "@/engine/types";
import { c } from "@/components/sim/ansi";
import { click, move, out, run, set, toast, wait } from "./helpers";

const P1 = "demo@prod-api:~$ ";
const P2 = "admin@db:~$ ";

const def: TutorialDefinition = {
  id: "terminal-workflow",
  initial: {
    view: "terminal",
    flags: { "tab-prod-api": true },
    selected: { tab: "prod-api" },
    terminals: { "prod-api": ["Welcome to Ubuntu 24.04 LTS (GNU/Linux 6.8.0-45-generic x86_64)", "", "Last login: Sun Nov 15 21:40:12 2026 from 198.51.100.9"] },
  },
  cursorStart: { x: 0.85, y: 0.8 },
  posterTime: 38000,
  steps: [
    wait(700, { chapter: "Run commands", note: "A connected host opens as a tab. Everything you type goes to the remote shell." }),
    move("term-prod-api", 850),
    click("term-prod-api"),
    run("prod-api", P1, "ls -la /srv/app"),
    out(
      "prod-api",
      ["total 20", "drwxr-xr-x 5 demo demo 4096 Nov 16 12:01 " + c.b("."), "drwxr-xr-x 3 root root 4096 Nov  1 09:00 " + c.b(".."), "drwxr-xr-x 2 demo demo 4096 Nov 16 03:00 " + c.b("backups"), "-rw-r--r-- 1 demo demo 2314 Nov 12 10:22 app.conf"],
      700,
    ),
    wait(500),
    run("prod-api", P1, "systemctl is-active app"),
    out("prod-api", [c.g("active")], 300),
    wait(600),

    move("tab-new", 900, { chapter: "Open another tab", note: "The + opens a new connection in its own tab. Other tabs stay connected in the background." }),
    click("tab-new", {
      flags: { "tab-db": true },
      selected: { tab: "db" },
      terminals: { db: ["Welcome to Debian GNU/Linux 12 (bookworm)", "", "Last login: Sat Nov 14 08:12:44 2026 from 192.0.2.77"] },
    }),
    wait(500),
    move("term-db", 700),
    click("term-db"),
    run("db", P2, "systemctl status postgresql --no-pager | head -3"),
    out("db", ["● postgresql.service - PostgreSQL RDBMS", "     Loaded: loaded (/lib/systemd/system/postgresql.service)", "     Active: " + c.g("active (exited)") + " since Mon 2026-11-02 04:11:09 UTC"], 800),
    wait(600),

    move("tab-prod-api", 900, { chapter: "Switch between tabs", note: "Click a tab to switch. Each keeps its own scrollback." }),
    click("tab-prod-api", { selected: { tab: "prod-api" } }),
    wait(1100),
    move("tab-db", 700),
    click("tab-db", { selected: { tab: "db" } }),
    wait(700),

    move("topbar-split", 850, { chapter: "Split the pane", note: "Split a tab into up to six panes, right or down." }),
    click("topbar-split", { flags: { split: true }, selected: { pane: "2" }, terminals: { "db-2": ["Last login: Sat Nov 14 08:12:44 2026"] } }),
    wait(500),
    move("term-db-2", 800),
    click("term-db-2"),
    run("db", P2, "tail -n 3 /var/log/postgresql/postgresql.log", {}, "term-input-db-2", "db-2"),
    out("db-2", ["2026-11-16 14:02:11 UTC [412] LOG:  checkpoint complete", "2026-11-16 14:07:12 UTC [412] LOG:  checkpoint starting: time", "2026-11-16 14:07:13 UTC [412] LOG:  checkpoint complete"], 700),
    toast("Two panes, one connection per pane", 1800, "info"),
    set({}),
    wait(1500),
  ],
};

export default def;
