// The backend-neutral model of spaces, tabs and terminals (the `snapshot` command, #586) and the
// pure helpers on it. Every backend maps its own model onto it in Rust (`poimen_core::snapshot`).

import { HOOK_AGENTS, agentName, hasTranscripts } from "./backend";
import type { Checks } from "../github/pulls";
import type { SidebarSort } from "./settings";

export type AgentStatus = "idle" | "working" | "blocked" | "done" | "error" | "unknown";

/** The agent waits for the user or failed: it rings like a block (OSC 7501 `error`, #682). */
export function needsAttention(status: AgentStatus): boolean {
  return status === "blocked" || status === "error";
}

/**
 * Text a program reported (OSC 7501, hooks) as plain text for display: bidi overrides, isolates,
 * zero-width and other invisible formatting characters and control characters are removed, so
 * a report cannot reorder or hide what the user reads (#682).
 */
export function plainText(text: string | undefined): string {
  return (text ?? "")
    .replace(
      // eslint-disable-next-line no-control-regex
      /[\u0000-\u001f\u007f-\u009f\u00ad\u061c\u180e\u200b-\u200f\u202a-\u202e\u2060-\u206f\ufeff\ufff9-\ufffb]|\udb40[\udc00-\udc7f]/g,
      "",
    )
    .trim();
}

/** 0 to 100 as the backend reports it; anything else is no progress. */
export function validProgress(value: number | undefined): number | undefined {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 100
    ? value
    : undefined;
}

/** What a status label adds to the plain state (#682). */
export interface StateDetails {
  kind?: string;
  progress?: number;
}

/**
 * The state word with what OSC 7501 adds: `waiting: permission` for a block of a known kind,
 * `working 40%` with progress, else the status itself; `undefined` for `unknown`.
 */
export function stateText(status: AgentStatus, details: StateDetails = {}): string | undefined {
  if (status === "unknown") return undefined;
  const kind = plainText(details.kind);
  const progress = validProgress(details.progress);
  let text: string = status;
  if (status === "blocked" && kind) text = `waiting: ${kind}`;
  if (progress !== undefined && status !== "done" && status !== "idle") text += ` ${progress}%`;
  return text;
}

export interface Space {
  space_id: string;
  label: string;
  number: number;
  agent_status: AgentStatus;
  /**
   * The space's own directory (poimen backend: persisted in `server.json`, so it outlives the
   * terminals across a server restart).
   */
  cwd?: string;
  /** The machine the space lives on (remote spaces only, #280); its ids are qualified. */
  machine?: string;
}

export interface Tab {
  tab_id: string;
  space_id: string;
  label: string;
  number: number;
  agent_status: AgentStatus;
  /**
   * The split tree of the tab as the server keeps it (poimen backend); absent for a terminal
   * without a leaf (an exited one), which is a tab of its own.
   */
  root?: TabTree;
  /** The focused leaf of the tab, shared by every client (poimen backend). */
  active_terminal?: string;
  /** The machine of a remote tab (#280). */
  machine?: string;
}

/**
 * A node of a tab's split tree (`Node` of the poimen-server protocol): a terminal, or a split of
 * two nodes; `right` puts the second child right of the first, `down` below it, and `ratio` is the
 * share of the first child.
 */
export type TabTree =
  | { kind: "terminal"; terminal_id: string }
  | {
      kind: "split";
      split_id: string;
      direction: SplitDirection;
      ratio: number;
      children: [TabTree, TabTree];
    };

/** The terminals of the leaves of `tree`, depth first (first child before second). */
export function treeTerminals(tree: TabTree | undefined): string[] {
  if (!tree) return [];
  if (tree.kind === "terminal") return [tree.terminal_id];
  return tree.children.flatMap(treeTerminals);
}

/**
 * The terminal that takes over the tab of a closed pane: the tab's server-active terminal, else
 * its first leaf, never `closedId` itself; `undefined` when the tab has no other leaf.
 */
export function survivingLeaf(tab: Tab | undefined, closedId: string): string | undefined {
  const leaves = treeTerminals(tab?.root).filter((id) => id !== closedId);
  const active = tab?.active_terminal;
  return active && leaves.includes(active) ? active : leaves[0];
}

/**
 * The leaf of `paneId`'s split tab that takes over its editor tab once it closes, read from a
 * snapshot that still has the pane (the one before the close); `undefined` for an unknown pane
 * or the last leaf of its tab.
 */
export function paneSuccessor(snapshot: Snapshot, paneId: string): string | undefined {
  const tabId = snapshot.terminals.find((t) => t.terminal_id === paneId)?.tab_id;
  if (tabId === undefined) return undefined;
  return survivingLeaf(
    snapshot.tabs.find((t) => t.tab_id === tabId),
    paneId,
  );
}

/**
 * The panes of `tab` in the order of its split tree (the first leaf first), then the panes of
 * the tab the tree does not name, in snapshot order; so no terminal of the tab is left out.
 */
