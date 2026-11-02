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
