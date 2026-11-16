import type { OpenAt } from "./editor-tabs";
import { tabTerminal, type Snapshot, type Tab } from "./snapshot";

/** The editor group `Tab.place` names for a tab (`right` is a `row` split, `down` a `column`). */
const DIRECTION = { right: "row", down: "column" } as const;

/** The terminal a tab shows, by tab id. */
function paneOfTab(snapshot: Snapshot, tabId: string): string | undefined {
  const tab = snapshot.tabs.find((t) => t.tab_id === tabId);
  return tab ? tabTerminal(tab, snapshot.terminals)?.terminal_id : undefined;
}

/**
 * Where the pane of `tab` opens (#657): in an editor group split off the group of `place.of`, else
 * in the group of its parent tab, else (a top-level tab) in the main group. `undefined` for a space
 * without any layout tab that has a parent or a placement, which keeps the plain behavior (the
 * focused group); an `of` or parent without a terminal degrades to the next rule.
 */
export function openAtFor(snapshot: Snapshot, tab: Tab | undefined): OpenAt | undefined {
  if (!tab) return undefined;
  const siblings = snapshot.tabs.filter((t) => t.space_id === tab.space_id);
  if (!siblings.some((t) => t.place || t.parent)) return undefined;
  const ofPane = tab.place ? paneOfTab(snapshot, tab.place.of) : undefined;
  if (tab.place && ofPane) {
    const dir = DIRECTION[tab.place.direction];
    return {
      place: {
        ofPaneId: ofPane,
        dir,
        ratio: tab.place.ratio,
        key: `${ofPane}|${dir}|${tab.place.ratio}`,
      },
    };
  }
  const parentPane = tab.parent ? paneOfTab(snapshot, tab.parent) : undefined;
  return parentPane ? { besidePaneId: parentPane } : { main: true };
}
