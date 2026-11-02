import { Injectable, inject, signal } from "@angular/core";
import { DEFAULT_SHORTCUTS, parseShortcuts, type Shortcuts } from "./shortcuts";
import { TauriBridge } from "./tauri";
import {
  DEFAULT_SIDEBAR_ROWS,
  isRowToken,
  type RowLines,
  type RowType,
  type SidebarRows,
} from "../sidebar/spaces";

export type ThemeSetting = "light" | "dark" | "system";

/** A scheme name, or a pair of names that `theme` picks from. */
export type ColorSchemeSetting = string | { light: string; dark: string };

/** The resolved user settings from `~/.config/poimen/config.json` (wire shape of `settings.rs`). */
export interface Settings {
  theme: ThemeSetting;
  /** The `colorSchema`: a name or a light/dark pair; `null` is the `poimen` scheme that follows `theme`. */
  colorSchema: ColorSchemeSetting | null;
  /** `null` keeps the app's default monospace stack. */
  fontFamily: string | null;
  fontSize: number;
  /** Terminal colors taken from Ghostty; `null` keeps the theme colors. */
  colors: { background: string | null; foreground: string | null } | null;
  /** Open panes for input when no other client controls them (`terminal.autoControl`). */
  autoControl: boolean;
  /** Accelerator per action: the defaults over the valid `shortcuts` of config.json. */
  shortcuts: Shortcuts;
  /** Lines and tokens per row type: the defaults over the valid `sidebar.rows` of config.json. */
  sidebarRows: SidebarRows;
  /** Roots of the explorer; a leading `~` is the home directory (resolved by the backend). */
  explorerRoots: string[];
  /** Whether the explorer shows hidden directories. */
  explorerShowHidden: boolean;
  warnings: string[];
}

/** The theme a click on the toolbar button selects: system, then light, then dark. */
export function nextTheme(theme: ThemeSetting): ThemeSetting {
  return theme === "system" ? "light" : theme === "light" ? "dark" : "system";
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  colorSchema: null,
  fontFamily: null,
  fontSize: 13,
  colors: null,
  autoControl: true,
  shortcuts: DEFAULT_SHORTCUTS,
  sidebarRows: DEFAULT_SIDEBAR_ROWS,
  explorerRoots: ["~"],
  explorerShowHidden: false,
  warnings: [],
};

/** A valid list of lines of tokens, or `null`. */
function parseRowLines(raw: unknown): RowLines | null {
  if (!Array.isArray(raw) || raw.length === 0) return null;
  const lines: RowLines = [];
  for (const l of raw) {
    if (!Array.isArray(l) || l.length === 0) return null;
    const line: RowLines[number] = [];
    for (const t of l) {
      if (typeof t === "string" && isRowToken(t)) line.push({ token: t });
      else if (typeof t === "object" && t !== null) {
        const o = t as Record<string, unknown>;
        if (typeof o["token"] !== "string" || !isRowToken(o["token"])) return null;
        line.push({ token: o["token"], bold: o["bold"] === true, dim: o["dim"] === true });
      } else return null;
    }
    lines.push(line);
  }
  return lines;
}

function parseSidebarRows(raw: unknown): SidebarRows {
  const r = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  const pick = (type: RowType) => parseRowLines(r[type]) ?? DEFAULT_SIDEBAR_ROWS[type];
  return { workspace: pick("workspace"), agent: pick("agent"), terminal: pick("terminal") };
}

function parseRoots(raw: unknown): string[] {
  const roots = Array.isArray(raw)
    ? raw.filter((p): p is string => typeof p === "string" && p !== "")
    : [];
  return roots.length ? roots : DEFAULT_SETTINGS.explorerRoots;
}

function parseColorSchema(raw: unknown): ColorSchemeSetting | null {
  const name = (v: unknown) => (typeof v === "string" && v.trim() !== "" ? v.trim() : null);
  if (typeof raw === "object" && raw !== null) {
    const light = name((raw as Record<string, unknown>)["light"]);
    const dark = name((raw as Record<string, unknown>)["dark"]);
    return light && dark ? { light, dark } : null;
  }
  return name(raw);
}

/** Accepts a payload from the backend; anything malformed falls back to the defaults. */
export function parseSettings(raw: unknown): Settings {
  if (typeof raw !== "object" || raw === null) return DEFAULT_SETTINGS;
  const r = raw as Record<string, unknown>;
  const colors = r["colors"] as Record<string, unknown> | null | undefined;
  const color = (v: unknown) => (typeof v === "string" ? v : null);
  return {
    theme: r["theme"] === "light" || r["theme"] === "dark" ? r["theme"] : "system",
    colorSchema: parseColorSchema(r["colorSchema"]),
    fontFamily: typeof r["fontFamily"] === "string" ? r["fontFamily"] : null,
    fontSize:
      typeof r["fontSize"] === "number" && Number.isFinite(r["fontSize"])
        ? r["fontSize"]
        : DEFAULT_SETTINGS.fontSize,
    colors:
      colors && typeof colors === "object"
        ? { background: color(colors["background"]), foreground: color(colors["foreground"]) }
        : null,
    autoControl: r["autoControl"] !== false,
    shortcuts: parseShortcuts(r["shortcuts"]),
    sidebarRows: parseSidebarRows(r["sidebarRows"]),
    explorerRoots: parseRoots(r["explorerRoots"]),
    explorerShowHidden: r["explorerShowHidden"] === true,
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

  /** Writes the `theme` entry of config.json and applies it at once; the file watcher confirms. */
  async setTheme(theme: ThemeSetting): Promise<void> {
    const previous = this._settings();
    this._settings.set({ ...previous, theme });
    try {
      await this.bridge.invoke("set_theme", { theme });
    } catch (e) {
      this._settings.set(previous);
      console.warn(`poimen settings: cannot save theme: ${e}`);
    }
  }

  /**
   * Replaces root `index` of `explorer.roots` in config.json with `path` (`..` moves it up) and
   * applies the new roots at once; the file watcher confirms.
   */
  async setExplorerRoot(index: number, path: string): Promise<void> {
    try {
      const roots = await this.bridge.invoke<string[]>("set_explorer_root", { index, path });
      this._settings.update((s) => ({ ...s, explorerRoots: roots }));
    } catch (e) {
      console.warn(`poimen settings: cannot save the explorer root: ${e}`);
    }
  }

  private apply(raw: unknown): void {
    const next = parseSettings(raw);
    for (const w of next.warnings) console.warn(`poimen settings: ${w}`);
    this._settings.set(next);
  }
}
