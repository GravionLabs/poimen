import { Injector, afterNextRender, signal } from "@angular/core";
import type { SpaceActions } from "../core/space-actions";
import type { PaneActions } from "../core/pane-actions";
import type { Terminal } from "../core/snapshot";

/** Whether a key event belongs to an IME composition: its Enter picks a candidate and saves nothing. */
export function isComposing(event: Event): boolean {
  const key = event as KeyboardEvent;
  return key.isComposing === true || key.keyCode === 229;
}

/** Focuses the field again once change detection re-enabled it after a failed save. */
export function refocus(input: HTMLInputElement, injector: Injector): void {
  afterNextRender(() => input.focus(), { injector });
}

interface PaneRenameDeps {
  actions: PaneActions;
  injector: Injector;
  pane: (id: string) => Terminal | undefined;
}

/** The handlers of the inline terminal rename of a tree row (#1035). */
export function paneRename(deps: PaneRenameDeps) {
  const { actions } = deps;
  const finish = async (
    paneId: string,
    input: HTMLInputElement,
    commit: boolean,
  ): Promise<void> => {
    if (actions.renaming() !== paneId || actions.renameBusy()) return;
    if (!commit) {
      actions.cancelRename();
      return;
    }
    if (await actions.rename(paneId, input.value)) actions.cancelRename();
    else refocus(input, deps.injector);
  };
  return {
    finish,
    start: (paneId: string, event: Event): void => {
      event.stopPropagation();
      actions.askRename(paneId);
    },
    /** Enter saves unless it ends an IME composition. */
    enter: (paneId: string, input: HTMLInputElement, event: Event): void => {
      if (!isComposing(event)) void finish(paneId, input, true);
    },
    /** F2 on the focused terminal row (tree item or card row) starts the rename. */
    onKey: (event: Event): void => {
      if ((event as KeyboardEvent).key !== "F2" || event.target instanceof HTMLInputElement) return;
      const item = (event.target as HTMLElement).closest('[role="treeitem"]');
      const id =
        // A tree item holds its children: take the row that belongs to this item itself.
        [...(item?.querySelectorAll<HTMLElement>("[data-pane]") ?? [])].find(
          (row) => row.closest('[role="treeitem"]') === item,
        )?.dataset["pane"] ?? (item as HTMLElement | null)?.dataset["nav"];
      if (!id || !deps.pane(id)) return;
      event.preventDefault();
      actions.askRename(id);
    },
  };
}

/** The inline rename of a space row: which one is edited, start and finish. */
export function spaceRename(deps: { actions: SpaceActions; closing: (id: string) => boolean }) {
  const editing = signal<string | null>(null);
  return {
    editing,
    start: (id: string, event: Event): void => {
      event.stopPropagation();
      if (deps.closing(id)) return;
      deps.actions.clearRenameError();
      editing.set(id);
    },
    /** Enter or blur confirms, Escape cancels; a failed rename keeps editing, the old label stays. */
    finish: async (id: string, input: HTMLInputElement, commit: boolean): Promise<void> => {
      if (editing() !== id) return;
      if (!commit) {
        editing.set(null);
        deps.actions.clearRenameError();
        return;
      }
      if (await deps.actions.rename(id, input.value)) editing.set(null);
      else input.focus();
    },
  };
}
