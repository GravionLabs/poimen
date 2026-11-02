import { Injectable, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";

export type ThemeSetting = "light" | "dark" | "system";

/** The resolved user settings from `~/.config/poimen/config.json` (wire shape of `settings.rs`). */
export interface Settings {
  theme: ThemeSetting;
  /** `null` keeps the app's default monospace stack. */
  fontFamily: string | null;
  fontSize: number;
  /** Terminal colors taken from Ghostty; `null` keeps the theme colors. */
  colors: { background: string | null; foreground: string | null } | null;
  warnings: string[];
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  fontFamily: null,
  fontSize: 13,
  colors: null,
  warnings: [],
};

/** Accepts a payload from the backend; anything malformed falls back to the defaults. */
export function parseSettings(raw: unknown): Settings {
  if (typeof raw !== "object" || raw === null) return DEFAULT_SETTINGS;
  const r = raw as Record<string, unknown>;
  const colors = r["colors"] as Record<string, unknown> | null | undefined;
  const color = (v: unknown) => (typeof v === "string" ? v : null);
  return {
    theme: r["theme"] === "light" || r["theme"] === "dark" ? r["theme"] : "system",
    fontFamily: typeof r["fontFamily"] === "string" ? r["fontFamily"] : null,
    fontSize:
      typeof r["fontSize"] === "number" && Number.isFinite(r["fontSize"])
        ? r["fontSize"]
        : DEFAULT_SETTINGS.fontSize,
    colors:
      colors && typeof colors === "object"
        ? { background: color(colors["background"]), foreground: color(colors["foreground"]) }
        : null,
    warnings: Array.isArray(r["warnings"])
      ? r["warnings"].filter((w): w is string => typeof w === "string")
      : [],
  };
}

/** User preferences; the backend watches the file, so edits apply without a restart. */
@Injectable({ providedIn: "root" })
export class SettingsStore {
  private readonly bridge = inject(TauriBridge);
  private readonly _settings = signal<Settings>(DEFAULT_SETTINGS);
  readonly settings = this._settings.asReadonly();

  async start(): Promise<void> {
    await this.bridge.listen<unknown>("settings", (s) => this.apply(s));
    try {
      this.apply(await this.bridge.invoke("get_settings"));
    } catch {
      // keep the defaults
    }
  }

  private apply(raw: unknown): void {
    const next = parseSettings(raw);
    for (const w of next.warnings) console.warn(`poimen settings: ${w}`);
    this._settings.set(next);
  }
}
