import { agentKind, type AgentStatus, type SpaceNode } from "../core/herdr";

/** One pane of a space as the overview lists it. */
export interface OverviewRow {
  paneId: string;
  /** Agent or plain terminal. */
  kind: "agent" | "terminal";
  /** The agent's name (`claude`, ...) for an agent pane. */
  agent?: string;
  label: string;
  /** Session status of the pane (what the backend reports). */
  status: AgentStatus;
  /** Epoch ms of the latest agent notification; `undefined` when none was reported. */
  activityAt?: number;
}

/** The panes of `space` in sidebar order. */
export function overviewRows(space: SpaceNode): OverviewRow[] {
  return space.rows.map((row) => {
    const agent = agentKind(row.pane);
    return {
      paneId: row.pane.terminal_id,
      kind: agent ? "agent" : "terminal",
      agent,
      label: row.name,
      status: row.pane.agent_status,
      activityAt: row.pane.agent_message_at,
    };
  });
}

/** "just now", "5 min ago", "2 h ago", "3 d ago"; "-" without a time. */
export function activityText(at: number | undefined, now: number): string {
  if (at === undefined) return "-";
  const minutes = Math.floor(Math.max(0, now - at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

/** The pull request URL of the first pane of `space` whose branch has one. */
export function spacePullUrl(space: SpaceNode): string | undefined {
  return space.rows.find((r) => r.pr?.url)?.pr?.url;
}

/** The first directory of a pane of `space`, for "Open in explorer". */
export function spaceDirectory(space: SpaceNode): string | undefined {
  return space.rows.find((r) => r.dir)?.dir;
}