export function tabPanes(tab: Tab, terminals: readonly Terminal[]): Terminal[] {
  const own = terminals.filter((t) => t.tab_id === tab.tab_id);
  const leaves = treeTerminals(tab.root);
  const rank = new Map(leaves.map((id, i) => [id, i]));
  const inTree = own
    .filter((t) => rank.has(t.terminal_id))
    .sort((a, b) => rank.get(a.terminal_id)! - rank.get(b.terminal_id)!);
  return [...inTree, ...own.filter((t) => !rank.has(t.terminal_id))];
}

/** The terminal a tab shows: the first leaf of its tree, else its first pane. */
export function tabTerminal(tab: Tab, terminals: readonly Terminal[]): Terminal | undefined {
  return tabPanes(tab, terminals)[0];
}

/**
 * The root terminal of a space: the terminal its first tab (by number) shows, which is the root
 * pane of the layout of a new space; else its first terminal.
 */
export function spaceRoot(snapshot: Snapshot, spaceId: string): Terminal | undefined {
  const first = snapshot.tabs
    .filter((t) => t.space_id === spaceId)
    .sort((a, b) => a.number - b.number)
    .map((tab) => tabTerminal(tab, snapshot.terminals))
    .find((t) => t !== undefined);
  return first ?? snapshot.terminals.find((t) => t.space_id === spaceId);
}

/** A child record of a terminal (OSC 7501 with an `id`, #684): a row below the terminal. */
export interface ChildRecord {
  /** `/`-separated path such as `run/issue-12`; a parent's id is a prefix of its children's. */
  id: string;
  status: AgentStatus;
  app?: string;
  message?: string;
  progress?: number;
  kind?: string;
  title?: string;
}

export interface Terminal {
  terminal_id: string;
  tab_id: string;
  space_id: string;
  agent?: string;
  /** The agent's own session (Claude Code, Copilot CLI: the transcript's session id in `value`). */
  agent_session?: { value?: string };
  agent_status: AgentStatus;
  /** Latest notification text of the agent (poimen backend only), with its time in epoch ms. */
  agent_message?: string;
  agent_message_at?: number;
  /** Progress 0 to 100, why the agent is blocked (`permission`, `question`, `auth`) and the program's own title (OSC 7501, #682). */
  agent_progress?: number;
  agent_kind?: string;
  agent_title?: string;
  /** Child records, least recently updated first (poimen backend, #684). */
  records?: ChildRecord[];
  cwd?: string;
  /** Directory of the foreground process; follows `cd` and is more precise than `cwd`. */
  foreground_cwd?: string;
  /** User-set pane label (e.g. "terminal"). */
  label?: string;
  /** Free-form key/value tokens set by integrations (e.g. `deliver_step`). */
  tokens?: Record<string, string>;
  /** Runs in a PTY host that a restarted poimen-server adopts (poimen backend, #604). */
  survives_restart?: boolean;
  /** The program the terminal runs (poimen backend). */
  program?: string;
  /** Version of the PTY host the terminal runs in, and of the one the server ships with (#606). */
  host_version?: string;
  host_bundled_version?: string;
  terminal_title_stripped?: string;
  focused?: boolean;
  /** The machine of a remote terminal (#280). */
  machine?: string;
}

/** Branch and worktree flag of a directory (answer of the `repo_info` command). */
export interface RepoInfo {
  branch: string | null;
  worktree: boolean;
  /** Canonical git common dir: the same for a repository's main checkout and its worktrees. */
  common_dir?: string;
  /** Changed plus untracked files; null/absent when unknown. */
  dirty?: number | null;
  /** Commits ahead of / behind the upstream; null/absent without upstream. */
  ahead?: number | null;
  behind?: number | null;
  /** The branch is the base branch of the repository (`git::base_ref`). */
  on_base?: boolean;
}

/** The pull request of a branch as `branch_pulls` returns it (`BranchPull` of `github_branches.rs`). */
export interface BranchPull {
  number: number;
  title: string;
  url: string;
  state: "open" | "merged";
  isDraft: boolean;
  checks: Checks;
}

/** The backend-neutral state: spaces hold tabs hold terminals (a terminal is identified by `terminal_id`). */
export interface Snapshot {
  spaces: Space[];
  tabs: Tab[];
  terminals: Terminal[];
}

/**
 * The answer of the `snapshot` and `machine_snapshot` commands (`poimen_core::snapshot::Snapshot`):
 * already the neutral model, the server's model is mapped onto it in `poimen_backend.rs`. Only
 * `toSnapshot` reads it; everything else works on `Snapshot`.
 */
export type WireSnapshot = Partial<Snapshot>;

/** The snapshot of a `snapshot` answer; a missing list is empty. */
export function toSnapshot(wire: WireSnapshot | null | undefined): Snapshot {
  return {
    spaces: wire?.spaces ?? [],
    tabs: wire?.tabs ?? [],
    terminals: (wire?.terminals ?? []).map(plainTerminal),
  };
}

