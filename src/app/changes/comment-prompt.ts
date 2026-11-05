import type { LineComment } from "../core/ui-state";

/** Longest prompt typed into the agent as is; longer comments go to a file the prompt names. */
export const MAX_PROMPT_LENGTH = 4000;

const INTRO = "Please address these review comments on the changes in this worktree.";

/** `path:start-end`, or `path:line` for one line; removed lines (left side) are marked. */
export function commentLocation(c: LineComment): string {
  // Start 0 is a review finding without a line: just the path.
  if (c.start === 0) return c.path;
  const lines = c.start === c.end ? `${c.start}` : `${c.start}-${c.end}`;
  return `${c.path}:${lines}${c.side === "old" ? " (removed lines)" : ""}`;
}

/** A fenced block for `text` whose fence is longer than any backtick run inside it. */
export function fenced(text: string): string {
  const longest = Math.max(0, ...(text.match(/`+/g) ?? []).map((run) => run.length));
  const fence = "`".repeat(Math.max(3, longest + 1));
  return `${fence}\n${text}\n${fence}`;
}

/** The comments as Markdown: per comment its location, the quoted diff lines and the comment. */
export function commentsMarkdown(comments: readonly LineComment[]): string {
  return comments
    .map((c) =>
      [commentLocation(c), ...(c.quote ? [fenced(c.quote)] : []), c.text.trim()].join("\n"),
    )
    .join("\n\n");
}

/** What to send: the prompt text, and the comments body when it has to be written to a file. */
export interface BuiltPrompt {
  text: string;
  /** Set when the comments are too long for the prompt: write it to a file, then `withFile`. */
  file: string | null;
}

/** The prompt for `comments`: inline when it fits {@link MAX_PROMPT_LENGTH}, else file-based. */
export function buildPrompt(comments: readonly LineComment[]): BuiltPrompt {
  const body = commentsMarkdown(comments);
  const text = `${INTRO}\n\n${body}`;
  return text.length <= MAX_PROMPT_LENGTH ? { text, file: null } : { text: INTRO, file: body };
}

/** The prompt that names the file the comments were written to. */
export function withFile(path: string): string {
  return `${INTRO} The comments are in ${path}; read that file and address each one.`;
}
