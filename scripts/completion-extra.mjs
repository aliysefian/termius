#!/usr/bin/env node
// Command specs written by hand for what the source package lacks (journalctl, apt-get, dnf, ip, ss, awk) and
// the global options it leaves out of kubectl. Applied on top of the generated specs, so a rebuild keeps them.
//
//   node scripts/completion-extra.mjs        apply to src/lib/completion/specs (also done by the build script)
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(resolve(dirname(fileURLToPath(import.meta.url)), ".."), "src/lib/completion/specs");

const text = (t, extra = {}) => ({ k: "text", t, ...extra });
const file = (t = "file", extra = {}) => ({ k: "file", t, ...extra });
const opt = (n, d, a) => ({ n, d, ...(a ? { a } : {}) });
const sub = (n, d, extra = {}) => ({ n, d, ...extra });

const packageManager = (d, verbs) => ({
  d,
  o: [opt(["-y", "--yes", "--assumeyes"], "Answer yes to every question"), opt(["-q", "--quiet"], "Print less")],
  s: verbs.map(([n, dd, args]) => sub(n, dd, args ? { a: [text("package", { o: 1, m: 1 })] } : {})),
});

export const EXTRA = {
  journalctl: {
    d: "Query the systemd journal",
    o: [
      opt(["-u", "--unit"], "Show messages from this unit", [text("unit")]),
      opt(["-f", "--follow"], "Follow new messages"),
      opt(["-n", "--lines"], "How many of the newest lines to show", [text("lines")]),
      opt(["-b", "--boot"], "Show messages from this boot", [text("boot", { o: 1 })]),
      opt(["-p", "--priority"], "Only this priority or worse", [{ k: "enum", t: "priority", v: [["emerg", "System is unusable"], ["alert", "Act immediately"], ["crit", "Critical"], ["err", "Errors"], ["warning", "Warnings"], ["notice", "Normal but significant"], ["info", "Informational"], ["debug", "Debug"]] }]),
      opt(["-S", "--since"], "Show entries since a date or time", [text("date")]),
      opt(["-U", "--until"], "Show entries until a date or time", [text("date")]),
      opt(["-r", "--reverse"], "Newest first"),
      opt(["-k", "--dmesg"], "Kernel messages only"),
      opt(["-g", "--grep"], "Only entries whose message matches a pattern", [text("pattern")]),
      opt(["-o", "--output"], "How to print each entry", [{ k: "enum", t: "format", v: ["short", "short-iso", "short-precise", "cat", "json", "json-pretty", "verbose"] }]),
      opt("--no-pager", "Do not pipe the output into a pager"),
      opt("--disk-usage", "Show how much disk the journal uses"),
      opt("--vacuum-time", "Delete entries older than this", [text("time")]),
      opt("--vacuum-size", "Shrink the journal to this size", [text("size")]),
    ],
  },
  "apt-get": packageManager("APT package handling utility", [
    ["update", "Refresh the package lists"],
    ["upgrade", "Install the newest versions of installed packages"],
    ["dist-upgrade", "Upgrade, adding or removing packages as needed"],
    ["install", "Install packages", 1],
    ["remove", "Remove packages", 1],
    ["purge", "Remove packages and their settings", 1],
    ["autoremove", "Remove packages nothing needs any more"],
    ["clean", "Delete downloaded package files"],
    ["autoclean", "Delete downloaded package files that can no longer be used"],
    ["download", "Download a package without installing it", 1],
    ["source", "Download the source of a package", 1],
  ]),
  dnf: packageManager("Package manager for Fedora and Red Hat systems", [
    ["install", "Install packages", 1],
    ["remove", "Remove packages", 1],
    ["update", "Update packages", 1],
    ["upgrade", "Upgrade packages", 1],
    ["search", "Search for packages", 1],
    ["info", "Show details of a package", 1],
    ["list", "List packages", 1],
    ["provides", "Find which package provides a file", 1],
    ["history", "Show past transactions"],
    ["autoremove", "Remove packages nothing needs any more"],
    ["clean", "Delete cached data"],
    ["makecache", "Refresh the package metadata"],
    ["repolist", "List the enabled repositories"],
    ["check-update", "List the available updates"],
  ]),
  ip: {
    d: "Show and change network addresses, routes and links",
    o: [
      opt(["-4"], "Only IPv4"),
      opt(["-6"], "Only IPv6"),
      opt(["-br", "-brief"], "Short, table-style output"),
      opt(["-c", "-color"], "Colour the output"),
      opt(["-s", "-stats"], "Show statistics"),
      opt(["-j", "-json"], "Print as JSON"),
    ],
    s: [
      sub("addr", "Addresses on interfaces", { s: [sub("show", "List addresses"), sub("add", "Add an address"), sub("del", "Remove an address"), sub("flush", "Remove all addresses of an interface")] }),
      sub("link", "Network interfaces", { s: [sub("show", "List interfaces"), sub("set", "Change an interface, such as up or down")] }),
      sub("route", "The routing table", { s: [sub("show", "List routes"), sub("add", "Add a route"), sub("del", "Remove a route"), sub("get", "Show the route to an address")] }),
      sub("neigh", "Neighbours (the ARP table)", { s: [sub("show", "List neighbours"), sub("flush", "Remove neighbours")] }),
      sub("rule", "Routing policy rules"),
      sub("netns", "Network namespaces"),
      sub("tunnel", "Tunnels over IP"),
      sub("maddr", "Multicast addresses"),
    ],
  },
  ss: {
    d: "Show network sockets",
    o: [
      opt(["-t", "--tcp"], "TCP sockets"),
      opt(["-u", "--udp"], "UDP sockets"),
      opt(["-x", "--unix"], "Unix sockets"),
      opt(["-l", "--listening"], "Only sockets that are listening"),
      opt(["-a", "--all"], "Listening and connected sockets"),
      opt(["-n", "--numeric"], "Do not look up names"),
      opt(["-p", "--processes"], "Show the process that owns each socket"),
      opt(["-e", "--extended"], "More details"),
      opt(["-s", "--summary"], "Totals only"),
      opt(["-4", "--ipv4"], "Only IPv4"),
      opt(["-6", "--ipv6"], "Only IPv6"),
      opt(["-o", "--options"], "Show timers"),
      opt(["-H", "--no-header"], "Leave out the header line"),
      opt(["-r", "--resolve"], "Look up names"),
      opt(["-m", "--memory"], "Show socket memory use"),
    ],
  },
  awk: {
    d: "Pattern scanning and processing language",
    o: [
      opt("-F", "Field separator", [text("separator")]),
      opt("-v", "Set a variable before the program runs", [text("var=value")]),
      opt("-f", "Read the program from a file", [file("program")]),
      opt("--version", "Show the version"),
    ],
    a: [text("program"), file("file", { o: 1, m: 1 })],
  },
};

