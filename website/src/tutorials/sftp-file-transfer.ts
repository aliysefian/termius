import type { TutorialDefinition } from "@/engine/types";
import { click, dblclick, move, progress, set, toast, wait } from "./helpers";

const def: TutorialDefinition = {
  id: "sftp-file-transfer",
  initial: { view: "hosts" },
  cursorStart: { x: 0.85, y: 0.85 },
  posterTime: 33000,
  steps: [
    wait(700, { chapter: "Open the file manager", note: "The Files page has two panes: this computer on the left, the remote host on the right." }),
    move("rail-sftp", 900),
    click("rail-sftp", { view: "sftp" }),
    wait(600),
    move("sftp-host-picker", 850),
    click("sftp-host-picker", { selected: { sftpHost: "Production API" } }),
    wait(500),
    move("sftp-connect", 700),
    click("sftp-connect", { flags: { sftpConnected: true } }),
    toast("Connected to Production API over SFTP", 1500),
    wait(400),

    move("remote-releases", 850, { chapter: "Navigate folders", note: "Double-click a folder to open it, or use the breadcrumb to jump back." }),
    dblclick("remote-releases", { selected: { remoteDir: "releases" } }),
    wait(1100),
    move("remote-..", 700),
    dblclick("remote-..", { selected: { remoteDir: "root" } }),
    wait(600),

    move("local-release-notes.md", 900, { chapter: "Select a file", note: "Select files with click, Ctrl+click or Shift+click." }),
    click("local-release-notes.md", { selected: { local: "release-notes.md" } }),
    wait(700),

    move("sftp-upload", 800, { chapter: "Upload with progress", note: "Upload, or drag the file across. A transfer queue shows progress and lets you pause or cancel." }),
    click("sftp-upload", { progress: { upload: 1 } }),
    progress("upload", 1, 100, 2600),
    set({ flags: { uploaded: true } }),
    toast("Uploaded release-notes.md (4.2 KB)", 1900),
    wait(400),

    move("remote-release-notes.md", 850, { chapter: "Check the result", note: "The file is now on the host. Right-click it for rename, permissions, download and delete." }),
    click("remote-release-notes.md", { selected: { remote: "release-notes.md" } }),
    wait(500),
    click("remote-release-notes.md", { menu: "file-menu" }, { duration: 300 }),
    move("menu-permissions", 700),
    wait(1400),
    set({ menu: null }),
    wait(1000),
  ],
};

export default def;