/** The terminal with the texts a program reported cleaned (`plainText`); unchanged when none is set. */
function plainTerminal(t: Terminal): Terminal {
  if (t.records?.length) t = { ...t, records: t.records.map(plainRecord) };
  if (t.agent_message === undefined && t.agent_title === undefined && t.agent_kind === undefined)
    return t;
  const next = { ...t };
  for (const key of ["agent_message", "agent_title", "agent_kind"] as const) {
    if (next[key] === undefined) continue;
    const clean = plainText(next[key]);
    if (clean) next[key] = clean;
    else delete next[key];
  }
  return next;
}

/** A child record with its texts as plain text and a valid progress. */
function plainRecord(r: ChildRecord): ChildRecord {
  const next: ChildRecord = { id: plainText(r.id), status: r.status };
  const progress = validProgress(r.progress);
  if (progress !== undefined) next.progress = progress;
  for (const key of ["app", "message", "kind", "title"] as const) {
    const clean = plainText(r[key]);
    if (clean) next[key] = clean;
  }
  return next;
}

const QUALIFIER = "::";

/** The id of `id` on `machine`: ids of two machines (and of this Mac) never collide. */
export function qualifyId(machine: string, id: string): string {
  return `${machine}${QUALIFIER}${id}`;
}

/** The machine and the machine's own id of an id from `qualifyId`; a local id has no machine. */
export function splitId(id: string): { machine?: string; id: string } {
  const at = id.lastIndexOf(QUALIFIER);
  return at < 0 ? { id } : { machine: id.slice(0, at), id: id.slice(at + QUALIFIER.length) };
}

function qualifyTree(machine: string, tree: TabTree): TabTree {
  return tree.kind === "terminal"
    ? { ...tree, terminal_id: qualifyId(machine, tree.terminal_id) }
    : {
        ...tree,
        children: [qualifyTree(machine, tree.children[0]), qualifyTree(machine, tree.children[1])],
      };
}

/**
 * A snapshot read from `machine` with every id qualified and `machine` set on each record. With
 * `stale` the machine is not reachable: its agents report `unknown`, so no old status rings.
 */
export function qualifySnapshot(machine: string, snapshot: Snapshot, stale = false): Snapshot {
  return {
    spaces: snapshot.spaces.map((s) => ({
      ...s,
      machine,
      space_id: qualifyId(machine, s.space_id),
      ...(stale ? { agent_status: "unknown" as const } : {}),
    })),
    tabs: snapshot.tabs.map((t) => ({
      ...t,
      machine,
      tab_id: qualifyId(machine, t.tab_id),
      space_id: qualifyId(machine, t.space_id),
      ...(t.root ? { root: qualifyTree(machine, t.root) } : {}),
      ...(t.active_terminal ? { active_terminal: qualifyId(machine, t.active_terminal) } : {}),
      ...(stale ? { agent_status: "unknown" as const } : {}),
    })),
    terminals: snapshot.terminals.map((p) => ({
      ...p,
      machine,
      terminal_id: qualifyId(machine, p.terminal_id),
      tab_id: qualifyId(machine, p.tab_id),
      space_id: qualifyId(machine, p.space_id),
      ...(stale ? { agent_status: "unknown" as const } : {}),
    })),
  };
}

/** Appends `extra` to `base` (disjoint ids; spaces with the same name stay two spaces). */
export function mergeSnapshots(base: Snapshot, extra: readonly Snapshot[]): Snapshot {
  if (extra.length === 0) return base;
  return {
    spaces: [...base.spaces, ...extra.flatMap((s) => s.spaces)],
    tabs: [...base.tabs, ...extra.flatMap((s) => s.tabs)],
    terminals: [...base.terminals, ...extra.flatMap((s) => s.terminals)],
  };
}

/** A tab or window title with the machine name in front when the pane is remote. */
export function withMachine(title: string, machine: string | undefined): string {
  return machine ? `${machine}: ${title}` : title;
}

/** Space and name of a pane as its tab title shows them, with the machine name for a remote pane. */
export function paneTitle(snap: Snapshot, pane: Terminal): string {
  const tabLabel = snap.tabs.find((t) => t.tab_id === pane.tab_id)?.label;
  const workspace = snap.spaces.find((w) => w.space_id === pane.space_id)?.label;
  return withMachine(tabHeader(workspace, rowName(pane, tabLabel)).title, pane.machine);
}

export interface TabNode {
  tab: Tab;
  panes: Terminal[];
}

export interface WorkspaceNode {
  workspace: Space;
  tabs: TabNode[];
}

const byNumber = <T extends { number: number }>(a: T, b: T) => a.number - b.number;

/**
 * Compares two names like `poimen_core::order::natural_cmp` (the TUI): case ignored, runs of ASCII
 * digits as numbers (`#9` before `#12`), accents significant (code points after lowercasing).
 * `fixtures/name-order.json` of poimen-core is read by both specs.
 */
