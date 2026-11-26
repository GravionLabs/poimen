import { agentMonogram, agentName, derivedMonogram } from "./backend";
import { plainText } from "./plain-text";
import type { Terminal } from "./snapshot";
import { agentBadge, type IconName } from "../ui/icon/icon";

/**
 * The kind of what runs in a pane: the hook agent's (#1314). A program's own status (OSC 7501)
 * names itself in `agent`, but the pane still runs a shell.
 */
function paneKind(pane: Terminal | undefined): string | undefined {
  return pane?.hook_agent?.trim() || undefined;
}

/**
 * The icon of what runs in a pane (#1215): the shell icon for a shell, the monogram badge of a
 * known agent kind, a monogram derived from the name for a harness without one, the generic agent icon for a kind with no usable name.
 */
export function paneKindIcon(pane: Terminal | undefined): IconName {
  const kind = paneKind(pane);
  if (!kind) return "terminal";
  const mono = agentMonogram(kind) ?? derivedMonogram(agentName(kind));
  return mono === "?" ? "agent" : agentBadge(mono);
}

/**
 * The tooltip and accessible name of a pane's kind icon: the agent, for a shell the title its
 * program reports or "Terminal", and the mode when read-only.
 */
export function paneKindLabel(pane: Terminal | undefined, readOnly: boolean): string {
  const kind = paneKind(pane);
  const name = kind ? agentName(kind) : plainText(pane?.agent_title) || "Terminal";
  return readOnly ? `${name} · Read-only` : name;
}
