import type { TutorialDefinition } from "@/engine/types";
import type { FeatureCategory, TutorialLoader, TutorialMeta } from "./types";

/**
 * Add a tutorial: write src/tutorials/<id>.ts (default-export a TutorialDefinition), add its metadata here and its
 * loader below. Definitions are code-split, so a visitor only downloads the ones they play.
 */
export const tutorials: TutorialMeta[] = [
  {
    id: "create-ssh-connection",
    title: "Create an SSH connection",
    description: "Add a host, trust its key on first contact, connect and run your first command.",
    difficulty: "Beginner",
    durationSec: 44,
    category: "SSH & Connections",
    steps: [
      { title: "Open the new host form", body: "In the Hosts view, press the + button next to the host list." },
      { title: "Fill in the connection", body: "Enter a label, the hostname or IP, the port and the username, then choose how to log in: a password, an SSH key, a shared credential, or Ask." },
      { title: "Save the host", body: "Press Save. The host is encrypted into your vault and appears in the list." },
      { title: "Select it and connect", body: "Select the host, press Connect and, the first time, compare and trust the server's fingerprint. SSHVault pins it so a changed key is flagged later." },
      { title: "Run a command", body: "The session opens in a terminal tab. Type a command and press Enter." },
    ],
    docs: [
      { slug: "hosts", label: "Hosts and credentials" },
      { slug: "connecting", label: "Connecting" },
    ],
    related: ["host-management", "terminal-workflow"],
    keywords: ["add host", "new host", "connect", "ssh", "fingerprint", "known hosts"],
  },
  {
    id: "host-management",
    title: "Manage your hosts",
    description: "Browse, search, select and edit hosts, and see the change everywhere it is used.",
    difficulty: "Beginner",
    durationSec: 37,
    category: "Host Management",
    steps: [
      { title: "View your hosts", body: "The Hosts view groups hosts and marks each with a status dot and an environment badge." },
      { title: "Search", body: "Type in the search box. It matches names, addresses, users and tags." },
      { title: "Select a host", body: "Click a host to see its address, port, user, group and tags." },
      { title: "Edit its settings", body: "Press Edit. The form opens with the saved values; change the label, port or tags." },
      { title: "Save the changes", body: "Press Save. The change syncs to your other computers through your vault folder." },
    ],
    docs: [{ slug: "hosts", label: "Hosts and credentials" }],
    related: ["create-ssh-connection", "run-on-many-hosts"],
    keywords: ["search", "edit", "tags", "groups", "favorites"],
  },
  {
    id: "terminal-workflow",
    title: "Work in the terminal",
    description: "Run commands, open and switch tabs, and split a tab into panes.",
    difficulty: "Beginner",
    durationSec: 43,
    category: "Terminal",
    steps: [
      { title: "Run commands", body: "A connected host opens as a tab. Click the terminal and type." },
      { title: "Open another tab", body: "Open a second connection. Tabs stay connected in the background." },
      { title: "Switch between tabs", body: "Click a tab, or use Ctrl+Tab and Ctrl+1 to Ctrl+9." },
      { title: "Split the pane", body: "Split a tab right or down (Ctrl+Shift+D / Ctrl+Shift+E), up to six panes per tab." },
    ],
    docs: [
      { slug: "terminal", label: "Terminals" },
      { slug: "shortcuts", label: "Keyboard shortcuts" },
    ],
    related: ["create-ssh-connection", "tmux-sessions"],
    keywords: ["tabs", "split", "panes", "commands", "shell"],
  },
  {
    id: "tmux-sessions",
    title: "Manage tmux sessions",
    description: "See the tmux sessions on a host, inspect panes, switch windows and attach.",
    difficulty: "Intermediate",
    durationSec: 35,
    category: "tmux",
    steps: [
      { title: "Open the Tmux page", body: "Choose Tmux in the activity bar. Sessions are read from the host over an existing connection." },
      { title: "Pick a session", body: "Select a session. Its windows and panes appear, with an attached marker." },
      { title: "Inspect the panes", body: "Pane tiles are read-only live previews. Select one for its details in the inspector." },
      { title: "Switch windows", body: "Select another window under the session to see its panes." },
      { title: "Attach to the session", body: "Press Attach to open a terminal tab on the session. Closing the tab only detaches." },
    ],
    docs: [{ slug: "tmux", label: "tmux sessions" }],
    related: ["terminal-workflow", "create-ssh-connection"],
    keywords: ["tmux", "sessions", "windows", "panes", "attach"],
  },
  {
    id: "sftp-file-transfer",
    title: "Transfer files with SFTP",
    description: "Open the two-pane file manager, navigate, select a file and upload it with progress.",
    difficulty: "Beginner",
    durationSec: 38,
    category: "File Management",
    steps: [
      { title: "Open the file manager", body: "Choose Files, pick a host in the right pane and press Connect." },
      { title: "Navigate folders", body: "Double-click folders, or click the breadcrumb to jump back." },
      { title: "Select a file", body: "Click a file in the left pane. Ctrl+click and Shift+click select several." },
      { title: "Upload with progress", body: "Press Upload, or drag the file across. The transfer queue shows progress and can pause or cancel." },
      { title: "Check the result", body: "The file appears in the remote pane. Right-click it for rename, permissions, download and delete." },
    ],
    docs: [{ slug: "sftp", label: "Files: SFTP, SCP and FTP" }],
    related: ["create-ssh-connection", "port-forwarding"],
    keywords: ["sftp", "upload", "download", "files", "scp", "transfer"],
  },
  {
    id: "port-forwarding",
    title: "Forward a port",
    description: "Create a local tunnel to a service behind a host, start it and use it.",
    difficulty: "Intermediate",
    durationSec: 33,
    category: "Networking",
    steps: [
      { title: "Open Tunnels", body: "Choose Tunnels in the activity bar to list every rule with its state." },
      { title: "Create a tunnel", body: "Press Create Tunnel, choose Local, and fill in the local port and the destination." },
      { title: "Start it", body: "Creating and starting are separate. Select the tunnel and press Start." },
      { title: "Use it", body: "Connect to 127.0.0.1 on the local port. Traffic is carried over SSH to the destination." },
    ],
    docs: [{ slug: "port-forwarding", label: "Port forwarding" }],
    related: ["sftp-file-transfer", "create-ssh-connection"],
    keywords: ["tunnel", "port forward", "socks", "ssh -L", "ssh -R", "ssh -D"],
  },
  {
    id: "ssh-agent-keys",
    title: "Keys and the built-in SSH agent",
    description: "Generate a key, turn on the vault-backed SSH agent and offer the key to ssh and git.",
    difficulty: "Intermediate",
    durationSec: 37,
    category: "Security",
    steps: [
      { title: "Open the Key Manager", body: "Choose Keys. Credentials refer to keys, so replacing one applies everywhere." },
      { title: "Generate a key", body: "Press Generate key, name it and choose a type. The public half is shown for authorized_keys." },
      { title: "Turn on the SSH agent", body: "Press Turn on, then copy the SSH_AUTH_SOCK line into your shell profile." },
      { title: "Offer the key", body: "Switch the key on and choose Ask every time or Allow while unlocked." },
    ],
    docs: [{ slug: "keys-and-agent", label: "Keys and the SSH agent" }],
    related: ["create-ssh-connection", "port-forwarding"],
    keywords: ["keys", "agent", "ed25519", "git", "authorized_keys", "security"],
  },
  {
    id: "run-on-many-hosts",
    title: "Run a snippet on many hosts",
    description: "Pick a saved command, choose several hosts and compare every result side by side.",
    difficulty: "Intermediate",
    durationSec: 33,
    category: "DevOps Tools",
    steps: [
      { title: "Open Snippets", body: "Snippets are saved commands. Placeholders like {{host}} are filled when they run." },
      { title: "Choose a snippet", body: "Select a snippet, then press Run on several hosts." },
      { title: "Pick the hosts", body: "Tick the hosts. Up to eight run at a time, each with a timeout." },
      { title: "Run and compare", body: "Each host shows its exit code, duration and output." },
    ],
    docs: [{ slug: "snippets", label: "Snippets" }],
    related: ["host-management", "terminal-workflow"],
    keywords: ["snippets", "fleet", "many hosts", "run", "automation", "ansible"],
  },
];

/** Homepage and feature-page loops. Not listed in the tutorial library. */
export const heroId = "hero";

const loaders: Record<string, TutorialLoader> = {
  hero: () => import("./hero"),
  "create-ssh-connection": () => import("./create-ssh-connection"),
  "host-management": () => import("./host-management"),
  "terminal-workflow": () => import("./terminal-workflow"),
  "tmux-sessions": () => import("./tmux-sessions"),
  "sftp-file-transfer": () => import("./sftp-file-transfer"),
  "port-forwarding": () => import("./port-forwarding"),
  "ssh-agent-keys": () => import("./ssh-agent-keys"),
  "run-on-many-hosts": () => import("./run-on-many-hosts"),
};

export function loadTutorial(id: string): Promise<TutorialDefinition> {
  const l = loaders[id];
  if (!l) return Promise.reject(new Error(`Unknown tutorial: ${id}`));
  return l().then((m) => m.default);
}

export const tutorialIds = Object.keys(loaders);
export const getTutorial = (id: string) => tutorials.find((t) => t.id === id);

export const categories: FeatureCategory[] = ["SSH & Connections", "Host Management", "Terminal", "File Management", "tmux", "Networking", "Security", "DevOps Tools"];