export function naturalCompare(a: string, b: string): number {
  let i = 0;
  let j = 0;
  const digit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
  while (i < a.length && j < b.length) {
    if (digit(a[i]) && digit(b[j])) {
      let ie = i;
      let je = j;
      while (digit(a[ie])) ie++;
      while (digit(b[je])) je++;
      const x = a.slice(i, ie).replace(/^0+/, "");
      const y = b.slice(j, je).replace(/^0+/, "");
      if (x.length !== y.length) return x.length < y.length ? -1 : 1;
      if (x !== y) return x < y ? -1 : 1;
      i = ie;
      j = je;
      continue;
    }
    const x = String.fromCodePoint(a.codePointAt(i)!);
    const y = String.fromCodePoint(b.codePointAt(j)!);
    i += x.length;
    j += y.length;
    const lx = Array.from(x.toLowerCase(), (c) => c.codePointAt(0)!);
    const ly = Array.from(y.toLowerCase(), (c) => c.codePointAt(0)!);
    for (let k = 0; k < Math.min(lx.length, ly.length); k++) {
      if (lx[k] !== ly[k]) return lx[k] < ly[k] ? -1 : 1;
    }
    if (lx.length !== ly.length) return lx.length < ly.length ? -1 : 1;
  }
  const ra = a.length - i;
  const rb = b.length - j;
  return ra === rb ? 0 : ra === 0 ? -1 : rb === 0 ? 1 : 0;
}

/** Alphabetical by label (`naturalCompare`); ties by backend number. */
const byLabel = <T extends { label: string; number: number }>(a: T, b: T) =>
  naturalCompare(a.label, b.label) || a.number - b.number;

/** What `attention` and `recent` sort by (#712); `manual` and `name` need none of it. */
export interface SortKeys {
  /** Attention rank per `space_id` (`spaceAttentionRanks`); lower comes first. */
  attention?: ReadonlyMap<string, number>;
  /** When the user last focused each space (epoch ms). */
  recent?: Readonly<Record<string, number>>;
}

/** Rank of a space without an agent, and of one the map does not know. */
export const NO_AGENT_RANK = 4;

/**
 * Rank of one terminal for `sidebar.sort: attention`: blocked 0, done and unread 1, working 2,
 * idle or done and read 3, no agent 4.
 */
export function attentionRank(terminal: Terminal, unread: ReadonlySet<string>): number {
  switch (terminal.agent_status) {
    case "blocked":
    case "error":
      return 0;
    case "done":
      return unread.has(terminal.terminal_id) ? 1 : 3;
    case "working":
      return 2;
    case "idle":
      return 3;
    default:
      return agentKind(terminal) ? 3 : NO_AGENT_RANK;
  }
}

/** The rank of each space: its most urgent terminal (`attentionRank`). */
export function spaceAttentionRanks(
  terminals: readonly Terminal[],
  unread: ReadonlySet<string>,
): Map<string, number> {
  const ranks = new Map<string, number>();
  for (const t of terminals) {
    const rank = attentionRank(t, unread);
    ranks.set(t.space_id, Math.min(ranks.get(t.space_id) ?? NO_AGENT_RANK, rank));
  }
  return ranks;
}

/**
 * The spaces in the user's order: ids of `order` first (in that order), then the others in backend
 * number order (new spaces). Ids of `order` that no longer exist are skipped. Pinned spaces
 * (`pinned`, #710) come first in every sort mode, ordered among themselves like the rest.
 */
export function orderSpaces<T extends { space_id: string; label: string; number: number }>(
  spaces: readonly T[],
  order: readonly string[] = [],
  sort: SidebarSort = "manual",
  pinned: readonly string[] = [],
  keys: SortKeys = {},
): T[] {
  const ordered = orderUnpinned(spaces, order, sort, keys);
  if (!pinned.length) return ordered;
  const pins = new Set(pinned);
  return [
    ...ordered.filter((s) => pins.has(s.space_id)),
    ...ordered.filter((s) => !pins.has(s.space_id)),
  ];
}

function orderUnpinned<T extends { space_id: string; label: string; number: number }>(
  spaces: readonly T[],
  order: readonly string[],
  sort: SidebarSort,
  keys: SortKeys,
): T[] {
  if (sort === "name") return [...spaces].sort(byLabel);
  if (sort === "attention") {
    const rank = (s: T) => keys.attention?.get(s.space_id) ?? NO_AGENT_RANK;
    return [...spaces].sort((a, b) => rank(a) - rank(b) || byLabel(a, b));
  }
  if (sort === "recent") {
    const at = (s: T) => keys.recent?.[s.space_id];
    return [...spaces].sort((a, b) => {
      const [x, y] = [at(a), at(b)];
      if (x !== undefined && y !== undefined) return y - x || b.number - a.number;
      if (x !== undefined) return -1;
      if (y !== undefined) return 1;
      return b.number - a.number;
    });
  }
  const rank = new Map(order.map((id, i) => [id, i]));
  const known = spaces
    .filter((s) => rank.has(s.space_id))
    .sort((a, b) => rank.get(a.space_id)! - rank.get(b.space_id)!);
  const rest = spaces.filter((s) => !rank.has(s.space_id)).sort(byNumber);
  return [...known, ...rest];
}

