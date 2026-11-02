import type { MouseCommand } from "../core/terminal-stream";

const BUTTONS = ["left", "middle", "right"] as const;

export function buttonName(button: number): MouseCommand["button"] {
  return BUTTONS[button] ?? "left";
}

/** Shift = 1, Ctrl = 2, Alt = 4 (herdr's `terminal.mouse` modifiers). */
export function modifierBits(event: { shiftKey: boolean; ctrlKey: boolean; altKey: boolean }) {
  return (event.shiftKey ? 1 : 0) | (event.ctrlKey ? 2 : 0) | (event.altKey ? 4 : 0);
}

/** The zero-based cell under a client point, clamped to the grid. */
export function cellAt(
  rect: { left: number; top: number; width: number; height: number },
  cols: number,
  rows: number,
  clientX: number,
  clientY: number,
): { column: number; row: number } {
  if (rect.width <= 0 || rect.height <= 0) return { column: 0, row: 0 };
  const column = Math.floor(((clientX - rect.left) / rect.width) * cols);
  const row = Math.floor(((clientY - rect.top) / rect.height) * rows);
  return {
    column: Math.max(0, Math.min(cols - 1, column)),
    row: Math.max(0, Math.min(rows - 1, row)),
  };
}
