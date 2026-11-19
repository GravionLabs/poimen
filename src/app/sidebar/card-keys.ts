import { afterNextRender, computed, signal, type Injector, type Signal } from "@angular/core";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import { cardNav, cardStopsOf, tabStop, type CardItem, type CardNavResult } from "./card-nav";

/** The result of a key event on a focused header or row; modifiers, buttons and inputs give none. */
export function cardKeyResult(items: readonly CardItem[], event: KeyboardEvent): CardNavResult {
  const key = (event.target as HTMLElement | null)?.dataset?.["nav"];
  if (key === undefined || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey)
    return null;
  return cardNav(items, key, event.key);
}

/** Focuses the header or row with this key and scrolls it into view. */
export function focusNav(root: HTMLElement, key: string): void {
  const el = Array.from(root.querySelectorAll<HTMLElement>("[data-nav]")).find(
    (e) => e.dataset["nav"] === key,
  );
  el?.focus();
  el?.scrollIntoView?.({ block: "nearest" });
}

/** What the sidebar gives the keyboard handling of the cards. */
export interface CardKeyboardDeps {
  nodes: Signal<readonly HxTreeNode[]>;
  collapsed: Signal<readonly string[]>;
  filtering: Signal<boolean>;
  toggle: (id: string, collapsed: boolean) => void;
  open: (key: string) => void;
  root: () => HTMLElement;
  injector: Injector;
}

/** Cursor, roving tab stop and the key and focus handlers of the cards list (#937). */
export function cardKeyboard(deps: CardKeyboardDeps) {
  const cursor = signal<string | null>(null);
  const items = computed(() => cardStopsOf(deps.nodes(), deps.collapsed(), deps.filtering()));
  return {
    /** The one tab stop of the list. */
    stop: computed(() => tabStop(items(), cursor())),
    onKey: (event: KeyboardEvent): void => {
      const result = cardKeyResult(items(), event);
      if (!result) return;
      event.preventDefault();
      if (result.action === "focus") {
        const key = result.key;
        cursor.set(key);
        afterNextRender(() => focusNav(deps.root(), key), { injector: deps.injector });
      } else if (result.action === "toggle") deps.toggle(result.key, !result.expanded);
      else deps.open(result.key);
    },
    /** The focus moved to a header or row (Tab, a click or a key): it is the cursor. */
    onFocus: (event: FocusEvent): void => {
      const key = (event.target as HTMLElement | null)?.dataset?.["nav"];
      if (key !== undefined) cursor.set(key);
    },
  };
}