/**
 * The order after dropping `sourceId` before or after `targetId`, or `null` when nothing changes
 * (same space, an unknown id, or the space already sits there).
 */
export function reorderIds(
  ids: readonly string[],
  sourceId: string,
  targetId: string,
  after: boolean,
): string[] | null {
  if (sourceId === targetId || !ids.includes(sourceId) || !ids.includes(targetId)) return null;
  const rest = ids.filter((id) => id !== sourceId);
  rest.splice(rest.indexOf(targetId) + (after ? 1 : 0), 0, sourceId);
  return rest.every((id, i) => id === ids[i]) ? null : rest;
}

/** The tabs of one space in the user's order: ids of `order` first, new tabs by number. */
export function orderTabs<T extends { tab_id: string; number: number }>(
  tabs: readonly T[],
  order: readonly string[] = [],
): T[] {
  const rank = new Map(order.map((id, i) => [id, i]));
  const known = tabs
    .filter((t) => rank.has(t.tab_id))
    .sort((a, b) => rank.get(a.tab_id)! - rank.get(b.tab_id)!);
  const rest = tabs.filter((t) => !rank.has(t.tab_id)).sort(byNumber);
  return [...known, ...rest];
}

/** Tab order per `space_id` (`tab_id`s), as the user arranged it. */
export type TabOrder = Readonly<Record<string, readonly string[]>>;

export function buildTree(
  snapshot: Snapshot,
  order: readonly string[] = [],
  tabOrder: TabOrder = {},
  sort: SidebarSort = "manual",
  pinned: readonly string[] = [],
  keys: SortKeys = {},
): WorkspaceNode[] {
  return orderSpaces(snapshot.spaces, order, sort, pinned, keys).map((workspace) => ({
    workspace,
    tabs: orderTabs(
      snapshot.tabs.filter((tab) => tab.space_id === workspace.space_id),
      tabOrder[workspace.space_id],
    ).map((tab) => ({ tab, panes: tabPanes(tab, snapshot.terminals) })),
  }));
}

/** The segments of a path, split at `/` and `\` (Windows cwds), without empty ones. */
export function pathSegments(path: string): string[] {
  return path.split(/[\\/]+/).filter(Boolean);
}

function basename(path: string): string {
  return pathSegments(path).pop() ?? path;
}

export function paneLabel(pane: Terminal): string {
  return (
    pane.terminal_title_stripped?.trim() ||
    pane.agent ||
    (pane.cwd ? basename(pane.cwd) : "") ||
    pane.terminal_id
  );
}

/**
 * Sidebar name of a pane row. The pane label wins when set (a rename); without one, an agent pane
 * shows its session title, and a plain terminal shows the tab label (`herdr tab list`, e.g.
 * "terminal"). The shell's terminal title (`user@host:~/path`) is never shown. With no label at
 * all, the directory name stands in.
 */
export function rowName(pane: Terminal, tabLabel?: string): string {
  if (agentKind(pane)) return pane.label?.trim() || paneLabel(pane);
  return (
    pane.label?.trim() ||
    tabLabel?.trim() ||
    (paneDir(pane) ? basename(paneDir(pane)!) : "") ||
    pane.terminal_id
  );
}

/** Tab header of a pane: workspace (shown uppercase), a slash, the pane name last; `title` is the full text. */
export function tabHeader(
  workspace: string | undefined,
  name: string,
): { group?: string; label: string; title: string } {
  const group = workspace?.trim() || undefined;
  return { group, label: name, title: group ? `${group.toUpperCase()} / ${name}` : name };
}

/** Panes whose agent waits for the user. */
export function waitingCount(panes: Terminal[]): number {
  return panes.filter((pane) => needsAttention(pane.agent_status)).length;
}

/**
 * The blocked pane after `activeId` in list order, wrapping around; undefined when none waits.
 * With `order` (terminal ids as the Agents section shows them) the panes follow it; panes not in
 * it come after, in list order.
 */
export function nextWaiting(
  all: Terminal[],
  activeId: string | null,
  order?: readonly string[],
): Terminal | undefined {
  const position = new Map(order?.map((id, i) => [id, i]));
  const panes = order
    ? [...all].sort(
        (a, b) =>
          (position.get(a.terminal_id) ?? order.length) -
          (position.get(b.terminal_id) ?? order.length),
      )
    : all;
  const waiting = panes.filter((pane) => needsAttention(pane.agent_status));
  if (!waiting.length) return undefined;
  const index = panes.findIndex((pane) => pane.terminal_id === activeId);
  const after = panes.slice(index + 1).find((pane) => needsAttention(pane.agent_status));
  return after ?? waiting[0];
}

/**
 * Applies a terminal.agent_status_changed event without a full refresh. A notification text replaces
 * the kept one; without one the text stays, except when the agent turns `working` or ends
 * (`unknown`).
 */
