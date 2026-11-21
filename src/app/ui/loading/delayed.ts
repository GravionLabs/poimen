import { DestroyRef, Signal, effect, inject, signal, untracked } from "@angular/core";

/** A load shows nothing for this long, so a fast one never flashes a placeholder (#1041). */
export const SKELETON_DELAY_MS = 200;
/** A started action only shows a spinner or a changed label after this long. */
export const BUSY_DELAY_MS = 1000;

/**
 * `true` once `source` has been `true` for `delayMs` without a break; `false` at once when it turns
 * false. Views use this instead of timing a placeholder by hand. Call it in an injection context.
 */
export function delayed(source: () => boolean, delayMs: number): Signal<boolean> {
  const shown = signal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const clear = (): void => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  effect(() => {
    const on = source();
    untracked(() => {
      clear();
      if (!on) {
        shown.set(false);
      } else if (!shown()) {
        timer = setTimeout(() => {
          timer = undefined;
          shown.set(true);
        }, delayMs);
      }
    });
  });
  inject(DestroyRef).onDestroy(clear);
  return shown.asReadonly();
}

/** Skeleton rows appear after `SKELETON_DELAY_MS` of loading. */
export const delayedSkeleton = (source: () => boolean): Signal<boolean> =>
  delayed(source, SKELETON_DELAY_MS);

/** A spinner or changed label appears after `BUSY_DELAY_MS` of an action; the control is disabled from the start. */
export const delayedBusy = (source: () => boolean): Signal<boolean> =>
  delayed(source, BUSY_DELAY_MS);
