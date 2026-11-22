// The status bar popovers (#1168): the bar must not clip them on the block axis.
import { expect, test } from "@playwright/test";
import { startup } from "./fake-base";
import { expectNoFailures, openFake } from "./support";

test.describe("status bar", () => {
  test("the Update available popover is visible above the bar, inside the window", async ({
    page,
  }) => {
    const errors = await openFake(page, {
      ...startup(),
      update_check: {
        version: "9.9.9",
        currentVersion: "0.32.0",
        notes: "Release notes",
        date: null,
      },
    });
    await page.getByText("Check for updates").first().click();
    const item = page.getByText("Update available").first();
    await expect(item).toBeVisible();
    await item.click();

    const details = page.locator(".update-details");
    await expect(details).toBeVisible();
    await expect(details.getByRole("button", { name: "Install and restart" })).toBeVisible();

    const box = await details.boundingBox();
    const bar = await page.locator("hx-status-bar").boundingBox();
    const view = page.viewportSize()!;
    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(view.width);
    expect(box!.y + box!.height).toBeLessThanOrEqual(bar!.y + 1);
    // Nothing paints over it: the element at its center is the popover or inside it.
    const hit = await page.evaluate(
      ([x, y]) => !!document.elementFromPoint(x, y)?.closest(".update-details"),
      [box!.x + box!.width / 2, box!.y + box!.height / 2],
    );
    expect(hit).toBe(true);
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });
});