export function applyStatus(
  snapshot: Snapshot,
  paneId: string,
  status: AgentStatus,
  message?: { text: string; at: number },
  details: { progress?: number; kind?: string; title?: string } = {},
): Snapshot {
  return {
    ...snapshot,
    terminals: snapshot.terminals.map((pane) => {
      if (pane.terminal_id !== paneId) return pane;
      // The event carries the whole state: progress, kind and title are what it says, or gone.
      const base: Terminal = { ...pane, agent_status: status };
      delete base.agent_progress;
      delete base.agent_kind;
      delete base.agent_title;
      const progress = validProgress(details.progress);
      const kind = plainText(details.kind);
      const title = plainText(details.title);
      if (progress !== undefined) base.agent_progress = progress;
      if (kind) base.agent_kind = kind;
      if (title) base.agent_title = title;
      if (message) {
        const text = plainText(message.text);
        if (text) return { ...base, agent_message: text, agent_message_at: message.at };
      }
      const next = base;
      if (status === "working" || status === "unknown") {
        delete next.agent_message;
        delete next.agent_message_at;
      }
      return next;
    }),
  };
}

/** The directory to show for a pane. */
export function paneDir(pane: Terminal): string | undefined {
  return pane.foreground_cwd || pane.cwd || undefined;
}

/** Agent kind of a pane (`claude`, `codex`, ...); `undefined` for a plain terminal. */
export function agentKind(pane: Terminal): string | undefined {
  return pane.agent?.trim() || undefined;
}

/** Display name of the agent of `pane` if its transcripts can be followed (`Claude Code`, ...). */
export function flowAgentName(pane: Terminal | undefined): string | undefined {
  const kind = pane && agentKind(pane);
  return kind !== undefined && hasTranscripts(kind) ? agentName(kind) : undefined;
}

/** Whether the Output panel and the Flow tab can follow the agent of `pane`. */
export function supportsFlow(pane: Terminal | undefined): boolean {
  return flowAgentName(pane) !== undefined;
}

/** The session id the transcripts of `pane` are found by; `null` when it has none to follow. */
export function flowSession(pane: Terminal | undefined): string | null {
  return (supportsFlow(pane) && pane?.agent_session?.value) || null;
}

/**
 * Row text of an agent pane: its state label (`agent_status`, as `herdr pane list` reports it)
 * plus the `deliver_step` token when an integration set one. herdr reports no free-form status
 * message, so plain terminals and agents with an unknown state have no text.
 */
export function statusText(pane: Terminal): string | undefined {
  if (!agentKind(pane)) return undefined;
  const state = stateText(pane.agent_status, {
    kind: pane.agent_kind,
    progress: pane.agent_progress,
  });
  const step = pane.tokens?.["deliver_step"]?.trim() || undefined;
  return [state, step].filter(Boolean).join(" · ") || undefined;
}

/** The last two path segments, so rows stay short. */
export function shortPath(path: string): string {
  const parts = pathSegments(path);
  return parts.length <= 2 ? path : "…/" + parts.slice(-2).join("/");
}

/** What a sidebar row shows for one pane. */
export interface PaneRow {
  pane: Terminal;
  tab: string;
  name: string;
  agent?: string;
  branch?: string;
  worktree: boolean;
  /** The branch is the repository's base branch. */
  onBase?: boolean;
  dirty?: number;
  ahead?: number;
  behind?: number;
  /** Pull request of the branch, when it has one. */
  pr?: BranchPull;
  dir?: string;
  text?: string;
}

export interface SpaceNode {
  workspace: Space;
  rows: PaneRow[];
  /** Branch and worktree of the space's own directory (`workspace.cwd`), when known. */
  repo?: RepoInfo;
  /** `space_id` of the head when this space is a linked worktree listed below it. */
  parent?: string;
}

/**
 * Lists the spaces whose directory is a linked worktree directly after the space of the main
 * checkout of the same repository (`common_dir`), keeping the given order inside heads and
 * inside members. A group without a main checkout and a space without repository stay flat.
 */
export function groupWorktrees(nodes: readonly SpaceNode[]): SpaceNode[] {
  const heads = new Map<string, SpaceNode>();
  for (const n of nodes) {
    const key = n.repo?.common_dir;
    if (key && !n.repo!.worktree && !heads.has(key)) heads.set(key, n);
  }
  const members = new Map<string, SpaceNode[]>();
  for (const n of nodes) {
    const key = n.repo?.common_dir;
    const head = key ? heads.get(key) : undefined;
    if (!head || !n.repo!.worktree) continue;
    members.set(head.workspace.space_id, [
      ...(members.get(head.workspace.space_id) ?? []),
      { ...n, parent: head.workspace.space_id },
    ]);
  }
  const grouped = new Set([...members.values()].flat().map((m) => m.workspace.space_id));
  return nodes
    .filter((n) => !grouped.has(n.workspace.space_id))
    .flatMap((n) => [n, ...(members.get(n.workspace.space_id) ?? [])]);
}

