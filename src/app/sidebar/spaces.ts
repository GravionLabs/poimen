import {
  shortPath,
  type AgentStatus,
  type BranchPull,
  type PaneRow,
  type SpaceNode,
} from "../core/herdr";
import { checksText } from "../github/pulls";

/** State word shown next to the name; `undefined` while herdr reports no known state. */
export function stateLabel(status: AgentStatus): string | undefined {
  return status === "unknown" ? undefined : status;
}

/**
 * Step or title of an agent pane: the `deliver_step` token, else the stripped terminal title.
 * A plain terminal has none: its title is `user@host:~/path`, which herdr does not show.
 */
export function paneDetail(row: PaneRow): string | undefined {
  if (!row.agent) return undefined;
  const step = row.pane.tokens?.["deliver_step"]?.trim();
  return step || row.pane.terminal_title_stripped?.trim() || undefined;
}

/** Comparison key: parts that differ only by case or whitespace count as the same text. */
const same = (text: string | undefined) => (text ?? "").trim().replace(/\s+/g, " ").toLowerCase();

/**
 * Second line of a pane row: only what the bold name does not already say. The name of an agent
 * row is its title, so the title (and an agent kind equal to the name) is left out.
 */
export function subline(row: PaneRow): string {
  const detail = paneDetail(row);
  const seen = new Set([same(row.name)]);
  return [row.agent, detail]
    .filter((part): part is string => {
      if (!part || seen.has(same(part))) return false;
      seen.add(same(part));
      return true;
    })
    .join(" · ");
}

/** How the sidebar marks a pane: the active tab (strong) or merely an open tab (weak). */
export interface PaneMarks {
  open: boolean;
  active: boolean;
}

export function paneMarks(
  paneId: string,
  openIds: ReadonlySet<string>,
  activeId: string | null,
): PaneMarks {
  return { open: openIds.has(paneId), active: activeId !== null && activeId === paneId };
}

/** Directory and branch every pane of a space shares (what the workspace row shows). */
export interface SpaceContext {
  dir?: string;
  branch?: string;
  worktree: boolean;
  /** Shared `dirty` and `sync` token texts (empty when clean/even or not shared). */
  dirty?: string;
  sync?: string;
  /** The pull request every pane of the space shares. */
  pr?: BranchPull;
  /** The newest notification text of the space's agents. */
  notification?: string;
}

/** Newest notification text among the panes of a space (`undefined` when none has one). */
export function newestNotification(rows: readonly PaneRow[]): string | undefined {
  let best: { text: string; at: number } | undefined;
  for (const { pane } of rows) {
    const text = pane.agent_message?.trim();
    if (!text) continue;
    const at = pane.agent_message_at ?? 0;
    if (!best || at > best.at) best = { text, at };
  }
  return best?.text;
}

/** `●3` for three changed or untracked files; empty when clean or unknown. */
export function dirtyText(dirty: number | undefined): string {
  return dirty ? `●${dirty}` : "";
}

/** `↑2 ↓1`; empty when even, without upstream or unknown. */
export function syncText(ahead: number | undefined, behind: number | undefined): string {
  return [ahead ? `↑${ahead}` : "", behind ? `↓${behind}` : ""].filter(Boolean).join(" ");
}

/** `#123 ✓`, `#123 ✗`, `#123 …` (no symbol without checks); `merged` for a merged pull request. */
export function prText(pr: BranchPull): string {
  if (pr.state === "merged") return "merged";
  const symbol = { passing: "✓", failing: "✗", pending: "…", none: "" }[pr.checks.state];
  return symbol ? `#${pr.number} ${symbol}` : `#${pr.number}`;
}

/** Tooltip of the `pr` token: number, title, state and check counts. */
export function prTooltip(pr: BranchPull): string {
  const state = pr.state === "merged" ? "merged" : pr.isDraft ? "draft" : "open";
  const checks = checksText(pr.checks);
  return [`#${pr.number} ${pr.title} (${state})`, checks && `Checks: ${checks}`]
    .filter(Boolean)
    .join("\n");
}

