import { shortPath, type AgentStatus, type PaneRow, type SpaceNode } from "../core/herdr";

/** State word shown next to the name; `undefined` while herdr reports no known state. */
export function stateLabel(status: AgentStatus): string | undefined {
  return status === "unknown" ? undefined : status;
}

/** Title or step of a pane: the `deliver_step` token, else the stripped terminal title. */
export function paneDetail(row: PaneRow): string | undefined {
  const step = row.pane.tokens?.["deliver_step"]?.trim();
  return step || row.pane.terminal_title_stripped?.trim() || undefined;
}

/** Second line of a pane row: agent kind, then the title or step. */
export function subline(row: PaneRow): string {
  return [row.agent, paneDetail(row)].filter(Boolean).join(" · ");
}

/** Directory and branch every pane of a space shares (what the workspace row shows). */
export interface SpaceContext {
  dir?: string;
  branch?: string;
  worktree: boolean;
}

export function spaceContext(space: SpaceNode): SpaceContext {
  const [first, ...rest] = space.rows;
  if (!first) return { worktree: false };
  const same = <K extends "dir" | "branch">(key: K) =>
    first[key] !== undefined && rest.every((r) => r[key] === first[key]) ? first[key] : undefined;
  const branch = same("branch");
  return { dir: same("dir"), branch, worktree: branch !== undefined && first.worktree };
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
