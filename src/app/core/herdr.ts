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
  agent_status: AgentStatus;
  cwd?: string;
  terminal_title_stripped?: string;
  focused?: boolean;
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

/** Panes whose agent waits for the user. */
export function waitingCount(panes: Pane[]): number {
  return panes.filter((pane) => pane.agent_status === "blocked").length;
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

export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
