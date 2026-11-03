import type { MouseCommand } from "../core/terminal-stream";

const BUTTONS = ["left", "middle", "right"] as const;

export function buttonName(button: number): MouseCommand["button"] {
  return BUTTONS[button] ?? "left";
}

/** Shift = 1, Ctrl = 2, Alt = 4 (herdr's `terminal.mouse` modifiers). */
export function modifierBits(event: { shiftKey: boolean; ctrlKey: boolean; altKey: boolean }) {
  return (event.shiftKey ? 1 : 0) | (event.ctrlKey ? 2 : 0) | (event.altKey ? 4 : 0);
}

/**
 * The zero-based cell under a client point, clamped to the grid. `cols`/`rows` is xterm's grid
 * (what the rect shows). When herdr renders a differently sized grid (`frame`, from the
 * `terminal.frame` width/height), the viewport is anchored at the top-left, so the point is
 * measured in xterm cells and clamped to the frame grid.
 */
export function cellAt(
  rect: { left: number; top: number; width: number; height: number },
  cols: number,
  rows: number,
  clientX: number,
  clientY: number,
  frame?: { cols: number; rows: number },
): { column: number; row: number } {
  if (rect.width <= 0 || rect.height <= 0) return { column: 0, row: 0 };
  const column = Math.floor(((clientX - rect.left) / rect.width) * cols);
  const row = Math.floor(((clientY - rect.top) / rect.height) * rows);
  const maxColumn = (frame && frame.cols > 0 ? Math.min(cols, frame.cols) : cols) - 1;
  const maxRow = (frame && frame.rows > 0 ? Math.min(rows, frame.rows) : rows) - 1;
  return {
    column: Math.max(0, Math.min(maxColumn, column)),
    row: Math.max(0, Math.min(maxRow, row)),
  };
}
