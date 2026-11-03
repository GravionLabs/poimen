// Shapes of herdr's workspace.list / tab.list / pane.list results (only the fields poimen uses).

export type AgentStatus = "idle" | "working" | "blocked" | "done" | "unknown";

export interface Workspace {
  workspace_id: string;
  label: string;
  number: number;
  agent_status: AgentStatus;
}

export interface Tab {
  tab_id: string;
  workspace_id: string;
  label: string;
  number: number;
  agent_status: AgentStatus;
}

export interface Pane {
  pane_id: string;
  tab_id: string;
  workspace_id: string;
  agent?: string;
  /** The agent's own session (Claude Code, Copilot CLI: the transcript's session id in `value`). */
  agent_session?: { value?: string };
  agent_status: AgentStatus;
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
}

export interface Snapshot {
  workspaces: Workspace[];
  tabs: Tab[];
  panes: Pane[];
}

/** Payload of the `herdr` Tauri event: a herdr event or a poimen connection event. */
export interface HerdrEvent {
  kind: string;
  data: { pane_id?: string; agent_status?: AgentStatus; reason?: string } | null;
}

/** How a terminal view attaches: as one of many observers or as the single controller. */
export type Mode = "observe" | "control";

/** One record of a terminal stream (payload of the `terminal` Tauri event, per session id). */
export type StreamRecord =
  | { type: "frame"; seq: number; full: boolean; width: number; height: number; bytes: string }
  | { type: "closed"; reason: string };

export interface TabNode {
  tab: Tab;
  panes: Pane[];
}

export interface WorkspaceNode {
  workspace: Workspace;
  tabs: TabNode[];
}

const byNumber = <T extends { number: number }>(a: T, b: T) => a.number - b.number;

export function buildTree(snapshot: Snapshot): WorkspaceNode[] {
  return [...snapshot.workspaces].sort(byNumber).map((workspace) => ({
    workspace,
    tabs: snapshot.tabs
      .filter((tab) => tab.workspace_id === workspace.workspace_id)
      .sort(byNumber)
      .map((tab) => ({
        tab,
        panes: snapshot.panes.filter((pane) => pane.tab_id === tab.tab_id),
      })),
  }));
}

function basename(path: string): string {
  return path.replace(/\/+$/, "").split("/").pop() ?? path;
}

export function paneLabel(pane: Pane): string {
  return (
    pane.terminal_title_stripped?.trim() ||
    pane.agent ||
    (pane.cwd ? basename(pane.cwd) : "") ||
    pane.pane_id
  );
}

/**
 * Sidebar name of a pane row. herdr's own sidebar shows an agent pane by its session title and a
 * plain terminal by its label: the pane label, else the tab label (`herdr tab list`, e.g.
 * "terminal"). The shell's terminal title (`user@host:~/path`) is never shown. With no label at
 * all, the directory name stands in.
 */
