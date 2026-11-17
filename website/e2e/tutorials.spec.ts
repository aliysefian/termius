import { expect, test } from "@playwright/test";
import { B, measureCursorAlignment, TUTORIALS, waitForPlayer } from "./helpers";

const TOLERANCE = 12; // CSS pixels

for (const id of TUTORIALS) {
  test.describe(id, () => {
    test("plays to the end and the pointer lands on every target", async ({ page }) => {
      const errors: string[] = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.goto(`${B}/tutorials/${id}`);
      await waitForPlayer(page, id);
      const probes = await measureCursorAlignment(page, id);
      expect(probes.length).toBeGreaterThan(4);
      const missing = probes.filter((p) => !p.found);
      expect(missing, `targets missing from the DOM: ${JSON.stringify(missing)}`).toEqual([]);
      const off = probes.filter((p) => Math.hypot(p.dx, p.dy) > TOLERANCE);
      expect(off, `cursor off target: ${JSON.stringify(off)}`).toEqual([]);
      expect(errors).toEqual([]);
    });

    test("alignment holds on a phone-sized viewport", async ({ page }) => {
      await page.setViewportSize({ width: 390, height: 800 });
      await page.goto(`${B}/tutorials/${id}`);
      await waitForPlayer(page, id);
      const probes = await measureCursorAlignment(page, id);
      // Scaled down to ~0.33, a 12px tolerance is 36 stage pixels; hold it to 6px instead.
      const off = probes.filter((p) => Math.hypot(p.dx, p.dy) > 6);
      expect(off, JSON.stringify(off)).toEqual([]);
    });
  });
}

test.describe("playback controls", () => {
  const id = "create-ssh-connection";
  test.beforeEach(async ({ page }) => {
    await page.goto(`${B}/tutorials/${id}`);
    await waitForPlayer(page, id);
  });

  test("pause and resume keep the position", async ({ page }) => {
    const time = () => page.evaluate((id) => window.__demoPlayers![id]!.time, id);
    await page.getByRole("button", { name: "Pause" }).or(page.getByRole("button", { name: "Play" })).first().click();
    await page.evaluate((id) => window.__demoPlayers![id]!.pause(), id);
    const t0 = await time();
    await page.waitForTimeout(500);
    expect(await time()).toBe(t0);
    await page.getByRole("button", { name: "Play" }).click();
    await expect.poll(time).toBeGreaterThan(t0);
    await page.getByRole("button", { name: "Pause" }).click();
    const t1 = await time();
    await page.waitForTimeout(400);
    expect(await time()).toBe(t1);
  });

  test("restart returns to the start and plays", async ({ page }) => {
    await page.evaluate((id) => window.__demoPlayers![id]!.seek(20000), id);
    await page.getByRole("button", { name: "Restart from the beginning" }).click();
    await expect.poll(() => page.evaluate((id) => window.__demoPlayers![id]!.time, id)).toBeLessThan(3000);
    expect(await page.evaluate((id) => window.__demoPlayers![id]!.status, id)).toBe("playing");
  });

  test("speed changes how fast the clock runs", async ({ page }) => {
    await page.evaluate((id) => window.__demoPlayers![id]!.pause(), id);
    await page.evaluate((id) => window.__demoPlayers![id]!.seek(0), id);
    await page.getByLabel("Playback speed").selectOption("2");
    await page.getByRole("button", { name: "Play" }).click();
    await page.waitForTimeout(1000);
    const t = await page.evaluate((id) => window.__demoPlayers![id]!.time, id);
    expect(t).toBeGreaterThan(1400);
  });

  test("step buttons and the scrubber seek, and the UI follows", async ({ page }) => {
    await page.evaluate((id) => window.__demoPlayers![id]!.pause(), id);
    await page.getByRole("button", { name: "Next step" }).click();
    const t1 = await page.evaluate((id) => window.__demoPlayers![id]!.time, id);
    expect(t1).toBeGreaterThan(0);
    await page.getByRole("slider", { name: "Seek the tutorial" }).fill("40000");
    // At 40 s the demo is in a terminal tab, connected.
    await expect(page.getByTestId("sim-stage")).toHaveAttribute("data-sim-view", "terminal");
    await page.getByRole("slider", { name: "Seek the tutorial" }).fill("1000");
    await expect(page.getByTestId("sim-stage")).toHaveAttribute("data-sim-view", "hosts");
  });

  test("clicking a written step jumps the animation there", async ({ page }) => {
    await page.getByRole("button", { name: /Run a command/ }).click();
    const t = await page.evaluate((id) => window.__demoPlayers![id]!.time, id);
    expect(t).toBeGreaterThan(30000);
  });

  test("the Space key toggles playback when the player is focused", async ({ page }) => {
    const root = page.getByTestId("tutorial-player");
    await root.focus();
    const before = await page.evaluate((id) => window.__demoPlayers![id]!.status, id);
    await page.keyboard.press("Space");
    const after = await page.evaluate((id) => window.__demoPlayers![id]!.status, id);
    expect(after).not.toBe(before);
  });

  test("the written steps mark the current one", async ({ page }) => {
    await page.evaluate((id) => {
      const p = window.__demoPlayers![id]!;
      p.pause();
      p.seek(p.timeline.chapterStart(2) + 100);
    }, id);
    await expect(page.locator('[aria-current="step"]')).toContainText("Save the host");
  });
});

