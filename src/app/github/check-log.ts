import type { DetailCheck } from "./pulls";

/** The failed log of one check: loading, its last lines (colors stripped) or why it failed. */
export type CheckLog =
  | { status: "loading" }
  | { status: "ready"; text: string }
  | { status: "error"; message: string };

/** The last lines of a failed log that are shown. */
export const CHECK_LOG_LINES = 200;

/** Cache key of a check's log: the head commit it belongs to and the check's details URL. */
export function checkLogKey(headSha: string, url: string): string {
  return `${headSha}|${url}`;
}

// oxlint-disable-next-line no-control-regex
const ANSI = /\u001b(?:\[[0-?]*[ -/]*[@-~]|\][^\u0007\u001b]*(?:\u0007|\u001b\\)|[@-Z\\-_])/g;

/** Removes ANSI escape sequences (colors, cursor moves) so a log is never shown raw. */
export function stripAnsi(text: string): string {
  return text.replace(ANSI, "");
}

/** The last `max` lines of `text`. */
export function tailLines(text: string, max: number): string {
  const lines = text.replace(/\s+$/, "").split("\n");
  return lines.length <= max ? lines.join("\n") : lines.slice(-max).join("\n");
}

/** Only a failing check of a GitHub Actions run has a log to show (`gh run view --log-failed`). */
export function hasCheckLog(c: DetailCheck): boolean {
  return (
    c.state === "failing" &&
    !!c.url &&
    /^https:\/\/github\.com\/[^/]+\/[^/]+\/actions\/runs\/\d+/.test(c.url)
  );
}