export function rowName(pane: Pane, tabLabel?: string): string {
  if (agentKind(pane)) return paneLabel(pane);
  return (
    pane.label?.trim() ||
    tabLabel?.trim() ||
    (paneDir(pane) ? basename(paneDir(pane)!) : "") ||
    pane.pane_id
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
export function waitingCount(panes: Pane[]): number {
  return panes.filter((pane) => pane.agent_status === "blocked").length;
}

/** The blocked pane after `activeId` in list order, wrapping around; undefined when none waits. */
export function nextWaiting(panes: Pane[], activeId: string | null): Pane | undefined {
  const waiting = panes.filter((pane) => pane.agent_status === "blocked");
  if (!waiting.length) return undefined;
  const index = panes.findIndex((pane) => pane.pane_id === activeId);
  const after = panes.slice(index + 1).find((pane) => pane.agent_status === "blocked");
  return after ?? waiting[0];
}

/** Applies a pane.agent_status_changed event without a full refresh. */
export function applyStatus(snapshot: Snapshot, paneId: string, status: AgentStatus): Snapshot {
  return {
    ...snapshot,
    panes: snapshot.panes.map((pane) =>
      pane.pane_id === paneId ? { ...pane, agent_status: status } : pane,
    ),
  };
}

/** The directory to show for a pane. */
export function paneDir(pane: Pane): string | undefined {
  return pane.foreground_cwd || pane.cwd || undefined;
}

/** Agent kind of a pane (`claude`, `codex`, ...); `undefined` for a plain terminal. */
export function agentKind(pane: Pane): string | undefined {
  return pane.agent?.trim() || undefined;
}

/** Agents whose session transcripts poimen reads (Output panel, Flow tab), with their names. */
const TRANSCRIPT_AGENTS: Readonly<Record<string, string>> = {
  claude: "Claude Code",
  copilot: "GitHub Copilot CLI",
};

/** Display name of the agent of `pane` if its transcripts can be followed (`Claude Code`, ...). */
export function flowAgentName(pane: Pane | undefined): string | undefined {
  const kind = pane && agentKind(pane);
  return kind !== undefined && Object.hasOwn(TRANSCRIPT_AGENTS, kind)
    ? TRANSCRIPT_AGENTS[kind]
    : undefined;
}

/** Whether the Output panel and the Flow tab can follow the agent of `pane`. */
export function supportsFlow(pane: Pane | undefined): boolean {
  return flowAgentName(pane) !== undefined;
}

/** The session id the transcripts of `pane` are found by; `null` when it has none to follow. */
export function flowSession(pane: Pane | undefined): string | null {
  return (supportsFlow(pane) && pane?.agent_session?.value) || null;
}

/**
 * Row text of an agent pane: its state label (`agent_status`, as `herdr pane list` reports it)
 * plus the `deliver_step` token when an integration set one. herdr reports no free-form status
 * message, so plain terminals and agents with an unknown state have no text.
 */
export function statusText(pane: Pane): string | undefined {
  if (!agentKind(pane)) return undefined;
  const state = pane.agent_status === "unknown" ? undefined : pane.agent_status;
  const step = pane.tokens?.["deliver_step"]?.trim() || undefined;
  return [state, step].filter(Boolean).join(" · ") || undefined;
}

/** The last two path segments, so rows stay short. */
export function shortPath(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/").filter(Boolean);
  return parts.length <= 2 ? path : "…/" + parts.slice(-2).join("/");
}

/** What a sidebar row shows for one pane. */
export interface PaneRow {
  pane: Pane;
  tab: string;
  name: string;
  agent?: string;
  branch?: string;
  worktree: boolean;
  dir?: string;
  text?: string;
}

export interface SpaceNode {
  workspace: Workspace;
  rows: PaneRow[];
}

/** Workspaces with their pane rows (tab label as secondary text), enriched with repo info. */
export function buildSpaces(snapshot: Snapshot, repos: ReadonlyMap<string, RepoInfo>): SpaceNode[] {
  return buildTree(snapshot).map(({ workspace, tabs }) => ({
    workspace,
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

/** `herdr <version>`, or `herdr ?` when herdr is unreachable or too old to report it. */
export function herdrVersionLabel(info: HerdrInfo | null | undefined): string {
  return info?.version ? `herdr ${info.version}` : "herdr ?";
}

/** `poimen <version>`, or `poimen ?` until the build version is known. */
export function appVersionLabel(version: string | null | undefined): string {
  return version ? `poimen ${version}` : "poimen ?";
}

/** The tooltip warning for a herdr protocol other than the supported one; empty when it matches or is unknown. */
export function protocolWarning(info: HerdrInfo | null | undefined): string {
  if (!info || info.protocol == null || info.protocol === info.supported) return "";
  return `herdr protocol ${info.protocol}, poimen supports ${info.supported}`;
}

/** Where a split puts the new pane (`pane.split` direction). */
export type SplitDirection = "right" | "down";

/** The distinct directories of a workspace's panes, in pane order: the choices for a new terminal. */
export function workspaceDirs(snapshot: Snapshot, workspaceId: string): string[] {
  const dirs = snapshot.panes
    .filter((pane) => pane.workspace_id === workspaceId)
    .map(paneDir)
    .filter((dir): dir is string => !!dir);
  return [...new Set(dirs)];
}

/** What the close confirmation says: an agent that is still working or waiting gets a warning. */
export function closeWarning(pane: Pane | undefined): string {
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

/** An action a herdr plugin offers (answer of the `plugin_actions` command). */
export interface PluginAction {
  plugin_id: string;
  action_id: string;
  title: string;
  description: string | null;
}

/** Where a plugin action starts: the focused pane and its tab and workspace (herdr's context). */
export interface PluginContext {
  focused_pane_id: string | null;
  focused_pane_cwd: string | null;
  focused_pane_agent: string | null;
  tab_id: string | null;
  tab_label: string | null;
  workspace_id: string | null;
  workspace_label: string | null;
}

/** The context for `paneId` (the focused pane); all `null` when there is no such pane. */
export function pluginContext(snapshot: Snapshot, paneId: string | null): PluginContext {
  const pane = paneId ? snapshot.panes.find((p) => p.pane_id === paneId) : undefined;
  const tab = pane && snapshot.tabs.find((t) => t.tab_id === pane.tab_id);
  const workspace = pane && snapshot.workspaces.find((w) => w.workspace_id === pane.workspace_id);
  return {
    focused_pane_id: pane?.pane_id ?? null,
    focused_pane_cwd: pane?.foreground_cwd ?? pane?.cwd ?? null,
    focused_pane_agent: pane?.agent ?? null,
    tab_id: pane?.tab_id ?? null,
    tab_label: tab?.label ?? null,
    workspace_id: pane?.workspace_id ?? null,
    workspace_label: workspace?.label ?? null,
  };
}