/**
 * Whether dropping `sourceId` on `targetId` is allowed in the grouped list: a member only among
 * its siblings (same head), a head or flat space on any head or flat space (not on a member).
 */
export function dropAllowed(
  nodes: readonly SpaceNode[],
  sourceId: string,
  targetId: string,
): boolean {
  const source = nodes.find((n) => n.workspace.space_id === sourceId);
  const target = nodes.find((n) => n.workspace.space_id === targetId);
  if (!source || !target) return false;
  return source.parent === target.parent;
}

/**
 * The flat id order after dropping `sourceId` before or after `targetId` in the grouped list
 * (a head moves with its members), or `null` when refused or nothing changes.
 */
export function reorderGrouped(
  nodes: readonly SpaceNode[],
  sourceId: string,
  targetId: string,
  after: boolean,
): string[] | null {
  if (!dropAllowed(nodes, sourceId, targetId)) return null;
  const source = nodes.find((n) => n.workspace.space_id === sourceId)!;
  const flat = nodes.map((n) => n.workspace.space_id);
  if (source.parent) return reorderIds(flat, sourceId, targetId, after);
  const blocks = new Map<string, string[]>();
  for (const n of nodes) {
    const head = n.parent ?? n.workspace.space_id;
    blocks.set(head, [...(blocks.get(head) ?? []), n.workspace.space_id]);
  }
  const heads = reorderIds([...blocks.keys()], sourceId, targetId, after);
  return heads ? heads.flatMap((h) => blocks.get(h)!) : null;
}

/** Workspaces with their pane rows (tab label as secondary text), enriched with repo info. */
export function buildSpaces(
  snapshot: Snapshot,
  repos: ReadonlyMap<string, RepoInfo>,
  pulls: ReadonlyMap<string, BranchPull> = new Map(),
  order: readonly string[] = [],
  tabOrder: TabOrder = {},
  sort: SidebarSort = "manual",
  group = false,
  pinned: readonly string[] = [],
  keys: SortKeys = {},
): SpaceNode[] {
  const nodes = buildTree(snapshot, order, tabOrder, sort, pinned, keys).map(
    ({ workspace, tabs }) => ({
      workspace,
      ...(workspace.cwd && repos.has(workspace.cwd) ? { repo: repos.get(workspace.cwd) } : {}),
      rows: tabs.flatMap(({ tab, panes }) =>
        panes.map((pane): PaneRow => {
          const dir = paneDir(pane);
          const repo = dir ? repos.get(dir) : undefined;
          return {
            pane,
            tab: tab.label,
            name: rowName(pane, tab.label),
            agent: agentKind(pane),
            branch: repo?.branch ?? undefined,
            worktree: repo?.worktree ?? false,
            onBase: repo?.on_base ?? false,
            dirty: repo?.dirty ?? undefined,
            ahead: repo?.ahead ?? undefined,
            behind: repo?.behind ?? undefined,
            pr: dir ? pulls.get(dir) : undefined,
            dir,
            text: statusText(pane),
          };
        }),
      ),
    }),
  );
  return group ? groupWorktrees(nodes) : nodes;
}

/** Keeps rows matching `query` (name, agent, branch, tab, directory, text; every word must match). */
export function filterSpaces(
  spaces: SpaceNode[],
  query: string,
  /** Texts a row is matched on instead of the defaults (the values of the shown tokens). */
  values?: (row: PaneRow) => string[],
): SpaceNode[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return spaces;
  const matches = (row: PaneRow, space: string) => {
    const hay = (
      values
        ? [...values(row), space]
        : [row.name, row.agent, row.branch, row.tab, row.dir, row.text, space]
    )
      .filter(Boolean)
      .join("\n")
      .toLowerCase();
    return words.every((w) => hay.includes(w));
  };
  const kept = spaces
    .map((space) => ({
      ...space,
      rows: space.rows.filter((r) => matches(r, space.workspace.label)),
    }))
    .filter((space) => space.rows.length > 0);
  // A matching worktree space keeps its head, shown without rows when the head itself has none.
  const shown = new Set(kept.map((s) => s.workspace.space_id));
  const needed = new Set(kept.flatMap((s) => (s.parent ? [s.parent] : [])));
  return spaces
    .filter((s) => shown.has(s.workspace.space_id) || needed.has(s.workspace.space_id))
    .map(
      (s) => kept.find((k) => k.workspace.space_id === s.workspace.space_id) ?? { ...s, rows: [] },
    );
}

/** Where a split puts the new pane (`pane.split` direction). */
export type SplitDirection = "right" | "down";

/**
 * The distinct directories of a workspace: its own `cwd` first (a space without terminals still
 * has it), then its panes' in pane order. The choices for a new terminal.
 */
export function workspaceDirs(snapshot: Snapshot, workspaceId: string): string[] {
  const own = snapshot.spaces.find((s) => s.space_id === workspaceId)?.cwd?.trim();
  const dirs = snapshot.terminals
    .filter((pane) => pane.space_id === workspaceId)
    .map(paneDir)
    .filter((dir): dir is string => !!dir);
  return [...new Set(own ? [own, ...dirs] : dirs)];
}

