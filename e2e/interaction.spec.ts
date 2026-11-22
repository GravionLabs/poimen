// Interaction tests on the fake variant (#1156): the backend answers what the test set up and logs every call,
// so a test asserts what an action called. Focus and keys are those of `ui/dialog-shell/dialog-contract.ts`.
import { expect, test } from "@playwright/test";
import { startup } from "./fake-base";
import { calls, clearCalls, expectNoFailures, openFake, openPane } from "./support";

test.describe("fake backend", () => {
  test("the shortcuts dialog takes the focus and gives it back on Escape", async ({ page }) => {
    const errors = await openFake(page, startup());
    await expect(page.getByText("alpha").first()).toBeVisible();
    const opener = page.getByRole("button", { name: "Settings", exact: true }).first();
    await opener.focus();
    await expect(opener).toBeFocused();

    await page.keyboard.press("ControlOrMeta+/");
    const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator(":focus")).toHaveCount(1);

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });

  test("closing a tab releases its terminal with close_terminal", async ({ page }) => {
    const nextId = 7;
    const errors = await openFake(page, {
      ...startup(),
      open_terminal: { id: nextId },
      terminal_resize: null,
      terminal_input: null,
      tab_focus: null,
      close_terminal: null,
      set_terminal_colors: null,
    });
    await openPane(page, "shell");
    await expect
      .poll(async () => (await calls(page)).map((c) => c.command))
      .toContain("open_terminal");
    await clearCalls(page);

    await page.getByRole("button", { name: /^Close .*shell$/ }).click();
    await expect
      .poll(async () => (await calls(page)).filter((c) => c.command === "close_terminal"))
      .toEqual([{ command: "close_terminal", args: { id: nextId } }]);
    await expectNoFailures(page);
    expect(errors()).toEqual([]);
  });
});
