import { expect, test } from "@playwright/test";
import { B } from "./helpers";

const PAGES: [string, RegExp][] = [
  ["/", /terminals/i],
  ["/features", /What SSHVault does today/],
  ["/tutorials", /Learn by watching/],
  ["/docs", /SSHVault docs/],
  ["/docs/getting-started", /Getting started/],
  ["/downloads", /Download SSHVault/],
  ["/faq", /Questions, answered/],
];

test.describe("pages", () => {
  for (const [path, h1] of PAGES) {
    test(`${path} renders`, async ({ page }) => {
      const errors: string[] = [];
      page.on("pageerror", (e) => errors.push(e.message));
      page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
      await page.goto(`${B}${path}`);
      await expect(page.locator("h1").first()).toContainText(h1);
      expect(errors).toEqual([]);
    });
  }

  test("an unknown path shows the not-found page", async ({ page }) => {
    await page.goto(`${B}/no-such-page`);
    await expect(page.locator("h1")).toContainText("That page is not here");
  });

  test("direct navigation to a deep docs route works", async ({ page }) => {
    const res = await page.goto(`${B}/docs/troubleshooting`);
    expect(res?.status()).toBe(200);
    await expect(page.locator("h1").first()).toContainText("Troubleshooting");
    await expect(page).toHaveTitle(/Troubleshooting/);
  });

  test("sitemap and robots exist and list the docs", async ({ request }) => {
    const sm = await request.get(`${B}/sitemap.xml`);
    expect(sm.ok()).toBeTruthy();
    expect(await sm.text()).toContain("/docs/getting-started");
    const robots = await request.get(`${B}/robots.txt`);
    expect(await robots.text()).toContain("Sitemap:");
  });
});

test.describe("navigation", () => {
  test("header links reach every section", async ({ page }) => {
    await page.goto(`${B}/`);
    const nav = page.getByRole("navigation", { name: "Main" });
    for (const [name, h1] of [["Features", /What SSHVault/], ["Tutorials", /Learn by watching/], ["Docs", /SSHVault docs/], ["Downloads", /Download SSHVault/], ["FAQ", /Questions/]] as const) {
      await nav.getByRole("link", { name }).click();
      await expect(page.locator("h1").first()).toContainText(h1);
    }
  });

  test("mobile menu opens, navigates and closes", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 800 });
    await page.goto(`${B}/`);
    await page.getByRole("button", { name: "Open menu" }).click();
    await page.getByRole("navigation", { name: "Mobile" }).getByRole("link", { name: "Tutorials" }).click();
    await expect(page.locator("h1").first()).toContainText("Learn by watching");
    await expect(page.getByRole("navigation", { name: "Mobile" })).toHaveCount(0);
  });

  test("theme toggle switches and persists", async ({ page }) => {
    await page.goto(`${B}/`);
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.getByRole("button", { name: /Switch to light theme/ }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  });

  test("downloads links point at the GitHub release assets", async ({ page }) => {
    await page.goto(`${B}/downloads`);
    await expect(page.locator('a[href*="/releases/download/"]')).toHaveCount(6);
    const hrefs = await page.locator('a[href*="/releases/download/"]').evaluateAll((a) => a.map((x) => (x as HTMLAnchorElement).href));
    expect(hrefs).toHaveLength(6);
    for (const h of hrefs) expect(h).toMatch(/^https:\/\/github\.com\/aliysefian\/termius\/releases\/download\/v\d+\.\d+\.\d+\/SSHVault[_-]/);
  });

  test("every internal link on the main pages resolves to a real page", async ({ page }) => {
    const seen = new Set<string>();
    const pages = ["/", "/features", "/tutorials", "/docs", "/downloads", "/faq", "/docs/getting-started", "/docs/troubleshooting", "/tutorials/sftp-file-transfer"];
    for (const p of pages) {
      await page.goto(`${B}${p}`);
      await expect(page.locator("h1").first()).toBeVisible();
      const hrefs = await page.locator("a[href]").evaluateAll((a) => a.map((x) => x.getAttribute("href") ?? ""));
      for (const h of hrefs) if (h.startsWith("/termius/") || h.startsWith("/docs") || h.startsWith("/tutorials")) seen.add(h.split("#")[0]!);
    }
    expect(seen.size).toBeGreaterThan(20);
    for (const href of seen) {
      const url = href.startsWith(B) ? href : `${B}${href}`;
      await page.goto(url);
      await expect(page.locator("h1").first(), url).not.toContainText("That page is not here");
    }
  });
});

test.describe("docs", () => {
  test("sidebar, table of contents, breadcrumbs and prev/next", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(`${B}/docs/hosts`);
    await expect(page.getByRole("navigation", { name: "Documentation" })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "On this page" })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("Hosts");
    await page.getByRole("navigation", { name: "Previous and next pages" }).getByRole("link").last().click();
    await expect(page).toHaveURL(/\/docs\/connecting$/);
  });

  test("a deep link to a heading scrolls to it", async ({ page }) => {
    await page.goto(`${B}/docs/troubleshooting`);
    const id = await page.locator("article h3[id]").first().getAttribute("id");
    await page.goto(`${B}/docs/troubleshooting#${id}`);
    await expect.poll(async () => page.locator(`#${id}`).evaluate((e) => e.getBoundingClientRect().top < 400)).toBeTruthy();
  });

  test("code blocks have a working copy button", async ({ page, context }) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    await page.goto(`${B}/docs/getting-started`);
    const block = page.locator("article pre").first();
    await block.hover();
    await block.locator("xpath=..").getByRole("button", { name: "Copy code" }).click();
    await expect(page.getByText("Copied to clipboard")).toBeAttached();
  });

  test("the mobile docs drawer opens and navigates", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 800 });
    await page.goto(`${B}/docs/hosts`);
    await expect(page.locator("h1").first()).toBeVisible();
    await page.getByRole("button", { name: "Open documentation menu" }).click();
    await page.getByRole("dialog").getByRole("link", { name: "Terminal", exact: true }).click();
    await expect(page).toHaveURL(/\/docs\/terminal$/);
  });

  test("search finds a doc (Ctrl+K)", async ({ page }) => {
    await page.goto(`${B}/`);
    await expect(page.locator("h1")).toBeVisible();
    await page.keyboard.press("Control+k");
    await page.getByRole("combobox", { name: "Search" }).fill("tunnel");
    await expect(page.getByRole("option").first()).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/(docs|tutorials)\//);
  });
});

test.describe("tutorial library", () => {
  test("filters by feature, difficulty and text", async ({ page }) => {
    await page.goto(`${B}/tutorials`);
    await expect(page.getByRole("status")).toContainText("8 of 8");
    await page.getByLabel("Difficulty").selectOption("Beginner");
    const beginner = await page.locator("ul li a h2").count();
    expect(beginner).toBeGreaterThan(0);
    expect(beginner).toBeLessThan(8);
    await page.getByLabel("Difficulty").selectOption("all");
    await page.getByLabel("Feature").selectOption("tmux");
    await expect(page.locator("ul li a h2")).toHaveCount(1);
    await page.getByLabel("Feature").selectOption("all");
    await page.getByRole("textbox", { name: "Search tutorials" }).fill("upload");
    await expect(page.locator("ul li a h2")).toContainText(["Transfer files with SFTP"]);
  });
});
