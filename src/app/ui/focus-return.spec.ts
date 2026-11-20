// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { NO_RING_ATTR, openedByKeyboard, returnFocus } from "./focus-return";

describe("focus-return", () => {
  it("knows whether the last input was a key or a pointer", () => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    expect(openedByKeyboard()).toBe(true);
    document.dispatchEvent(new MouseEvent("mousedown"));
    expect(openedByKeyboard()).toBe(false);
  });

  it("hides the ring after a pointer open until the element blurs", () => {
    const button = document.createElement("button");
    document.body.append(button);
    returnFocus(button, false);
    expect(document.activeElement).toBe(button);
    expect(button.hasAttribute(NO_RING_ATTR)).toBe(true);
    button.blur();
    expect(button.hasAttribute(NO_RING_ATTR)).toBe(false);
    button.remove();
  });

  it("keeps the ring after a keyboard open", () => {
    const button = document.createElement("button");
    document.body.append(button);
    returnFocus(button, true);
    expect(document.activeElement).toBe(button);
    expect(button.hasAttribute(NO_RING_ATTR)).toBe(false);
    button.remove();
  });
});
