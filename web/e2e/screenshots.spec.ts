import { expect, test, type Page } from "@playwright/test";

// Regenerates the README screenshots from a real `ripplepath serve` (no mock data). Skipped unless
// RIPPLEPATH_SCREENSHOTS names the output directory, so the CI smoke run never writes files.
// The published images use the java-banking demo with its recorded evidence ingested
// (fixtures/java-banking/evidence); see docs/assets/README.md.
const out = process.env.RIPPLEPATH_SCREENSHOTS;
const DEMO = "/?base=main~1&head=main";

test.skip(!out, "set RIPPLEPATH_SCREENSHOTS=<dir> to write screenshots");
test.use({ viewport: { width: 1600, height: 1000 }, colorScheme: "light", deviceScaleFactor: 1 });

async function settle(page: Page) {
  // The graph lays out asynchronously and fits the viewport afterwards.
  await expect(page.locator(".graph--stale")).toHaveCount(0);
  await page.waitForTimeout(400);
}

async function shoot(page: Page, name: string) {
  await page.screenshot({ path: `${out ?? "."}/${name}.png` });
}

test("change overview", async ({ page }) => {
  await page.goto(DEMO);
  await expect(page.locator(".react-flow .node").first()).toBeVisible();
  await settle(page);
  await shoot(page, "change-overview");
});

test("blast radius with an evidence path highlighted", async ({ page }) => {
  await page.goto(DEMO);
  await expect(page.locator(".react-flow .node").first()).toBeVisible();
  await page.getByRole("button", { name: /TransferControllerTest\.returnsOkOnSuccessfulTransfer/ }).first().click();
  await expect(page.locator(".node--selected")).toBeVisible();
  // The click lands in the Tests tab below the graph; bring the graph back into view.
  await page.evaluate(() => {
    window.scrollTo(0, 0);
  });
  await settle(page);
  await shoot(page, "blast-radius-path");
});

test("test evidence path", async ({ page }) => {
  await page.goto(`${DEMO}&view=tests&test=${encodeURIComponent("java:com.acme.bank.application.TransferServiceTest")}`);
  await expect(page.getByRole("list", { name: /Evidence path/ })).toContainText("measured by coverage");
  await shoot(page, "test-evidence");
});

test("architecture drift", async ({ page }) => {
  await page.goto(`${DEMO}&view=architecture`);
  const violations = page.getByRole("table", { name: /Layer-rule violations/ });
  await violations.getByRole("row", { name: /withdraw/ }).getByRole("button").click();
  await page.getByRole("radio", { name: "Base → head" }).check({ force: true });
  await shoot(page, "architecture-drift");
});

test("risk decomposition", async ({ page }) => {
  await page.goto(`${DEMO}&view=risk`);
  await expect(page.getByRole("table", { name: /Risk signals/ })).toBeVisible();
  await shoot(page, "risk-decomposition");
});
