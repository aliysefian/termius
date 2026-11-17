/**
 * Verified against the GitHub release API for v0.62.0. Update this file when you publish a release (see the README).
 * Never add an asset that is not on the release page.
 */
export const REPO_URL = "https://github.com/aliysefian/termius";
export const RELEASES_URL = `${REPO_URL}/releases`;
export const LATEST_URL = `${REPO_URL}/releases/latest`;
export const VERIFIED_VERSION = "0.62.0";

const dl = (file: string) => `${REPO_URL}/releases/download/v${VERIFIED_VERSION}/${file}`;

export interface Installer {
  label: string;
  file: string;
  url: string;
  how: string;
  note?: string;
}

export interface Platform {
  id: "windows" | "macos" | "linux";
  name: string;
  requirement: string;
  installers: Installer[];
  notes: string[];
}

export const platforms: Platform[] = [
  {
    id: "windows",
    name: "Windows",
    requirement: "Windows 10 or 11 (x64)",
    installers: [
      { label: "Installer (.exe)", file: `SSHVault_${VERIFIED_VERSION}_x64-setup.exe`, url: dl(`SSHVault_${VERIFIED_VERSION}_x64-setup.exe`), how: "Run it." },
      { label: "MSI package", file: `SSHVault_${VERIFIED_VERSION}_x64_en-US.msi`, url: dl(`SSHVault_${VERIFIED_VERSION}_x64_en-US.msi`), how: "Run it, or deploy it with your management tools." },
    ],
    notes: ["The installers are not code-signed yet, so SmartScreen may warn: choose More info, then Run anyway.", "The WebView2 runtime is part of Windows 10 and 11; the installer fetches it if it is missing."],
  },
  {
    id: "macos",
    name: "macOS",
    requirement: "Apple Silicon and Intel (universal)",
    installers: [{ label: "Disk image (.dmg)", file: `SSHVault_${VERIFIED_VERSION}_universal.dmg`, url: dl(`SSHVault_${VERIFIED_VERSION}_universal.dmg`), how: "Open it and drag SSHVault to Applications." }],
    notes: [
      "Not signed with an Apple Developer ID yet, so macOS blocks the first launch: open System Settings, Privacy & Security, then Open Anyway (older macOS: right-click the app, then Open).",
      "If macOS says the app is damaged, run xattr -dr com.apple.quarantine /Applications/SSHVault.app once.",
      "Updates are not applied in place on macOS: the app opens the releases page and you install the new .dmg.",
    ],
  },
  {
    id: "linux",
    name: "Linux",
    requirement: "glibc 2.35 or newer (Ubuntu 22.04 and later)",
    installers: [
      { label: "Debian, Ubuntu, Mint, Pop!_OS (.deb)", file: `SSHVault_${VERIFIED_VERSION}_amd64.deb`, url: dl(`SSHVault_${VERIFIED_VERSION}_amd64.deb`), how: `sudo apt install ./SSHVault_${VERIFIED_VERSION}_amd64.deb` },
      { label: "Fedora, RHEL, openSUSE (.rpm)", file: `SSHVault-${VERIFIED_VERSION}-1.x86_64.rpm`, url: dl(`SSHVault-${VERIFIED_VERSION}-1.x86_64.rpm`), how: `sudo dnf install ./SSHVault-${VERIFIED_VERSION}-1.x86_64.rpm` },
      { label: "Any distribution (AppImage)", file: `SSHVault_${VERIFIED_VERSION}_amd64.AppImage`, url: dl(`SSHVault_${VERIFIED_VERSION}_amd64.AppImage`), how: `chmod +x SSHVault_${VERIFIED_VERSION}_amd64.AppImage && ./SSHVault_${VERIFIED_VERSION}_amd64.AppImage` },
    ],
    notes: ["The AppImage and Windows builds can update themselves once signed updates are published; .deb and .rpm show a download link."],
  },
];
