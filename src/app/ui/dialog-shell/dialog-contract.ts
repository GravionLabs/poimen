import { expect } from "vitest";

/** Helpers for the keyboard contract every dialog spec checks (see `DialogShell`). */
export function dialogOf(root: HTMLElement): HTMLDialogElement {
  const dialog = root.querySelector("dialog");
  if (!dialog) throw new Error("no dialog is open");
  return dialog;
}

/** Puts the focus on a button outside, as the control that opens the dialog would have it. */
export function focusOpener(): HTMLButtonElement {
  const opener = document.createElement("button");
  opener.textContent = "opener";
  document.body.append(opener);
  opener.focus();
  expect(document.activeElement).toBe(opener);
  return opener;
}

/** The focus moved into the dialog without a click. */
export function expectFocusInside(root: HTMLElement): void {
  expect(dialogOf(root).contains(document.activeElement)).toBe(true);
}

/** A key pressed where the focus is, as the browser delivers it. */
export function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const target = document.activeElement ?? document.body;
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}
