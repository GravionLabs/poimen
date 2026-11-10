// Shapes of herdr's workspace.list / tab.list / pane.list results (only the fields poimen uses).

import type { Checks } from "../github/pulls";
import type { BackendKind } from "./settings";

export type AgentStatus = "idle" | "working" | "blocked" | "done" | "unknown";

export interface Space {
  space_id: string;
  label: string;
  number: number;
  agent_status: AgentStatus;
  /**
   * The space's own directory (poimen backend: persisted in `server.json`, so it outlives the
   * terminals across a server restart); herdr workspaces have none.
   */
  cwd?: string;
}

export interface Tab {
  tab_id: string;
  space_id: string;
  label: string;
  number: number;
  agent_status: AgentStatus;
  /**
   * The split tree of the tab as the server keeps it (poimen backend); absent for a herdr tab and
   * for a terminal without a leaf (an exited one), which is a tab of its own.
   */
  root?: TabTree;
  /** The focused leaf of the tab, shared by every client (poimen backend). */
  active_terminal?: string;
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
  cwd?: string;
  /** Directory of the foreground process; follows `cd` and is more precise than `cwd`. */
  foreground_cwd?: string;
  /** User-set pane label (e.g. "terminal"). */
  label?: string;
  /** Free-form key/value tokens set by integrations (e.g. `deliver_step`). */
  tokens?: Record<string, string>;
  terminal_title_stripped?: string;
  focused?: boolean;
}

