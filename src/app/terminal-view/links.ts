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
