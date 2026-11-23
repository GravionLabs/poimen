import { agentMonogram, agentName, derivedMonogram } from "./backend";
import { agentKind, type Terminal } from "./snapshot";
import { agentBadge, type IconName } from "../ui/icon/icon";

/**
 * The icon of what runs in a pane (#1215): the terminal icon for a shell, the monogram badge of a
 * known agent kind, a monogram derived from the name for a harness without one, the generic agent icon for a kind with no usable name.
 */
export function paneKindIcon(pane: Terminal | undefined): IconName {
  const kind = pane && (agentKind(pane) ?? pane.agent_kind?.trim());
  if (!kind) return "terminal";
  const mono = agentMonogram(kind) ?? derivedMonogram(agentName(kind));
  return mono === "?" ? "agent" : agentBadge(mono);
}

/** The tooltip and accessible name of a pane's kind icon: the kind, and the mode when read-only. */
export function paneKindLabel(pane: Terminal | undefined, readOnly: boolean): string {
  const kind = pane && (agentKind(pane) ?? pane.agent_kind?.trim());
  const name = kind ? agentName(kind) : "Terminal";
  return readOnly ? `${name} · Read-only` : name;
}
