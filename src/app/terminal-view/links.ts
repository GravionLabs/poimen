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

const TRAILING_PLAIN = new Set([".", ",", ";", ":", "'", '"']);
const OPENER_FOR_CLOSER: Record<string, string> = { ")": "(", "]": "[" };

function count(text: string, char: string): number {
  return text.split(char).length - 1;
}

/**
 * Removes trailing punctuation that URL detection picks up from the surrounding text; the same
 * rules as the herdr link-trim plugin. `. , ; : ' "` go repeatedly; a closing `)` or `]` goes only
 * when the URL holds no opening partner, so `https://en.wikipedia.org/wiki/Foo_(bar)` stays whole.
 */
export function trimTrailingPunctuation(input: string): string {
  let url = input;
  for (;;) {
    const last = url.slice(-1);
    const opener = OPENER_FOR_CLOSER[last];
    if (
      TRAILING_PLAIN.has(last) ||
      (opener !== undefined && count(url, last) > count(url, opener))
    ) {
      url = url.slice(0, -1);
    } else {
      return url;
    }
  }
}

/** The click handler for URLs detected in plain terminal text: trims, then opens like `linkHandler`. */
export function detectedLinkHandler(open: (url: string) => Promise<void>) {
  const handle = linkHandler(open);
  return (event: MouseEvent, uri: string): void => handle(event, trimTrailingPunctuation(uri));
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

/** A GitHub issue link found in terminal text. */
export interface IssueLink {
  owner: string;
  repo: string;
  number: number;
}

const ISSUE_LINK =
  /^https:\/\/github\.com\/([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)\/issues\/([1-9]\d*)(?:[/?#].*)?$/;

/** The parts of `https://github.com/<owner>/<repo>/issues/<nr>`; `null` for any other URL or name. */
export function parseIssueLink(uri: string): IssueLink | null {
  const m = ISSUE_LINK.exec(uri);
  if (!m || [m[1], m[2]].some((n) => n === "." || n === "..")) return null;
  return { owner: m[1], repo: m[2], number: Number(m[3]) };
}

/** Whether `link` points at the repository `owner/name` (GitHub names compare case-insensitively). */
export function isRepo(link: IssueLink, repo: { owner: string; name: string } | null): boolean {
  return (
    !!repo &&
    link.owner.toLowerCase() === repo.owner.toLowerCase() &&
    link.repo.toLowerCase() === repo.name.toLowerCase()
  );
}

/**
 * The click handler for detected URLs and OSC 8 links with the task start: an issue link clicked
 * with the link modifier goes to `startTask`; when that does not take it (`false`: other
 * repository, no repository) or for any other click, the link opens as `detectedLinkHandler` does.
 */
export function taskLinkHandler(
  open: (url: string) => Promise<void>,
  startTask: (link: IssueLink) => Promise<boolean>,
  modifier: (event: MouseEvent) => boolean = holdsLinkModifier,
) {
  const fallback = detectedLinkHandler(open);
  return (event: MouseEvent, uri: string): void => {
    const url = trimTrailingPunctuation(uri);
    const link = parseIssueLink(url);
    if (!link || !modifier(event)) return fallback(event, uri);
    void startTask(link)
      .catch(() => false)
      .then((started) => {
        if (!started) fallback(event, uri);
      });
  };
}
