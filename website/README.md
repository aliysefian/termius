# SSHVault website

Static marketing site, documentation portal and tutorial platform for [SSHVault](https://github.com/aliysefian/termius), deployed to
<https://aliysefian.github.io/termius/>. It is a separate project: its own `package.json`, lockfile and build. It never touches the desktop app
(the only changes outside this folder are a one-line test exclusion in the root `vite.config.js`, a `paths-ignore` entry in
`.github/workflows/build.yml`, and `.github/workflows/deploy-website.yml`).

Stack: React 19, TypeScript, Vite, Tailwind CSS 4, shadcn/ui-style components (Radix + cva), Lucide, Motion, MDX, Pagefind, Vitest, Playwright.

## Run it

```bash
cd website
npm ci
npm run dev          # http://localhost:5173/termius/
npm run validate     # typecheck + lint + unit/component tests + production build
npm run test:e2e     # builds, serves, and runs the Playwright suite (smoke, tutorials, axe, responsive)
npm run preview      # serve dist/ at http://localhost:4173/termius/
```

If Playwright cannot download Chromium, set `PLAYWRIGHT_CHROMIUM_PATH` to a Chromium/`chrome-headless-shell` binary (a copy cached under
`~/.cache/ms-playwright` is found automatically). Search (Pagefind) only exists after `npm run build`; in `dev` a built-in fallback searches titles.

## Layout

```
src/engine/       Timeline (compiled steps), pure state reducer, Player (single rAF clock). No React.
src/components/sim/       The simulated SSHVault UI, rendered from a DemoState. DemoStage scales it and drives the cursor.
src/components/tutorial/  TutorialPlayer, Controls, Timeline, StepDescription, AnimatedProductWindow.
src/tutorials/    One file per tutorial + registry.ts (metadata, written steps, lazy loaders).
src/data/         Fictional fixtures, features, FAQ, release assets, docs index.
src/content/docs/ MDX documentation (frontmatter: title, description, section, order).
scripts/          postbuild (per-route HTML, 404, sitemap, robots), export-videos, make-og, docs-meta.
e2e/ tests/       Playwright and Vitest suites.
```

## How the animation engine works

A tutorial is a list of declarative steps (`moveCursor`, `click`, `doubleClick`, `type`, `wait`, `showPanel`, `terminalOutput`, `highlight`,
`set`, `toast`, `progress`). `Timeline` gives every step an absolute start and end and stores the UI state after each one. The UI state
at any time `t` is a pure function, `timeline.stateAt(t)`; there are no chained timeouts. `Player` owns one `requestAnimationFrame` loop that
advances `t` by elapsed time x speed, so pause, resume, restart, speed, seeking and replay all just change `t`. Seeking therefore always
reconstructs the exact UI. Unmounting calls `player.destroy()`, which cancels the pending frame.

The simulated window is laid out at a fixed 1100 x 640 and scaled with a CSS transform. Targets are elements marked `data-demo="id"`; the cursor
is placed at the target's normalised position (optionally `id@fx,fy` for a point inside it), measured each frame, so it stays aligned at any width.
Playback starts only while the demo is on screen and the tab is visible, and never with `prefers-reduced-motion` (a poster frame and manual controls instead).

All data is fictional (RFC 5737 addresses, `demo` users). No demo can run a command: terminal output is predefined text.

## Add a tutorial

1. Create `src/tutorials/my-tutorial.ts` exporting a `TutorialDefinition` (copy `port-forwarding.ts`). Use `makeHelpers(pace)` from `helpers.ts`;
   pace 1.5-2.2 gives 30-90 s. Put `chapter: "Title"` on the first step of each chapter and `note: "..."` for captions.
2. If it needs UI the simulation lacks, add a view or dialog in `src/components/sim/` (give interactive elements `data-demo` ids) and route it in `SimApp.tsx`.
3. Add an entry to `tutorials` and `loaders` in `src/tutorials/registry.ts`: title, description, difficulty, category, `durationSec`,
   written `steps` (titles must equal the chapter titles, in order), docs links and related ids.
4. `npm test` checks chapters, duration (30-90 s, within 3 s of `durationSec`) and determinism; `npm run test:e2e` checks that the cursor lands on every target.
5. Optional: reference a chapter from `src/data/features.ts` to show it as a looping clip on the Features page.

## Add a doc page

Add `src/content/docs/<slug>.mdx` with frontmatter (`title`, `description`, `section` one of Start/Connect/Work/Secure/Help, `order`). Headings get anchors and the
table of contents automatically; `<Callout type="note|warning|tip">` is available. Keep claims traceable to the app's own docs.

## Releases and downloads

`src/data/releases.ts` lists the installers of the release it was checked against (v0.62.0). After publishing a release, update `VERIFIED_VERSION`
and the file names to match the assets on the release page; the "Latest release" button always works without edits.

## Deploy (GitHub Pages)

`.github/workflows/deploy-website.yml` validates, builds and deploys on pushes to `main` that touch `website/**`; pull requests only validate.
A maintainer must enable it once: **Settings > Pages > Source: GitHub Actions**. The workflow does not change settings itself.
The base path is `/termius/` (`VITE_BASE`). The build writes a real `index.html` per route and a `404.html`, so deep links work.

Custom domain later: add `website/public/CNAME` containing the domain, set repository variable/env `VITE_BASE=/` and `SITE_URL=https://your.domain` for the build step, and set the domain in Pages settings.

## Export tutorials to video (optional)

The site does not depend on this. `/render/<id>` is a bare stage whose clock is never started; the exporter seeks to each frame and screenshots it, so the
result is deterministic.

```bash
npm run build
npm run export:videos -- --id create-ssh-connection --fps 30   # frames in .export/, MP4 + WebM if ffmpeg is installed
npm run export:videos -- --all --scale 2
```

## Known limits

- The simulation is a hand-built replica, not a recording; it can drift from the real UI and says so on every player.
- On phone widths the 1100 px window scales to about a third, so text inside it is small; the written steps carry the content.
- If the cursor's previous target element has disappeared when you seek straight into a move, it starts from the default start position for that move.
- `releases.ts` is static and must be updated by hand per release.
