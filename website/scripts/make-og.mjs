// Regenerates public/og.png (1200x630 social card). Run: node scripts/make-og.mjs
import { chromium } from "@playwright/test";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const logo = readFileSync(new URL("../public/logo.svg", import.meta.url), "utf8");
const root = join(homedir(), ".cache", "ms-playwright");
const exe = process.env.PLAYWRIGHT_CHROMIUM_PATH ?? (existsSync(root) ? readdirSync(root).filter((d) => d.startsWith("chromium")).map((d) => join(root, d, "chrome-headless-shell-linux64/chrome-headless-shell")).find(existsSync) : undefined);
const b = await chromium.launch({ executablePath: exe });
const p = await b.newPage({ viewport: { width: 1200, height: 630 } });
await p.setContent(`<body style="margin:0;width:1200px;height:630px;background:radial-gradient(900px 500px at 50% 0,#2a1f6b,#0a0f19 70%);color:#e8edf5;font-family:Inter,system-ui,sans-serif;display:flex;flex-direction:column;justify-content:center;padding:0 90px;box-sizing:border-box">
<div style="width:96px;height:96px">${logo}</div>
<h1 style="font-size:76px;line-height:1.05;margin:36px 0 18px;letter-spacing:-2px">SSHVault</h1>
<p style="font-size:34px;color:#9aa8bd;margin:0;max-width:900px">An SSH client for people who live in terminals, with an encrypted vault you sync yourself.</p></body>`);
await p.screenshot({ path: new URL("../public/og.png", import.meta.url).pathname });
await b.close();
