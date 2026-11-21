import { Injectable, inject, signal } from "@angular/core";
import { SettingsStore } from "./settings";
import { TauriBridge } from "./tauri";

/** A limit window as `usage_claude` returns it. */
export interface LimitWindow {
  label: string;
  /** 0 to 100; `null` when the provider did not say. */
  usedPercent: number | null;
  resetsAt: string | null;
}

/** The status bar item of the limits. */
export interface LimitsItem {
  /** The tightest window: one percentage (the status bar's narrower forms, #1034). */
  text: string;
  /** Every window with its percentage; the status bar shows it while it has room. Falls back to `text`. */
  full?: string;
  title: string;
  warning: boolean;
}

/** From this use on the item gets the warning color. */
export const WARN_PERCENT = 80;
/** The limits are asked again this often while the app runs. */
export const REFRESH_MS = 5 * 60_000;
/** Never more often than this, however often the window gets focus. */
export const MIN_GAP_MS = 60_000;

/** The window with the highest known use. */
export function tightest(windows: readonly LimitWindow[]): LimitWindow | null {
  let best: LimitWindow | null = null;
  for (const w of windows) {
    if (w.usedPercent !== null && (best === null || w.usedPercent > (best.usedPercent ?? -1))) {
      best = w;
    }
  }
  return best;
}

function percentText(value: number | null): string {
  return value === null ? "unknown" : `${Math.round(value)} %`;
}

function resetText(iso: string | null): string {
  if (!iso) return "reset unknown";
  const at = new Date(iso);
  return Number.isNaN(at.getTime()) ? "reset unknown" : `resets ${at.toLocaleString()}`;
}

/** The status bar item: the tightest window, a warning from 80 %, every window in the tooltip. `null` hides it. */
export function limitsItem(windows: readonly LimitWindow[]): LimitsItem | null {
  const top = tightest(windows);
  if (!top) return null;
  const lines = windows.map(
    (w) => `${w.label}: ${percentText(w.usedPercent)}, ${resetText(w.resetsAt)}`,
  );
  return {
    text: `${top.label} ${percentText(top.usedPercent)}`,
    full: windows.map((w) => `${w.label} ${percentText(w.usedPercent)}`).join(" · "),
    title: `Claude limits\n${lines.join("\n")}`,
    warning: (top.usedPercent ?? 0) >= WARN_PERCENT,
  };
}

/**
 * The Claude limits for the status bar (#784). Asks `usage_claude` at start, every 5 minutes and
 * when the window gets focus, never more often than once a minute, and only while the app runs.
 * `usage.claude` off asks nothing. A failure hides the item and is reported once per cause; it
 * affects nothing else.
 */
@Injectable({ providedIn: "root" })
export class ClaudeLimits {
  private readonly bridge = inject(TauriBridge);
  private readonly prefs = inject(SettingsStore);
  private readonly _item = signal<LimitsItem | null>(null);
  /** The item to show; `null` hides it. */
  readonly item = this._item.asReadonly();
  private last = Number.NEGATIVE_INFINITY;
  private reported = new Set<string>();

  /** Asks now unless the last ask was less than a minute ago. */
  async refresh(now = Date.now()): Promise<void> {
    if (!this.prefs.settings().usage.claude) {
      this._item.set(null);
      return;
    }
    if (now - this.last < MIN_GAP_MS) return;
    this.last = now;
    try {
      const windows = await this.bridge.invoke<LimitWindow[]>("usage_claude");
      this._item.set(limitsItem(windows));
    } catch (e) {
      this._item.set(null);
      const cause = typeof e === "string" ? e : e instanceof Error ? e.message : "unknown error";
      if (!this.reported.has(cause)) {
        this.reported.add(cause);
        console.warn(`poimen settings: usage.claude: ${cause}; the limits are not shown`);
      }
    }
  }

  /** Starts the timer and the focus listener; returns the stop function. */
  start(): () => void {
    void this.refresh();
    const timer = setInterval(() => void this.refresh(), REFRESH_MS);
    const onFocus = () => void this.refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }
}
