// Offline export: renders a tutorial to PNG frames, then to MP4/WebM with FFmpeg when it is installed.
// The site never needs this. Every frame is a pure function of the playhead time (Timeline.stateAt), so output is deterministic.
//
//   npm run build
//   npm run export:videos -- --id create-ssh-connection --fps 30        (one tutorial)
//   npm run export:videos -- --all
//   options: --scale 2 (device pixel ratio)  --out .export  --frames-only
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { chromium } from "@playwright/test";

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? (args[i + 1]?.startsWith("--") || args[i + 1] === undefined ? true : args[i + 1]) : fallback;
};
const fps = Number(opt("fps", 30));
const scale = Number(opt("scale", 1));
const outDir = String(opt("out", ".export"));
const framesOnly = Boolean(opt("frames-only", false));
const PORT = 4174;

const { tutorials } = await import("../src/tutorials/registry.ts");
const ids = opt("all", false) ? tutorials.map((t) => t.id) : [String(opt("id", "create-ssh-connection"))];

function chromiumPath() {
  if (process.env.PLAYWRIGHT_CHROMIUM_PATH) return process.env.PLAYWRIGHT_CHROMIUM_PATH;
  const root = join(homedir(), ".cache", "ms-playwright");
  if (!existsSync(root)) return undefined;
  for (const d of readdirSync(root).filter((x) => x.startsWith("chromium")).sort().reverse())
    for (const sub of ["chrome-headless-shell-linux64/chrome-headless-shell", "chrome-linux64/chrome"]) if (existsSync(join(root, d, sub))) return join(root, d, sub);
}

const server = spawn("npx", ["vite", "preview", "--port", String(PORT), "--strictPort"], { stdio: "ignore" });
const stop = () => server.kill();
process.on("exit", stop);
await new Promise((r) => setTimeout(r, 2500));

const hasFfmpeg = spawnSync("ffmpeg", ["-version"]).status === 0;
const browser = await chromium.launch({ executablePath: chromiumPath() });
try {
  for (const id of ids) {
    const page = await browser.newPage({ viewport: { width: 1100, height: 640 }, deviceScaleFactor: scale });
    await page.goto(`http://localhost:${PORT}/termius/render/${id}`);
    await page.waitForSelector("html[data-ready=true]");
    const duration = await page.evaluate((id) => window.__demoPlayers[id].timeline.duration, id);
    const dir = join(outDir, id);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    const stage = page.getByTestId("sim-stage");
    const frames = Math.ceil((duration / 1000) * fps);
    for (let f = 0; f <= frames; f++) {
      await page.evaluate(async ([id, t]) => {
        window.__demoPlayers[id].seek(t);
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      }, [id, Math.min(duration, (f / fps) * 1000)]);
      await stage.screenshot({ path: join(dir, `frame-${String(f).padStart(5, "0")}.png`) });
    }
    console.log(`${id}: ${frames + 1} frames in ${dir}`);
    await page.close();
    if (framesOnly || !hasFfmpeg) {
      if (!hasFfmpeg) console.log("ffmpeg not found: frames kept; install ffmpeg and rerun without --frames-only to encode.");
      continue;
    }
    const input = ["-y", "-framerate", String(fps), "-i", join(dir, "frame-%05d.png")];
    spawnSync("ffmpeg", [...input, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", join(outDir, `${id}.mp4`)], { stdio: "inherit" });
    spawnSync("ffmpeg", [...input, "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "34", join(outDir, `${id}.webm`)], { stdio: "inherit" });
  }
} finally {
  await browser.close();
  stop();
}
