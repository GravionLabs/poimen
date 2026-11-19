/** One review comment on a line range of a diff (Changes tab); `quote` is the commented text. */
export interface LineComment {
  id: string;
  path: string;
  /** `old` is the left side (removed lines), `new` the right side or the inline view. */
  side: "old" | "new";
  start: number;
  end: number;
  quote: string;
  text: string;
}

/** The comments of one worktree: not yet sent, and already sent. */
export interface CommentSet {
  pending: LineComment[];
  sent: LineComment[];
}

function parseComment(value: unknown): LineComment | null {
  if (typeof value !== "object" || value === null) return null;
  const c = value as Record<string, unknown>;
  if (
    typeof c["id"] !== "string" ||
    typeof c["path"] !== "string" ||
    (c["side"] !== "old" && c["side"] !== "new") ||
    !Number.isInteger(c["start"]) ||
    !Number.isInteger(c["end"]) ||
    (c["start"] as number) < 0 ||
    (c["end"] as number) < (c["start"] as number) ||
    typeof c["quote"] !== "string" ||
    typeof c["text"] !== "string"
  )
    return null;
  return {
    id: c["id"],
    path: c["path"],
    side: c["side"],
    start: c["start"] as number,
    end: c["end"] as number,
    quote: c["quote"],
    text: c["text"],
  };
}

/** Reads stored comments per worktree; malformed entries are dropped, empty worktrees too. */
export function parseLineComments(value: unknown): Record<string, CommentSet> {
  const out: Record<string, CommentSet> = {};
  if (typeof value !== "object" || value === null || Array.isArray(value)) return out;
  const list = (v: unknown): LineComment[] =>
    Array.isArray(v) ? v.map(parseComment).filter((c): c is LineComment => c !== null) : [];
  for (const [dir, set] of Object.entries(value)) {
    const s = (set ?? {}) as Record<string, unknown>;
    const entry = { pending: list(s["pending"]), sent: list(s["sent"]) };
    if (entry.pending.length || entry.sent.length) out[dir] = entry;
  }
  return out;
}
