export interface Faq {
  q: string;
  a: string;
  doc?: { slug: string; label: string; hash?: string };
}

export interface FaqGroup {
  title: string;
  items: Faq[];
}

/** Answers are condensed from README.md and docs/USAGE.md. Keep them factual; link out for detail. */
export const faqGroups: FaqGroup[] = [
  {
    title: "About SSHVault",
    items: [
      { q: "What is SSHVault?", a: "An open-source SSH client for Linux, Windows and macOS. Hosts, keys, credentials, tunnels and snippets are encrypted on your computer and synced through a folder you already use. It is built with Rust, Tauri, Svelte and xterm.js and released under the MIT license.", doc: { slug: "getting-started", label: "Getting started" } },
      { q: "Do I need an account or a server?", a: "No. There is no account and no SSHVault server. Your sync service (Dropbox, OneDrive, Nextcloud, Syncthing, iCloud Drive) only ever stores ciphertext.", doc: { slug: "vault-and-sync", label: "Vault and sync" } },
      { q: "What does my sync service see?", a: "Random file names, how many records of each type there are, their sizes and when they change. Never host names, users, addresses, keys, passwords, snippets or anything that unlocks the vault.", doc: { slug: "security", label: "Security model" } },
      { q: "Is it production ready?", a: "SSHVault is young. The SSH engine, vault, agent and sync have more than 150 automated tests, many against a real OpenSSH server, but the app has not had wide real-world use yet. Keep backups of anything important.", doc: { slug: "security", label: "Security model" } },
      { q: "Are these tutorials real recordings?", a: "No. They are silent, self-running simulations built with React and fictional data (documentation-only IP addresses). They are modelled on the app's layout and workflows; nothing connects to a server." },
    ],
  },
  {
    title: "Install and updates",
    items: [
      { q: "Which systems are supported?", a: "Windows 10/11 (.exe and .msi), macOS on Apple Silicon and Intel (.dmg), and Linux (.deb, .rpm and AppImage; glibc 2.35 or newer).", doc: { slug: "install", label: "Install" } },
      { q: "Windows SmartScreen warns about the installer.", a: "The installers are not code-signed yet. Choose More info, then Run anyway. Signing is built and switches on when the keys are added.", doc: { slug: "install", label: "Install" } },
      { q: "macOS blocks the app or says it is damaged.", a: "The app is not signed with an Apple Developer ID yet. Use System Settings, Privacy & Security, Open Anyway. If it still says damaged, run xattr -dr com.apple.quarantine /Applications/SSHVault.app once.", doc: { slug: "install", label: "Install" } },
    ],
  },
  {
    title: "Using the vault",
    items: [
      { q: "I forgot my master password.", a: "The master password is never stored anywhere. Without it or the recovery key shown when you created the vault, nobody can open it, including you. Keep the recovery key somewhere safe and offline.", doc: { slug: "vault-and-sync", label: "Vault and sync" } },
      { q: "Changes from another computer do not appear.", a: "Check that your sync client is running and has finished. Comparing the Revision value on each computer's Vault screen tells you whether they hold the same data.", doc: { slug: "troubleshooting", label: "Troubleshooting" } },
      { q: "“Another device changed the same item” when saving.", a: "Someone, or you on another computer, edited the same field before the change synced. The current version is now shown; make your change again.", doc: { slug: "troubleshooting", label: "Troubleshooting" } },
    ],
  },
  {
    title: "Connections and terminals",
    items: [
      { q: "The terminal says the host key changed.", a: "The server presented a different key from the one trusted before. If you expected that (a reinstalled server), compare fingerprints and replace the key. If you did not, do not connect.", doc: { slug: "troubleshooting", label: "Troubleshooting" } },
      { q: "ssh-agent authentication fails with “no ssh-agent available”.", a: "An app opened from the desktop menu often does not see the SSH_AUTH_SOCK your terminal has. Enter the agent's socket under Settings, Connections, Hardware keys and the system SSH agent.", doc: { slug: "troubleshooting", label: "Troubleshooting" } },
      { q: "The terminal is blank or garbled on Linux.", a: "Some GPU and driver combinations misbehave with WebGL in WebKitGTK. Try starting with WEBKIT_DISABLE_DMABUF_RENDERER=1; xterm.js falls back to a slower renderer when WebGL is unavailable.", doc: { slug: "troubleshooting", label: "Troubleshooting" } },
      { q: "A forwarding rule says “cannot listen on …”.", a: "Another program is using that port. Pick another one, or use 0 to have a free port chosen.", doc: { slug: "port-forwarding", label: "Port forwarding" } },
    ],
  },
];
