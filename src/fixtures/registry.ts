import type { Fixture } from "./fixture";
import { CONTEXT_MENU_FIXTURES } from "../app/ui/context-menu/context-menu.fixtures";
import { DIALOG_SHELL_FIXTURES } from "../app/ui/dialog-shell/dialog-shell.fixtures";
import { TAB_BAR_FIXTURES } from "../app/ui/helix-candidates/tab-bar/tab-bar.fixtures";
import { SPACE_CARD_FIXTURES } from "../app/sidebar/space-card.fixtures";
import { STATUS_BAR_FIXTURES } from "../app/status-bar/status-bar.fixtures";

/** Every fixture; a new `*.fixtures.ts` is added here. */
export const FIXTURES: readonly Fixture[] = [
  ...DIALOG_SHELL_FIXTURES,
  ...CONTEXT_MENU_FIXTURES,
  ...SPACE_CARD_FIXTURES,
  ...TAB_BAR_FIXTURES,
  ...STATUS_BAR_FIXTURES,
];

export function findFixture(id: string): Fixture | undefined {
  return FIXTURES.find((f) => f.id === id);
}

export function listFixtures(): { id: string; title: string; viewport?: Fixture["viewport"] }[] {
  return FIXTURES.map(({ id, title, viewport }) => ({ id, title, viewport }));
}