export function spaceContext(space: SpaceNode): SpaceContext {
  const [first, ...rest] = space.rows;
  if (!first) return { worktree: false };
  const same = <K extends "dir" | "branch">(key: K) =>
    first[key] !== undefined && rest.every((r) => r[key] === first[key]) ? first[key] : undefined;
  const sharedText = (text: (r: PaneRow) => string) =>
    rest.every((r) => text(r) === text(first)) ? text(first) : "";
  const branch = same("branch");
  return {
    dir: same("dir"),
    branch,
    worktree: branch !== undefined && first.worktree,
    dirty: sharedText((r) => dirtyText(r.dirty)),
    sync: sharedText((r) => syncText(r.ahead, r.behind)),
    pr: rest.every((r) => r.pr?.url === first.pr?.url) ? first.pr : undefined,
    notification: newestNotification(space.rows),
  };
}

/**
 * Path and branch rule (herdr layout): the workspace row shows the shared branch and path once;
 * a pane row shows a branch or path only when it differs from that, and only on its second line.
 */
export function paneMeta(row: PaneRow, ctx: SpaceContext): string {
  const parts: string[] = [];
  if (row.branch && row.branch !== ctx.branch) {
    parts.push((row.worktree ? "wt " : "") + row.branch);
  }
  if (row.dir && row.dir !== ctx.dir) parts.push(shortPath(row.dir));
  return parts.join(" · ");
}

/** Text under the workspace name: shared branch and path. */
export function spaceMeta(ctx: SpaceContext): string {
  const parts: string[] = [];
  if (ctx.branch) parts.push((ctx.worktree ? "wt " : "") + ctx.branch);
  if (ctx.dir) parts.push(shortPath(ctx.dir));
  return parts.join(" · ");
}

/** Second (and last) line of a pane row: agent kind and title, then a differing branch or path. */
export function paneLine2(row: PaneRow, ctx: SpaceContext): string {
  return [subline(row), paneMeta(row, ctx)].filter(Boolean).join(" · ");
}

const STATUS_RANK: Record<AgentStatus, number> = {
  blocked: 4,
  working: 3,
  done: 2,
  idle: 1,
  unknown: 0,
};

/** Aggregate state of a workspace: the highest ranked of herdr's own value and its panes. */
export function aggregateStatus(space: SpaceNode): AgentStatus {
  return [space.workspace.agent_status, ...space.rows.map((r) => r.pane.agent_status)].reduce(
    (best, s) => ((STATUS_RANK[s] ?? 0) > (STATUS_RANK[best] ?? 0) ? s : best),
  );
}

/** Row types whose layout the user configures (`sidebar.rows`). */
export type RowType = "workspace" | "agent" | "terminal";

/** A token on a row line, with optional emphasis. */
export interface TokenSpec {
  token: string;
  bold?: boolean;
  dim?: boolean;
}

/** Lines of a row, each a list of tokens. */
export type RowLines = TokenSpec[][];

export type SidebarRows = Record<RowType, RowLines>;

const line = (...tokens: string[]): TokenSpec[] => tokens.map((token) => ({ token }));

/** The layout poimen shipped before it became configurable. */
export const DEFAULT_SIDEBAR_ROWS: SidebarRows = {
  workspace: [line("state_icon", "name", "state_text", "branch", "dirty", "sync", "dir")],
  agent: [line("state_icon", "name", "state_text"), line("agent", "title", "branch", "dir")],
  terminal: [line("state_icon", "name", "state_text"), line("agent", "title", "branch", "dir")],
};

/** Built-in token names (besides `$<name>`). */
export const ROW_TOKENS = [
  "state_icon",
  "name",
  "state_text",
  "agent",
  "title",
  "branch",
  "dirty",
  "sync",
  "pr",
  "notification",
  "dir",
  "tab",
] as const;

/** Whether `name` is a built-in token or `$<name>` with a herdr token name. */
export function isRowToken(name: string): boolean {
  return (ROW_TOKENS as readonly string[]).includes(name) || /^\$\S+$/.test(name);
}

