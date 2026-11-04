export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Widest the popup gets, in px. */
export const PALETTE_WIDTH = 480;
/** Tallest the popup gets with a full list (search, 10 rows, footer), in px. */
export const PALETTE_MAX_HEIGHT = 320;
const MARGIN = 8;

/**
 * Top-left corner of the palette: centred on the editor area (the window when there is none), with
 * the popup's maximum height as basis so the top edge does not move while the list shrinks. Stays
 * inside the window.
 */
export function palettePosition(
  area: Box | null,
  viewport: { width: number; height: number },
  maxHeight = PALETTE_MAX_HEIGHT,
): { left: number; top: number } {
  const box = area ?? { left: 0, top: 0, width: viewport.width, height: viewport.height };
  const width = Math.min(PALETTE_WIDTH, viewport.width * 0.9);
  const height = Math.min(maxHeight, viewport.height * 0.85 - 16);
  const clamp = (value: number, size: number, limit: number) =>
    Math.max(MARGIN, Math.min(value, limit - size - MARGIN));
  return {
    left: clamp(box.left + (box.width - width) / 2, width, viewport.width),
    top: clamp(box.top + (box.height - height) / 2, height, viewport.height),
  };
}