test.describe("lifecycle", () => {
  test("leaving a tutorial stops its clock and cancels its frame request", async ({ page }) => {
    const id = "tmux-sessions";
    await page.goto(`${B}/tutorials/${id}`);
    await waitForPlayer(page, id);
    await page.evaluate((id) => window.__demoPlayers![id]!.play(), id);
    const handle = await page.evaluateHandle((id) => window.__demoPlayers![id]!, id);
    await page.getByRole("link", { name: "FAQ" }).first().click();
    await expect(page.locator("h1").first()).toContainText("Questions");
    const t0 = await handle.evaluate((p) => p.time);
    await page.waitForTimeout(600);
    const state = await handle.evaluate((p) => ({ t: p.time, status: p.status, raf: (p as unknown as { handle: number | null }).handle }));
    expect(state.t).toBe(t0);
    expect(state.status).not.toBe("playing");
    expect(state.raf).toBeNull();
    expect(await page.evaluate(() => Object.keys(window.__demoPlayers ?? {}))).toEqual([]);
  });

  test("replaying many times does not accumulate players or errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    const id = "host-management";
    await page.goto(`${B}/tutorials/${id}`);
    await waitForPlayer(page, id);
    for (let i = 0; i < 6; i++) {
      await page.getByRole("button", { name: "Restart from the beginning" }).click();
      await page.waitForTimeout(80);
    }
    expect(await page.evaluate(() => Object.keys(window.__demoPlayers ?? {}).length)).toBe(1);
    expect(errors).toEqual([]);
  });
});

test.describe("autoplay policy", () => {
  test("the home loop plays when visible", async ({ page }) => {
    await page.goto(`${B}/`);
    await waitForPlayer(page, "hero");
    await expect.poll(() => page.evaluate(() => window.__demoPlayers!.hero!.time)).toBeGreaterThan(500);
  });

  test("reduced motion shows a static poster and does not autoplay", async ({ browser }) => {
    const ctx = await browser.newContext({ reducedMotion: "reduce" });
    const page = await ctx.newPage();
    await page.goto(`${B}/`);
    await waitForPlayer(page, "hero");
    await page.waitForTimeout(1200);
    const s = await page.evaluate(() => {
      const p = window.__demoPlayers!.hero!;
      return { status: p.status, time: p.time, poster: p.timeline.posterTime };
    });
    expect(s.status).not.toBe("playing");
    expect(s.time).toBe(s.poster);
    // Manual control still works.
    await page.getByRole("button", { name: "Play demonstration" }).first().click();
    await expect.poll(() => page.evaluate(() => window.__demoPlayers!.hero!.status)).toBe("playing");
    await ctx.close();
  });

  test("clips scrolled out of view are not playing", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(`${B}/features`);
    await page.waitForTimeout(800);
    const playing = await page.evaluate(() => Object.values(window.__demoPlayers ?? {}).filter((p) => p.status === "playing").length);
    expect(playing).toBeLessThanOrEqual(2);
    await page.locator("#tunnels").scrollIntoViewIfNeeded();
    await page.waitForTimeout(1000);
    const after = await page.evaluate(() => Object.values(window.__demoPlayers ?? {}).filter((p) => p.status === "playing").length);
    expect(after).toBeLessThanOrEqual(3);
  });

  test("a hidden tab pauses playback", async ({ page }) => {
    await page.goto(`${B}/`);
    await waitForPlayer(page, "hero");
    await expect.poll(() => page.evaluate(() => window.__demoPlayers!.hero!.status)).toBe("playing");
    await page.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect.poll(() => page.evaluate(() => window.__demoPlayers!.hero!.status)).toBe("paused");
  });
});