/** One rendered piece of a row line; `kind` selects the element and its status classes. */
export interface Segment {
  kind: "dot" | "name" | "state" | "text";
  text: string;
  status: AgentStatus;
  bold: boolean;
  dim: boolean;
  /** Preceded by another text value on the line: shown after a " · ". */
  sep: boolean;
  /** `pr` token: the pull request to open on a click; `tip`: tooltip (`pr`, `notification`). */
  url?: string;
  tip?: string;
}

export type Line = Segment[];

/**
 * Renders the configured lines of a row. Empty values are skipped, and so is a value equal to an
 * earlier one on the row (the name counts as shown). On a pane row `branch` and `dir` show only
 * when they differ from the workspace; on the workspace row they are the shared values.
 */
export function renderRow(
  rowType: RowType,
  lines: RowLines,
  subject: SpaceNode | PaneRow,
  ctx: SpaceContext,
): Line[] {
  const pane = rowType === "workspace" ? undefined : (subject as PaneRow);
  const space = rowType === "workspace" ? (subject as SpaceNode) : undefined;
  const status: AgentStatus = pane
    ? pane.pane.agent_status
    : space
      ? aggregateStatus(space)
      : "unknown";
  const name = pane ? pane.name : (space?.workspace.label ?? "");
  const branchText = (branch: string, worktree: boolean) => (worktree ? "wt " : "") + branch;
  const value = (token: string): string | undefined => {
    switch (token) {
      case "name":
        return name;
      case "state_text":
        return stateLabel(status);
      case "agent":
        return pane?.agent;
      case "title":
        return pane ? paneDetail(pane) : undefined;
      case "tab":
        return pane?.tab;
      case "notification":
        return pane ? pane.pane.agent_message : ctx.notification;
      case "branch":
        if (!pane) return ctx.branch ? branchText(ctx.branch, ctx.worktree) : undefined;
        return pane.branch && pane.branch !== ctx.branch
          ? branchText(pane.branch, pane.worktree)
          : undefined;
      case "dirty": {
        const text = pane ? dirtyText(pane.dirty) : (ctx.dirty ?? "");
        return pane && text === (ctx.dirty ?? "") ? undefined : text;
      }
      case "sync": {
        const text = pane ? syncText(pane.ahead, pane.behind) : (ctx.sync ?? "");
        return pane && text === (ctx.sync ?? "") ? undefined : text;
      }
      case "dir":
        if (!pane) return ctx.dir ? shortPath(ctx.dir) : undefined;
        return pane.dir && pane.dir !== ctx.dir ? shortPath(pane.dir) : undefined;
      default:
        return token.startsWith("$") ? pane?.pane.tokens?.[token.slice(1)]?.trim() : undefined;
    }
  };
  const seen = new Set([same(name)]);
  return lines
    .map((spec) => {
      const out: Line = [];
      for (const { token, bold, dim } of spec) {
        const base = { status, bold: bold === true, dim: dim === true };
        if (token === "state_icon") {
          out.push({ ...base, kind: "dot", text: "", sep: false });
          continue;
        }
        if (token === "pr") {
          const pr = pane ? (pane.pr?.url !== ctx.pr?.url ? pane.pr : undefined) : ctx.pr;
          if (!pr) continue;
          const text = prText(pr);
          if (seen.has(same(text))) continue;
          seen.add(same(text));
          out.push({
            ...base,
            dim: base.dim || pr.isDraft,
            kind: "text",
            text,
            sep: out.at(-1)?.kind === "text",
            url: pr.url,
            tip: prTooltip(pr),
          });
          continue;
        }
        const text = value(token)?.trim();
        if (!text) continue;
        if (token === "name") {
          out.push({ ...base, kind: "name", text, sep: false });
          continue;
        }
        if (token === "state_text") {
          out.push({ ...base, kind: "state", text, sep: false });
          continue;
        }
        if (seen.has(same(text))) continue;
        seen.add(same(text));
        out.push({
          ...base,
          kind: "text",
          text,
          sep: out.at(-1)?.kind === "text",
          ...(token === "notification" && { tip: text }),
        });
      }
      return out;
    })
    .filter((l) => l.some((s) => s.kind !== "dot"));
}