/** Branch and worktree flag of a directory (answer of the `repo_info` command). */
export interface RepoInfo {
  branch: string | null;
  worktree: boolean;
  /** Changed plus untracked files; null/absent when unknown. */
  dirty?: number | null;
  /** Commits ahead of / behind the upstream; null/absent without upstream. */
  ahead?: number | null;
  behind?: number | null;
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
 * The records of the `snapshot` command as the backends deliver them (herdr's workspace.list,
 * tab.list and pane.list shape). Only `toSnapshot` reads them; everything else works on `Snapshot`.
 */
export interface WireSnapshot {
  workspaces: (Omit<Space, "space_id"> & { workspace_id: string })[];
  tabs: (Omit<Tab, "space_id"> & { workspace_id: string })[];
  panes: (Omit<Terminal, "terminal_id" | "space_id"> & {
    pane_id: string;
    workspace_id: string;
  })[];
}

/** Maps the wire shape of the `snapshot` command to the neutral model (ids keep their values). */
export function toSnapshot(wire: WireSnapshot): Snapshot {
  return {
    spaces: wire.workspaces.map(({ workspace_id, ...rest }) => ({
      ...rest,
      space_id: workspace_id,
    })),
    tabs: wire.tabs.map(({ workspace_id, ...rest }) => ({ ...rest, space_id: workspace_id })),
    terminals: wire.panes.map(({ pane_id, workspace_id, ...rest }) => ({
      ...rest,
      terminal_id: pane_id,
      space_id: workspace_id,
    })),
  };
}

/** Payload of the `herdr` Tauri event: a herdr event or a poimen connection event. */
export interface HerdrEvent {
  kind: string;
  data: {
    pane_id?: string;
    agent_status?: AgentStatus;
    agent_message?: string;
    agent_message_at?: number;
    reason?: string;
  } | null;
}

/** How a terminal view attaches: as one of many observers or as the single controller. */
export type Mode = "observe" | "control";

/**
 * One record of a terminal stream (sent through the channel of `open_terminal`), the same
 * for every backend. `bytes` is base64.
 */
export type StreamRecord =
  | { type: "reset"; cols: number; rows: number; bytes: string }
  | { type: "output"; bytes: string }
  | { type: "resized"; cols: number; rows: number }
  | { type: "controller"; attached: boolean }
  | {
      type: "closed";
      reason: string;
      exit_code?: number;
      /**
       * Set when the reason is more than text; `controller_held`: another client is the
       * controller; `exited`: the process ended (`exit_code` says how) and its last screen stays.
       */
      code?: "controller_held" | "exited";
    }
  /** Raw PTY bytes of an emulated stream (an `ArrayBuffer` over the channel). */
  | { type: "bytes"; data: Uint8Array };

/** What `open_terminal` answers: the stream id and whether the stream is emulated. */
export interface OpenedStream {
  id: number;
  /** Raw PTY bytes (xterm.js tracks the modes) rather than finished frames. */
  emulated: boolean;
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
 * The spaces in the user's order: ids of `order` first (in that order), then the others in backend
 * number order (new spaces). Ids of `order` that no longer exist are skipped.
 */
export function orderSpaces<T extends { space_id: string; number: number }>(
  spaces: readonly T[],
  order: readonly string[] = [],
): T[] {
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
): WorkspaceNode[] {
  return orderSpaces(snapshot.spaces, order).map((workspace) => ({
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
  return panes.filter((pane) => pane.agent_status === "blocked").length;
}

/** The blocked pane after `activeId` in list order, wrapping around; undefined when none waits. */
export function nextWaiting(panes: Terminal[], activeId: string | null): Terminal | undefined {
  const waiting = panes.filter((pane) => pane.agent_status === "blocked");
  if (!waiting.length) return undefined;
  const index = panes.findIndex((pane) => pane.terminal_id === activeId);
  const after = panes.slice(index + 1).find((pane) => pane.agent_status === "blocked");
  return after ?? waiting[0];
}

/**
 * Applies a pane.agent_status_changed event without a full refresh. A notification text replaces
 * the kept one; without one the text stays, except when the agent turns `working` or ends
 * (`unknown`).
 */
export function applyStatus(
  snapshot: Snapshot,
  paneId: string,
  status: AgentStatus,
  message?: { text: string; at: number },
): Snapshot {
  return {
    ...snapshot,
    terminals: snapshot.terminals.map((pane) => {
      if (pane.terminal_id !== paneId) return pane;
      if (message) {
        return {
          ...pane,
          agent_status: status,
          agent_message: message.text,
          agent_message_at: message.at,
        };
      }
      const next = { ...pane, agent_status: status };
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

/** Agents whose session transcripts poimen reads (Output panel, Flow tab), with their names. */
const TRANSCRIPT_AGENTS: Readonly<Record<string, string>> = {
  claude: "Claude Code",
  copilot: "GitHub Copilot CLI",
};

/** Display name of the agent of `pane` if its transcripts can be followed (`Claude Code`, ...). */
export function flowAgentName(pane: Terminal | undefined): string | undefined {
  const kind = pane && agentKind(pane);
  return kind !== undefined && Object.hasOwn(TRANSCRIPT_AGENTS, kind)
    ? TRANSCRIPT_AGENTS[kind]
    : undefined;
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
  const state = pane.agent_status === "unknown" ? undefined : pane.agent_status;
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
}

/** Workspaces with their pane rows (tab label as secondary text), enriched with repo info. */
export function buildSpaces(
  snapshot: Snapshot,
  repos: ReadonlyMap<string, RepoInfo>,
  pulls: ReadonlyMap<string, BranchPull> = new Map(),
  order: readonly string[] = [],
  tabOrder: TabOrder = {},
): SpaceNode[] {
  return buildTree(snapshot, order, tabOrder).map(({ workspace, tabs }) => ({
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
          dirty: repo?.dirty ?? undefined,
          ahead: repo?.ahead ?? undefined,
          behind: repo?.behind ?? undefined,
          pr: dir ? pulls.get(dir) : undefined,
          dir,
          text: statusText(pane),
        };
      }),
    ),
  }));
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
  return spaces
    .map((space) => ({
      ...space,
      rows: space.rows.filter((r) => matches(r, space.workspace.label)),
    }))
    .filter((space) => space.rows.length > 0);
}

export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** The herdr version and protocol reported by `ping`; `supported` is the protocol poimen is written for. */
export interface HerdrInfo {
  version: string;
  /** Absent when herdr reports a version but no protocol: nothing to check. */
  protocol?: number | null;
  supported: number;
}

/** `backend herdr <version>`, or `backend herdr ?` when unreachable or too old to report it; `backend` names another server. */
export function herdrVersionLabel(info: HerdrInfo | null | undefined, backend = "herdr"): string {
  return info?.version ? `backend ${backend} ${info.version}` : `backend ${backend} ?`;
}

/** The release channel of the build (`app_channel`): `rc` is the "Poimen RC" app installed beside the stable one. */
export type AppChannel = "stable" | "rc";

/** The window title: `poimen`, `poimen RC` on rc, with ` (<n> waiting)` while agents wait. */
export function windowTitle(channel: AppChannel, waiting: number): string {
  const name = channel === "rc" ? "poimen RC" : "poimen";
  return waiting > 0 ? `${name} (${waiting} waiting)` : name;
}

/** `frontend poimen <version>`, or `frontend poimen ?` until the build version is known. */
export function appVersionLabel(version: string | null | undefined): string {
  return version ? `frontend poimen ${version}` : "frontend poimen ?";
}

/** The tooltip warning for a herdr protocol other than the supported one; empty when it matches or is unknown. */
export function protocolWarning(info: HerdrInfo | null | undefined, backend = "herdr"): string {
  if (!info || info.protocol == null || info.protocol === info.supported) return "";
  return `${backend} protocol ${info.protocol}, poimen supports ${info.supported}`;
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

/** Agent kinds `agent.start` supports (`herdr agent start --help`). */
export const AGENT_KINDS = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "copilot",
  "cursor",
  "pi",
  "devin",
  "agy",
  "cline",
  "omp",
  "mastracode",
  "kimi",
  "kiro",
  "droid",
  "amp",
  "grok",
  "hermes",
  "kilo",
  "qodercli",
  "qwen",
  "letta",
  "maki",
  "muse",
] as const;

/** Agent kinds that report their state through the poimen hooks
 * (`poimen-server integration install claude|copilot`); `start_agent` waits for their
 * `SessionStart`, so any other kind would time out although it runs. */
export const HOOK_AGENT_KINDS = ["claude", "copilot"] as const;

/** The kinds the "Start agent" dialog offers on `backend`: herdr detects all of `AGENT_KINDS`,
 * the poimen backend only learns of the ones with hooks. */
export function agentKinds(backend: BackendKind): readonly string[] {
  return backend === "poimen" ? HOOK_AGENT_KINDS : AGENT_KINDS;
}

/** The agents named in the empty states of the Output panel and the Flow tab. */
export const TRANSCRIPT_AGENTS_LABEL = "Claude Code and GitHub Copilot CLI";

/**
 * Empty-state text of the Output panel and the Flow tab for `pane`; `subject` names the view
 * (`Subagent output`, `The flow`). It tells apart a pane without a known agent, a known agent
 * that has not reported its session yet, and a session whose transcript is not found yet.
 */
export function transcriptEmptyText(pane: Terminal | undefined, subject: string): string {
  if (!pane) {
    return `${subject} is available for ${TRANSCRIPT_AGENTS_LABEL} agents. Select a pane that runs one.`;
  }
  if (!supportsFlow(pane)) {
    return `${subject} is available for ${TRANSCRIPT_AGENTS_LABEL} agents. This pane does not run one.`;
  }
  if (flowSession(pane) === null) return "Waiting for the agent to report its session.";
  return "Waiting for the agent's transcript to appear.";
}
