import { DestroyRef, Injectable, inject, signal } from "@angular/core";

/** Input older than this means nobody is at the machine (not a setting). */
export const PRESENCE_MS = 60_000;

/**
 * Whether the user is looking at the window: visible, focused and with pointer, key or wheel input
 * in the last `PRESENCE_MS`. Focus alone does not count, so after a return the marks clear on the
 * first input, not at the moment of return. Unknown presence is not present (orca: "Unknown
 * presence must not clear unread attention").
 */
@Injectable({ providedIn: "root" })
export class Presence {
  private readonly _present = signal(false);
  readonly present = this._present.asReadonly();
  private readonly destroyRef = inject(DestroyRef);
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    if (typeof window === "undefined") return;
    const input = () => this.input();
    const leave = () => this.set(false);
    // Capture phase: xterm stops keydown events before they bubble.
    const inputs = ["keydown", "pointerdown", "pointermove", "wheel"] as const;
    for (const type of inputs)
      window.addEventListener(type, input, { capture: true, passive: true });
    const visibility = () => {
      if (document.visibilityState === "hidden") leave();
    };
    window.addEventListener("blur", leave);
    document.addEventListener("visibilitychange", visibility);
    this.destroyRef.onDestroy(() => {
      clearTimeout(this.timer);
      for (const type of inputs) window.removeEventListener(type, input, { capture: true });
      window.removeEventListener("blur", leave);
      document.removeEventListener("visibilitychange", visibility);
    });
  }

  /** The panes as a set while the user is present, else an empty set (nothing is viewed). */
  whenPresent(panes: readonly string[]): ReadonlySet<string> {
    return this._present() ? new Set(panes) : new Set();
  }

  /** Records input now. Ignored while the window is hidden or without the focus. */
  input(): void {
    if (document.visibilityState === "hidden" || !document.hasFocus()) return;
    this.set(true);
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.set(false), PRESENCE_MS);
  }

  private set(value: boolean): void {
    if (!value) clearTimeout(this.timer);
    if (this._present() !== value) this._present.set(value);
  }
}
