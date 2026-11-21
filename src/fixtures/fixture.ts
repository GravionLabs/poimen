import type { Provider, Type } from "@angular/core";

/**
 * One named state of a presentational component (#1020). The id is `<component>/<state>`, e.g.
 * `dialog-shell/confirm`; `pnpm fixtures:list` prints them, `pnpm fixtures:shot <id>` takes the
 * picture. Fixtures render with fixed inputs and no Tauri, backend or settings.
 */
export interface Fixture {
  id: string;
  title: string;
  /** A component, or a host component when the state needs projected content. */
  component: Type<unknown>;
  inputs?: Record<string, unknown>;
  providers?: Provider[];
  /** Browser window of the picture; default 900x600. */
  viewport?: { width: number; height: number };
}
