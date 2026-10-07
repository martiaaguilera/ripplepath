import { expect, test } from "@playwright/test";

test("change page explains the blast radius of the demo change", async ({ page }) => {
  await page.goto("/?base=main~1&head=main");

  await expect(page.getByRole("region", { name: "Summary" })).toContainText("Changed symbols");
  await expect(page.locator(".node").first()).toBeVisible();

  // The changed implementation reaches the controller test through dispatch and calls.
  await page.getByRole("button", { name: /TransferControllerTest\.returnsOkOnSuccessfulTransfer/ }).first().click();
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await expect(inspector).toContainText("impacted at depth");
  await expect(inspector.getByRole("list", { name: "Explaining path from the changed symbol" })).toBeVisible();

  // Every finding is also available as text, not only in the graph.
  await page.getByRole("tab", { name: /Uncertainty/ }).click();
  await expect(page.getByRole("tabpanel")).toContainText("not in a supported language");
});

test("unknown revisions produce a readable error", async ({ page }) => {
  await page.goto("/?base=no-such-branch&head=main");
  await expect(page.getByRole("alert")).toContainText("no-such-branch");
});
