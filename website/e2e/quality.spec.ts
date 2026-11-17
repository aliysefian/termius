import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { B } from "./helpers";

const PAGES = ["/", "/features", "/tutorials", "/tutorials/sftp-file-transfer", "/docs", "/docs/security", "/downloads", "/faq"];

test.describe("accessibility (axe, WCAG 2.2 AA)", () => {
  for (const theme of ["dark", "light"] as const) {
    for (const path of PAGES) {
      test(`${path} (${theme})`, async ({ page }) => {
        await page.addInitScript((t) => localStorage.setItem("sshvault-theme", t), theme);
        await page.goto(`${B}${path}`);
        await page.waitForTimeout(600);
        const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
        const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
        expect(bad.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`)).toEqual([]);
      });
    }
  }
});

test.describe("responsive layout", () => {
  const sizes = [
    [360, 740],
    [768, 1024],
    [1280, 800],
    [1920, 1080],
  ] as const;
  for (const [w, h] of sizes) {
    for (const path of ["/", "/features", "/tutorials/terminal-workflow", "/docs/hosts", "/downloads", "/faq"]) {
      test(`${path} has no horizontal overflow at ${w}px`, async ({ page }) => {
        await page.setViewportSize({ width: w, height: h });
        await page.goto(`${B}${path}`);
        await page.waitForTimeout(400);
        const over = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
        expect(over).toBeLessThanOrEqual(1);
      });
    }
  }

  test("the simulated window scales with its container and keeps its aspect ratio", async ({ page }) => {
    const ratios: number[] = [];
    for (const w of [390, 820, 1400]) {
      await page.setViewportSize({ width: w, height: 900 });
      await page.goto(`${B}/tutorials/host-management`);
      const box = await page.getByTestId("sim-stage").boundingBox();
      ratios.push(box!.width / box!.height);
    }
    for (const r of ratios) expect(r).toBeCloseTo(1100 / 640, 1);
  });
});

test.describe("keyboard", () => {
  test("skip link is the first tab stop and focuses main", async ({ page }) => {
    await page.goto(`${B}/faq`);
    await expect(page.locator("h1")).toBeVisible();
    await page.keyboard.press("Tab");
    await expect(page.getByRole("link", { name: "Skip to content" })).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.locator("#main")).toBeFocused();
  });

  test("the search dialog traps focus and Escape closes it", async ({ page }) => {
    await page.goto(`${B}/`);
    await expect(page.locator("h1")).toBeVisible();
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await expect(page.getByRole("combobox", { name: "Search" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Search", exact: true })).toBeFocused();
  });
});
