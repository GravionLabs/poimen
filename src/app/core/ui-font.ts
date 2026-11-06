/** Range of `ui.fontSize` in px (mirrors `settings.rs`). */
export const MIN_UI_FONT_SIZE = 10;
export const MAX_UI_FONT_SIZE = 20;
/** Range of `terminal.fontSize` in px (mirrors `settings.rs`). */
export const MIN_TERMINAL_FONT_SIZE = 6;
export const MAX_TERMINAL_FONT_SIZE = 72;

/** Common families offered as suggestions; any other family can be typed. */
export const COMMON_MONO_FONTS: readonly string[] = [
  "Menlo",
  "Monaco",
  "Consolas",
  "Cascadia Code",
  "Fira Code",
  "JetBrains Mono",
  "Source Code Pro",
  "SF Mono",
  "Hack",
  "Ubuntu Mono",
  "DejaVu Sans Mono",
  "IBM Plex Mono",
];

export const COMMON_UI_FONTS: readonly string[] = [
  "system-ui",
  "Inter",
  "Segoe UI",
  "Helvetica Neue",
  "Arial",
  "Roboto",
  "Noto Sans",
  "Ubuntu",
  "Open Sans",
  "IBM Plex Sans",
];

/** A size error message, or `null` when `size` is a finite number within `min`..`max`. */
export function sizeError(label: string, size: number, min: number, max: number): string | null {
  return Number.isFinite(size) && size >= min && size <= max
    ? null
    : `${label} must be a number from ${min} to ${max}`;
}

/**
 * Applies the UI font to the document: `--font-ui` (removed for the default stack) and
 * `--font-size-ui`, which `body` uses.
 */
export function applyUiFont(root: HTMLElement, family: string | null, size: number): void {
  if (family) root.style.setProperty("--font-ui", family);
  else root.style.removeProperty("--font-ui");
  root.style.setProperty("--font-size-ui", `${size}px`);
}