/** Spaces with a directory of their own and no terminals: what a server restart leaves behind. */
export function emptySpaces(snapshot: Snapshot): Space[] {
  const used = new Set(snapshot.terminals.map((t) => t.space_id));
  return snapshot.spaces.filter((s) => !!s.cwd?.trim() && !used.has(s.space_id));
}

/** Path segments for comparing directories; Windows paths (drive letter or UNC) ignore case. */
function comparableSegments(path: string): string[] {
  const windows = /^[a-z]:/i.test(path) || /^[\\/]{2}/.test(path);
  const segments = pathSegments(path);
  return windows ? segments.map((s) => s.toLowerCase()) : segments;
}

/**
 * The space whose root contains `dir`: the space's own directory or one of its panes' is `dir` or
 * above it. The deepest
 * such directory wins, then the lowest space number; `undefined` when no space contains it.
 */
export function spaceContaining(snapshot: Snapshot, dir: string): string | undefined {
  const target = comparableSegments(dir.trim());
  if (target.length === 0) return undefined;
  let best: { id: string; depth: number; number: number } | undefined;
  for (const space of snapshot.spaces) {
    for (const root of workspaceDirs(snapshot, space.space_id)) {
      const segments = comparableSegments(root);
      if (segments.length === 0 || segments.length > target.length) continue;
      if (!segments.every((s, i) => s === target[i])) continue;
      const better =
        !best ||
        segments.length > best.depth ||
        (segments.length === best.depth && space.number < best.number);
      if (better) best = { id: space.space_id, depth: segments.length, number: space.number };
    }
  }
  return best?.id;
}

/** The numeric segments of a version (`0.16.0-rc1` is 0, 16, 0), `null` when it has none. */
function versionParts(version: string): number[] | null {
  const parts = version
    .split("-")[0]
    .split(".")
    .map((p) => Number.parseInt(p, 10));
  return parts.length > 0 && parts.every((n) => Number.isFinite(n)) ? parts : null;
}

/**
 * The two versions of a terminal that runs on a PTY host older than the one the server ships with
 * (a host is not updated in place, #606); `null` when it is current, newer or not reported.
 */
export function olderHost(
  pane: Pick<Terminal, "host_version" | "host_bundled_version"> | undefined,
): { have: string; bundled: string } | null {
  const have = pane?.host_version;
  const bundled = pane?.host_bundled_version;
  if (!have || !bundled || have === bundled) return null;
  const a = versionParts(have);
  const b = versionParts(bundled);
  if (a && b) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      const [x, y] = [a[i] ?? 0, b[i] ?? 0];
      if (x !== y) return x < y ? { have, bundled } : null;
    }
    return null;
  }
  return { have, bundled };
}

/** The tooltip of the quiet mark on a terminal with an older host. */
export function olderHostNote(host: { have: string; bundled: string }): string {
  return `Runs on poimen-pty-host ${host.have}; the bundled host is ${host.bundled}. Restart the terminal to use the new one.`;
}

/** What the close confirmation says: an agent that is still working or waiting gets a warning. */
export function closeWarning(pane: Terminal | undefined): string {
  const agent = pane ? agentKind(pane) : undefined;
  if (!pane || !agent) return "The process in the pane ends.";
  if (pane.agent_status === "working" || pane.agent_status === "blocked") {
    const state = pane.agent_status === "working" ? "working" : "waiting for you";
    return `The ${agent} agent is ${state}; closing the pane ends it.`;
  }
  return `The ${agent} agent in the pane ends.`;
}

/** The agents named in the empty states of the Output panel and the Flow tab. */
export const TRANSCRIPT_AGENTS_LABEL = HOOK_AGENTS.filter((a) => a.transcripts)
  .map((a) => a.name)
  .join(" and ");

/**
 * Empty-state text of the Output panel and the Flow tab for `pane`; `subject` names the view
 * (`Subagent output`, `The flow`). `block` is why the pane's machine cannot be asked
 * (`machineBlock`); it comes first. It tells apart a pane without a known agent, a known agent
 * that has not reported its session yet, and a session whose transcript is not found yet.
 */
export function transcriptEmptyText(
  pane: Terminal | undefined,
  subject: string,
  block: string | null = null,
): string {
  // The machine of a remote pane cannot be asked (offline, connecting, unknown; #698).
  if (block !== null && pane) return `${subject} is unavailable: ${block}.`;
  if (!pane) {
    return `${subject} is available for ${TRANSCRIPT_AGENTS_LABEL} agents. Select a pane that runs one.`;
  }
  if (!supportsFlow(pane)) {
    return `${subject} is available for ${TRANSCRIPT_AGENTS_LABEL} agents. This pane does not run one.`;
  }
  if (flowSession(pane) === null) return "Waiting for the agent to report its session.";
  return "Waiting for the agent's transcript to appear.";
}
