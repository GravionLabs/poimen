/** Narrowest and widest a tab gets; the strip scrolls only when all tabs are at the minimum. */
export const TAB_MIN_WIDTH = 144;
export const TAB_MAX_WIDTH = 320;

/** The width of every tab: the strip shared equally, kept between the minimum and the maximum. */
export function tabWidth(stripWidth: number, count: number): number {
  if (count <= 0) return TAB_MAX_WIDTH;
  const share = Math.floor(stripWidth / count);
  return Math.min(TAB_MAX_WIDTH, Math.max(TAB_MIN_WIDTH, share));
}

/**
 * Which edge of the tab `over` the insertion bar sits on while the tab `from` is dragged: after it
 * when the dragged tab comes from the left, before it when from the right, none over itself.
 */
export function dropSide(from: number | null, over: number | null): "before" | "after" | null {
  if (from === null || over === null || from === over) return null;
  return from > over ? "before" : "after";
}
