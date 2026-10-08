import { expect, test } from "@playwright/test";

// Runs against a real `ripplepath serve` on the java-banking demo (`ripplepath demo`), base main~1,
// head main. Assertions are about facts of that fixture, so they hold with or without ingested
// coverage and CI evidence.

const DEMO = "/?base=main~1&head=main";
const ACCOUNT = "src/main/java/com/acme/bank/domain/Account.java";

test("change view: verdicts, impact graph and an explained dependent", async ({ page }) => {
  await page.goto(DEMO);

  const verdicts = page.getByRole("region", { name: "Change summary" });
  await expect(verdicts).toContainText("not a probability");
  await expect(verdicts).toContainText("new violations");
  await expect(page.locator(".react-flow .node").first()).toBeVisible();

  // The changed implementation reaches the controller test through calls.
  await page.getByRole("button", { name: /TransferControllerTest\.returnsOkOnSuccessfulTransfer/ }).first().click();
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await expect(inspector).toContainText("impacted at depth");
  await expect(inspector.getByRole("list", { name: "Explaining path from the changed symbol" })).toBeVisible();
  await expect(page).toHaveURL(/sel=java%3Acom\.acme\.bank\.api\.TransferControllerTest/);

  // Every graph finding is also available as text, and the node list works from the keyboard.
  await page.getByRole("tab", { name: /Nodes/ }).click();
  const list = page.getByRole("listbox", { name: "Graph symbols" });
  await list.focus();
  await list.press("Home");
  await list.press("Enter");
  await expect(inspector).toContainText("changed");
  await page.getByRole("tab", { name: /Uncertainty/ }).click();
  await expect(page.getByRole("tabpanel")).toContainText("not in a supported language");
});

test("graph filters: evidence classes and module collapsing", async ({ page }) => {
  await page.goto(DEMO);
  await expect(page.locator(".react-flow .node").first()).toBeVisible();
  const symbols = await page.locator(".react-flow .node:not(.node--cluster)").count();
  await page.getByRole("checkbox", { name: "collapse modules" }).check();
  await expect(page.locator(".react-flow .node--cluster").first()).toBeVisible();
  expect(await page.locator(".react-flow .node:not(.node--cluster)").count()).toBeLessThan(symbols);
  await page.getByRole("checkbox", { name: "collapse modules" }).uncheck();
  await page.getByRole("checkbox", { name: "resolved exactly" }).uncheck();
  await page.getByRole("tab", { name: /Edges/ }).click();
  await expect(page.getByRole("tabpanel")).not.toContainText("resolved exactly");
});

test("diff view: real file contents with semantic hunk annotation", async ({ page }) => {
  await page.goto(`${DEMO}&view=diff&file=${encodeURIComponent(ACCOUNT)}`);
  const diff = page.getByRole("table", { name: /Unified diff/ });
  await expect(diff).toBeVisible();
  await expect(diff).toContainText("ApiErrors.accountFrozen");
  const hunk = diff.getByLabel("Hunk 1");
  await expect(hunk).toContainText("@@");
  await hunk.getByRole("button").first().click();
  await expect(page).toHaveURL(/[?&]sel=/);
  await expect(page).not.toHaveURL(/view=diff/);
  await expect(page.getByRole("complementary", { name: "Inspector" })).toContainText("changed");
});

test("tests view: the decision and every fallback reason", async ({ page }) => {
  await page.goto(`${DEMO}&view=tests`);
  await expect(page.getByRole("heading", { name: "Run the full suite" })).toBeVisible();
  await expect(page.getByText("MIGRATION_CHANGED")).toBeVisible();
  const ranked = page.getByRole("table", { name: /Recommended tests/ });
  await expect(ranked).toContainText("TransferControllerTest.returnsOkOnSuccessfulTransfer()");
  await ranked.getByRole("button", { name: /returnsOkOnSuccessfulTransfer/ }).click();
  await expect(page.getByRole("list", { name: /Evidence path/ })).toContainText("calls");
});

test("risk view: decomposition table and policy gates", async ({ page }) => {
  await page.goto(`${DEMO}&view=risk`);
  await expect(page.getByRole("note")).toContainText("not a probability of failure");
  const signals = page.getByRole("table", { name: /Risk signals/ });
  await expect(signals.getByRole("row", { name: /Migration changed/ })).toContainText("×15");
  await expect(page.getByRole("table", { name: /Merge-policy gates/ })).toContainText("new_architecture_violation");
});

test("architecture view: the new domain → api violation down to file and line", async ({ page }) => {
  await page.goto(`${DEMO}&view=architecture`);
  const violations = page.getByRole("table", { name: /Layer-rule violations/ });
  const calls = violations.getByRole("row", { name: /withdraw/ });
  await expect(calls).toContainText("new");
  await calls.getByRole("button").click();
  const detail = page.getByRole("complementary", { name: "Violation detail" });
  await expect(detail).toContainText("domain must not depend on");
  await expect(detail).toContainText(`${ACCOUNT}:25`);
  await page.getByRole("radio", { name: "Base", exact: true }).check({ force: true });
  await expect(page.getByRole("group", { name: /Layer diagram/ })).not.toContainText("domain → api");
  await detail.getByRole("button", { name: `${ACCOUNT}:25` }).click();
  await expect(page.locator("#diff-L25")).toHaveClass(/is-target/);
});

test("owners view says when there is no CODEOWNERS", async ({ page }) => {
  await page.goto(`${DEMO}&view=owners`);
  await expect(page.getByText(/not authorization/)).toBeVisible();
});

test("views are reachable from the keyboard", async ({ page }) => {
  await page.goto(DEMO);
  await expect(page.getByRole("region", { name: "Change summary" })).toBeVisible();
  await page.keyboard.press("4");
  await expect(page.getByRole("heading", { level: 1, name: "Risk & policy" })).toBeFocused();
  await page.keyboard.press("5");
  await expect(page).toHaveURL(/view=architecture/);
  await page.goBack();
  await expect(page).toHaveURL(/view=risk/);
});

test("unknown revisions produce a readable error", async ({ page }) => {
  await page.goto("/?base=no-such-branch&head=main");
  await expect(page.getByRole("alert")).toContainText("no-such-branch");
});
