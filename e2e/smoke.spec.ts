// Smoke tests on the demo variant (#1156): whole views open with no unanswered command and no error.
import { expect, test } from "@playwright/test";
import { button, expectNoFailures, openDemo, ready } from "./support";

test.describe("demo backend", () => {
  test("the app starts and shows the sidebar", async ({ page }) => {
    const errors = await openDemo(page);
    await ready(page);
    await expect(page.getByText("billing-api").first()).toBeVisible();
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });

  test("the Runs view opens", async ({ page }) => {
    const errors = await openDemo(page);
    await ready(page);
    await button(page, "Runs").click();
    await expect(page.getByText("Writing the demo backend").first()).toBeVisible();
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });

  test("the command palette opens and closes", async ({ page }) => {
    const errors = await openDemo(page);
    await ready(page);
    await page.keyboard.press("ControlOrMeta+Shift+P");
    await expect(page.getByText("Close Tab").first()).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByText("Close Tab").first()).toBeHidden();
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });

  test("the settings open", async ({ page }) => {
    const errors = await openDemo(page);
    await ready(page);
    await button(page, "Settings").click();
    await expect(page.getByText("Color scheme").first()).toBeVisible();
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });
});