/** kubectl lists these in its docs but the source spec does not have them. */
const KUBECTL_GLOBAL = [
  { n: "--context", d: "The name of the kubeconfig context to use", a: [text("context")], p: 1 },
  { n: "--cluster", d: "The name of the kubeconfig cluster to use", a: [text("cluster")], p: 1 },
  { n: "--user", d: "The name of the kubeconfig user to use", a: [text("user")], p: 1 },
  { n: "--kubeconfig", d: "Path to the kubeconfig file to use", a: [file("kubeconfig")], p: 1 },
];

/** Write the extras into `out` and into `index` (name → one-line description). Returns the names added. */
export function applyExtras(out = OUT, index = JSON.parse(readFileSync(join(out, "index.json"), "utf8"))) {
  const added = [];
  for (const [name, spec] of Object.entries(EXTRA)) {
    writeFileSync(join(out, `${name}.json`), JSON.stringify(spec) + "\n");
    index[name] = spec.d ?? "";
    added.push(name);
  }
  const kp = join(out, "kubectl.json");
  if (existsSync(kp)) {
    const k = JSON.parse(readFileSync(kp, "utf8"));
    k.o ??= [];
    for (const o of KUBECTL_GLOBAL) if (!k.o.some((x) => [x.n].flat().includes(o.n))) k.o.push(o);
    writeFileSync(kp, JSON.stringify(k) + "\n");
  }
  return { added, index };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { added, index } = applyExtras();
  writeFileSync(join(OUT, "index.json"), JSON.stringify(index) + "\n");
  const sp = join(OUT, "SOURCE.json");
  const src = JSON.parse(readFileSync(sp, "utf8"));
  writeFileSync(sp, JSON.stringify({ ...src, commands: Object.keys(index).length, handwritten: added }, null, 2) + "\n");
  console.log(`added ${added.join(", ")}; ${Object.keys(index).length} commands`);
}
