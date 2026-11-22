import { agentMonogram, agentName } from "./backend";
import { agentKind, type Terminal } from "./snapshot";
import { agentBadge, type IconName } from "../ui/icon/icon";

/**
 * The icon of what runs in a pane (#1215): the terminal icon for a shell, the monogram badge of a
 * known agent kind, the generic agent icon for a kind poimen does not know.
 */
export function paneKindIcon(pane: Terminal | undefined): IconName {
  const kind = pane && (agentKind(pane) ?? pane.agent_kind?.trim());
  if (!kind) return "terminal";
  const mono = agentMonogram(kind);
  return mono ? agentBadge(mono) : "agent";
}

/** The tooltip and accessible name of a pane's kind icon: the kind, and the mode when read-only. */
export function paneKindLabel(pane: Terminal | undefined, readOnly: boolean): string {
  const kind = pane && (agentKind(pane) ?? pane.agent_kind?.trim());
  const name = kind ? agentName(kind) : "Terminal";
  return readOnly ? `${name} · Read-only` : name;
}
