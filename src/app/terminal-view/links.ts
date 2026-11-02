/** Whether `uri` is a link poimen opens: only `http` and `https`, never `file:`, `javascript:` or custom schemes. */
export function isOpenableUrl(uri: string): boolean {
  try {
    const { protocol } = new URL(uri);
    return protocol === "http:" || protocol === "https:";
  } catch {
    return false;
  }
}

/** The click handler of the web-links addon: opens a checked URL through `open`, ignores the rest. */
export function linkHandler(open: (url: string) => Promise<void>) {
  return (_event: MouseEvent, uri: string): void => {
    if (isOpenableUrl(uri)) void open(uri).catch(() => undefined);
  };
}

/** Whether the platform opens links with Cmd (macOS); elsewhere Ctrl does. */
export function isMacPlatform(): boolean {
  return typeof navigator !== "undefined" && /mac/i.test(navigator.platform);
}

/**
 * Whether `event` holds the modifier that works a link in input mode: Cmd on macOS, Ctrl elsewhere
 * (Ctrl+click is the context menu on macOS). Such a press bypasses the mouse forwarding, like Shift.
 */
export function holdsLinkModifier(
  event: { metaKey: boolean; ctrlKey: boolean },
  mac = isMacPlatform(),
): boolean {
  return mac ? event.metaKey : event.ctrlKey;
}

/** A rectangle of the underline drawn over a hovered link: one per terminal row the link covers. */
export interface UnderlineRect {
  left: number;
  top: number;
  width: number;
}

/**
 * The underline of a link as per-row segments, in pixels. `range` is the link's 1-based cell range
 * (`end.x` inclusive) as the web-links addon reports it; `cell` is the size of one cell and
 * `scrolled` the number of rows the viewport is scrolled down. The line sits on the bottom of the cell.
 */
export function underlineRects(
  range: { start: { x: number; y: number }; end: { x: number; y: number } },
  cols: number,
  cell: { width: number; height: number },
  scrolled = 0,
): UnderlineRect[] {
  const rects: UnderlineRect[] = [];
  for (let y = range.start.y; y <= range.end.y; y++) {
    const from = y === range.start.y ? range.start.x - 1 : 0;
    const to = y === range.end.y ? range.end.x : cols;
    if (to <= from) continue;
    rects.push({
      left: from * cell.width,
      top: (y - scrolled) * cell.height - 1,
      width: (to - from) * cell.width,
    });
  }
  return rects;
}

/**
 * Draws the hover underline as DOM over `screen`. xterm draws its own underline in the DOM renderer,
 * but the WebGL renderer paints it on a canvas layer that does not show in every webview, so the
 * view draws it itself and works the same with either renderer.
 */
export class LinkUnderline {
  private readonly lines: HTMLElement[] = [];

  show(screen: HTMLElement, rects: UnderlineRect[], color: string): void {
    this.hide();
    for (const rect of rects) {
      const line = screen.ownerDocument.createElement("div");
      line.className = "link-underline";
      line.style.cssText = `position:absolute;pointer-events:none;z-index:6;height:1px;background:${color};left:${rect.left}px;top:${rect.top}px;width:${rect.width}px`;
      screen.appendChild(line);
      this.lines.push(line);
    }
  }

  hide(): void {
    for (const line of this.lines.splice(0)) line.remove();
  }
}
