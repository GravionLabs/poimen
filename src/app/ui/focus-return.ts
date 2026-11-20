/**
 * Giving the focus back to the element that opened a menu or dialog (#1072).
 *
 * A script focus after the menu's own keys or a dialog is treated as keyboard focus by browsers,
 * so a button clicked with the mouse showed `:focus-visible` after Cancel. The opener remembers
 * whether the last input before the open was a key; only then the ring stays.
 */

let lastWasKeyboard = false;
let tracking = false;

function track(): void {
  if (tracking || typeof document === "undefined") return;
  tracking = true;
  document.addEventListener("keydown", () => (lastWasKeyboard = true), true);
  document.addEventListener("pointerdown", () => (lastWasKeyboard = false), true);
  document.addEventListener("mousedown", () => (lastWasKeyboard = false), true);
}

track();

/** Marker that hides the ring of a pointer-origin focus until the element loses the focus. */
export const NO_RING_ATTR = "data-pointer-focus";

/** Call when the menu or dialog opens: true when the opener was reached by keyboard. */
export function openedByKeyboard(): boolean {
  track();
  return lastWasKeyboard;
}

/** Focus `opener` again; without `byKeyboard` no focus ring shows. */
export function returnFocus(opener: HTMLElement, byKeyboard: boolean): void {
  if (byKeyboard) {
    opener.focus({ preventScroll: true });
    return;
  }
  opener.setAttribute(NO_RING_ATTR, "");
  opener.addEventListener("blur", () => opener.removeAttribute(NO_RING_ATTR), { once: true });
  opener.focus({ preventScroll: true, focusVisible: false } as FocusOptions);
}
