import { Injectable, inject, signal } from "@angular/core";
import { DEFAULT_SHORTCUTS, parseShortcuts, type Shortcuts } from "./shortcuts";
import { parseSpaceLayout, type SpaceLayout } from "./space-layout";
import { parseSpaceTemplates, templateConfig, type SpaceTemplate } from "./space-templates";
import { TauriBridge } from "./tauri";
import {
  MAX_TERMINAL_FONT_SIZE,
  MAX_UI_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  MIN_UI_FONT_SIZE,
  sizeError,
} from "./ui-font";
import {
  DEFAULT_SIDEBAR_ROWS,
  isRowToken,
  type RowLines,
  type RowType,
  type SidebarRows,
} from "../sidebar/spaces";

export type SidebarLayout = "tree" | "split";

export type ThemeSetting = "light" | "dark" | "system";

/** A scheme name, or a pair of names that `theme` picks from. */
export type ColorSchemeSetting = string | { light: string; dark: string };

/** Terminal colors of the Ghostty config; `palette` is empty or holds the 16 ANSI colors. */
export interface GhosttyColors {
  background: string | null;
  foreground: string | null;
  cursor: string | null;
  selectionBackground: string | null;
  selectionForeground: string | null;
  palette: (string | null)[];
}

/** A task mode (`tasks.modes[]`, wire shape of `tasks.rs`). */
export interface TaskMode {
  name: string;
  worktree: boolean;
  /** Agent mode: the harness (`null`: the first of the registry), model and prompt template. */
  harness: string | null;
  model: string | null;
  prompt: string | null;
  /** An argv list, or a deprecated shell string; `null` for an agent mode. */
  command: string[] | string | null;
  branch: string;
  cleanup: string[] | string | null;
}

/** A remote machine (`machines[]`, wire shape of `machines.rs`). */
export interface Machine {
  name: string;
  /** The ssh target as in `~/.ssh/config`. */
  ssh: string;
  /** Where repositories live on the machine (`{owner}`, `{repo}`). */
  reposDir: string;
}

/** A machine of the wire, or `null` when `name` or `ssh` is missing. */
export function parseMachine(raw: unknown): Machine | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const name = r["name"];
  const ssh = r["ssh"];
  if (typeof name !== "string" || name === "" || typeof ssh !== "string" || ssh === "") return null;
  const reposDir = typeof r["reposDir"] === "string" ? r["reposDir"] : "~/Source/{owner}/{repo}";
  return { name, ssh, reposDir };
}

export interface TaskModes {
  modes: TaskMode[];
  worktreeDir: string;
}

export const DEFAULT_TASKS: TaskModes = {
  modes: [
    {
      name: "agent",
      worktree: true,
      harness: null,
      model: null,
      prompt: null,
      command: null,
      branch: "{type}/{nr}-{slug}",
      cleanup: null,
    },
  ],
  worktreeDir: "../{repo}-{nr}",
};

/** An agent harness with its availability (`get_harnesses`, wire shape of `harness.rs`). */
export interface HarnessInfo {
  id: string;
  command: string[];
  models: string[];
  defaultModel: string | null;
  /** The program was found on the `PATH` of a child. */
  available: boolean;
}

/** The harnesses of a `get_harnesses` answer; malformed entries are skipped. */
export function parseHarnesses(raw: unknown): HarnessInfo[] {
  if (!Array.isArray(raw)) return [];
  const strings = (v: unknown): string[] =>
    Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  return raw.flatMap((item): HarnessInfo[] => {
    if (typeof item !== "object" || item === null) return [];
    const r = item as Record<string, unknown>;
    if (typeof r["id"] !== "string") return [];
    return [
      {
        id: r["id"],
        command: strings(r["command"]),
        models: strings(r["models"]),
        defaultModel: typeof r["defaultModel"] === "string" ? r["defaultModel"] : null,
        available: r["available"] === true,
      },
    ];
  });
}

/** When an agent action is offered (`When` of `agent_actions.rs`); every given condition must hold. */
export interface AgentActionWhen {
  level: string[];
  label: string[];
  noChildren: boolean;
  checks: string | null;
  state: string[];
  review: string | null;
}

/** A resolved agent action (`get_agent_actions`, wire shape of `agent_actions.rs`). */
export interface AgentAction {
  id: string;
  label: string;
  on: "issue" | "pr" | "milestone";
  prompt: string;
  run: string;
  harness: string | null;
  model: string | null;
  when: AgentActionWhen;
  batch: string;
  /** Where the entry comes from. */
  source: "default" | "user" | "project";
}

function parseWhen(raw: unknown): AgentActionWhen {
  const r = (typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>;
  const list = (v: unknown): string[] =>
    Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  return {
    level: list(r["level"]),
    label: list(r["label"]),
    noChildren: r["noChildren"] === true,
    checks: typeof r["checks"] === "string" ? r["checks"] : null,
    state: list(r["state"]),
    review: typeof r["review"] === "string" ? r["review"] : null,
  };
}

/** The actions of a `get_agent_actions` answer; malformed entries are skipped. */
export function parseAgentActions(raw: unknown): AgentAction[] {
  const list = (raw as { actions?: unknown } | null)?.actions;
  if (!Array.isArray(list)) return [];
  return list.flatMap((item): AgentAction[] => {
    if (typeof item !== "object" || item === null) return [];
    const r = item as Record<string, unknown>;
    const on = r["on"];
    const source = r["source"];
    if (typeof r["id"] !== "string" || typeof r["prompt"] !== "string") return [];
    if (on !== "issue" && on !== "pr" && on !== "milestone") return [];
    if (source !== "default" && source !== "user" && source !== "project") return [];
    return [
      {
        id: r["id"],
        label: typeof r["label"] === "string" ? r["label"] : r["id"],
        on,
        prompt: r["prompt"],
        run: typeof r["run"] === "string" ? r["run"] : "repoRoot",
        harness: typeof r["harness"] === "string" ? r["harness"] : null,
        model: typeof r["model"] === "string" ? r["model"] : null,
        when: parseWhen(r["when"]),
        batch: typeof r["batch"] === "string" ? r["batch"] : "each",
        source,
      },
    ];
  });
}

function commandSpec(v: unknown): string[] | string | null {
  if (typeof v === "string") return v;
  if (Array.isArray(v) && v.every((x) => typeof x === "string")) return v as string[];
  return null;
}

/** The resolved task modes; the default when the shape is wrong. */
export function parseTaskModes(raw: unknown): TaskModes {
  if (typeof raw !== "object" || raw === null) return DEFAULT_TASKS;
  const r = raw as Record<string, unknown>;
  if (!Array.isArray(r["modes"]) || typeof r["worktreeDir"] !== "string") return DEFAULT_TASKS;
  const modes: TaskMode[] = [];
  for (const m of r["modes"]) {
    if (typeof m !== "object" || m === null) continue;
    const o = m as Record<string, unknown>;
    if (typeof o["name"] !== "string") continue;
    modes.push({
      name: o["name"],
      worktree: o["worktree"] !== false,
      harness: typeof o["harness"] === "string" ? o["harness"] : null,
      model: typeof o["model"] === "string" ? o["model"] : null,
      prompt: typeof o["prompt"] === "string" ? o["prompt"] : null,
      command: commandSpec(o["command"]),
      branch: typeof o["branch"] === "string" ? o["branch"] : DEFAULT_TASKS.modes[0].branch,
      cleanup: commandSpec(o["cleanup"]),
    });
  }
  return modes.length > 0 ? { modes, worktreeDir: r["worktreeDir"] } : DEFAULT_TASKS;
}

/** The resolved user settings from `~/.config/poimen/config.json` (wire shape of `settings.rs`). */
export interface Settings {
  theme: ThemeSetting;
  /** The `colorSchema`: a name or a light/dark pair; `null` is the `poimen` scheme that follows `theme`. */
  colorSchema: ColorSchemeSetting | null;
  /** `null` keeps the app's default monospace stack. */
  fontFamily: string | null;
  fontSize: number;
  /** App UI font family (`ui.fontFamily`); `null` keeps the default `--font-ui`. */
  uiFontFamily: string | null;
  /** App UI font size in px (`ui.fontSize`). */
  uiFontSize: number;
  /** Terminal colors taken from Ghostty; `null` keeps the theme colors. */
  colors: GhosttyColors | null;
  /** Open panes for input when no other client controls them (`terminal.autoControl`). */
  autoControl: boolean;
  /** Accelerator per action: the defaults over the valid `shortcuts` of config.json. */
  shortcuts: Shortcuts;
  /** Lines and tokens per row type: the defaults over the valid `sidebar.rows` of config.json. */
  sidebarRows: SidebarRows;
  /** `sidebar.layout`: the Spaces view as a tree or as two lists (Spaces, Agents). */
  sidebarLayout: SidebarLayout;
  /** Roots of the explorer; a leading `~` is the home directory (resolved by the backend). */
  explorerRoots: string[];
  /** Whether the explorer shows hidden directories. */
  explorerShowHidden: boolean;
  /** The terminal backend (`backend`); a change needs a restart. */
  backend: BackendKind;
  /** Lines of scrollback xterm.js keeps on an emulated stream (`terminal.scrollback`). */
  scrollback: number;
  /** Start the backend's server when it is not running (`herdr.autoStart`). */
  herdrAutoStart: boolean;
  /** `spaces.layout`: the pane layout of a new space; `null` uses the built-in default. */
  spacesLayout: SpaceLayout | null;
  /** `spaceTemplates`: directory or remote matched templates with agent and layout. */
  spaceTemplates: SpaceTemplate[];
  /** `trustedProjects`: repository root to the hash of the project file the user trusted. */
  trustedProjects: Record<string, string>;
  /** `tasks`: the task modes and the worktree directory template. */
  tasks: TaskModes;
  /** `machines`: remote machines reached over SSH; invalid entries are settings warnings. */
  machines: Machine[];
  /** The home directory `~` stands for; `null` until the backend knows it. */
  home: string | null;
  warnings: string[];
}

/** The theme a click on the toolbar button selects: system, then light, then dark. */
export function nextTheme(theme: ThemeSetting): ThemeSetting {
  return theme === "system" ? "light" : theme === "light" ? "dark" : "system";
}

/** The terminal backend poimen talks to: poimen-server (default) or herdr. */
export type BackendKind = "herdr" | "poimen";

/** Whether the app runs on Windows (the webview reports `Win32`). */
export function isWindows(): boolean {
  return typeof navigator !== "undefined" && /Win/i.test(navigator.platform);
}

/** Default `terminal.fontSize`: 14 on Windows, 13 elsewhere (mirrors `settings.rs`). */
/**
 * xterm.js option for a PTY that is ConPTY (Windows): it tells xterm.js the output was already
 * reflowed by ConPTY. `buildNumber` is the first build with ConPTY's reflow handling; undefined
 * elsewhere, where the option must stay unset.
 */
export function windowsPtyOption(
  windows: boolean = isWindows(),
): { backend: "conpty"; buildNumber: number } | undefined {
  return windows ? { backend: "conpty", buildNumber: 21376 } : undefined;
}

export function defaultTerminalFontSize(windows: boolean = isWindows()): number {
  return windows ? 14 : 13;
}

const WINDOWS_MONO = '"Cascadia Mono", "Cascadia Code", Consolas';

/**
 * The terminal font stack when `terminal.fontFamily` is not set: on Windows the system
 * monospace fonts come first, then `--font-mono` (`mono`) or `Menlo, monospace`.
 */
export function defaultTerminalFontFamily(mono: string, windows: boolean = isWindows()): string {
  const base = mono || "Menlo, monospace";
  return windows ? `${WINDOWS_MONO}, ${base}` : base;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  colorSchema: null,
  fontFamily: null,
  fontSize: defaultTerminalFontSize(),
  uiFontFamily: null,
  uiFontSize: 13,
  colors: null,
  autoControl: true,
  shortcuts: DEFAULT_SHORTCUTS,
  sidebarRows: DEFAULT_SIDEBAR_ROWS,
  sidebarLayout: "tree",
  explorerRoots: ["~"],
  explorerShowHidden: false,
  backend: "poimen",
  scrollback: 10000,
  herdrAutoStart: true,
  spacesLayout: null,
  spaceTemplates: [],
  trustedProjects: {},
  tasks: DEFAULT_TASKS,
  machines: [],
  home: null,
  warnings: [],
};

/** The `trustedProjects` map; entries that are not strings are skipped. */
function parseTrusted(raw: unknown): Record<string, string> {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return {};
  return Object.fromEntries(
    Object.entries(raw).filter((e): e is [string, string] => typeof e[1] === "string"),
  );
}

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
    uiFontFamily: typeof r["uiFontFamily"] === "string" ? r["uiFontFamily"] : null,
    uiFontSize:
      typeof r["uiFontSize"] === "number" && Number.isFinite(r["uiFontSize"])
        ? r["uiFontSize"]
        : DEFAULT_SETTINGS.uiFontSize,
    colors:
      colors && typeof colors === "object"
        ? {
            background: color(colors["background"]),
            foreground: color(colors["foreground"]),
            cursor: color(colors["cursor"]),
            selectionBackground: color(colors["selectionBackground"]),
            selectionForeground: color(colors["selectionForeground"]),
            palette: Array.isArray(colors["palette"])
              ? colors["palette"].slice(0, 16).map(color)
              : [],
          }
        : null,
    autoControl: r["autoControl"] !== false,
    shortcuts: parseShortcuts(r["shortcuts"]),
    sidebarRows: parseSidebarRows(r["sidebarRows"]),
    sidebarLayout: r["sidebarLayout"] === "split" ? "split" : "tree",
    explorerRoots: parseRoots(r["explorerRoots"]),
    explorerShowHidden: r["explorerShowHidden"] === true,
    backend: r["backend"] === "herdr" ? "herdr" : "poimen",
    scrollback:
      typeof r["scrollback"] === "number" &&
      Number.isInteger(r["scrollback"]) &&
      r["scrollback"] >= 0
        ? r["scrollback"]
        : DEFAULT_SETTINGS.scrollback,
    herdrAutoStart: r["herdrAutoStart"] !== false,
    spacesLayout: parseSpaceLayout(r["spacesLayout"]),
    spaceTemplates: parseSpaceTemplates(r["spaceTemplates"]),
    trustedProjects: parseTrusted(r["trustedProjects"]),
    tasks: parseTaskModes(r["tasks"]),
    machines: Array.isArray(r["machines"])
      ? r["machines"].map(parseMachine).filter((m) => m !== null)
      : [],
    home: typeof r["home"] === "string" ? r["home"] : null,
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
  private readonly _writeError = signal<string | null>(null);
  /** Why the last write of a setting from the UI failed; cleared by the next successful write. */
  readonly writeError = this._writeError.asReadonly();

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

  /** Writes the `colorSchema` entry of config.json and applies it at once; the watcher confirms. */
  async setColorSchema(colorSchema: ColorSchemeSetting): Promise<void> {
    const previous = this._settings();
    this._settings.set({ ...previous, colorSchema });
    try {
      await this.bridge.invoke("set_color_schema", { schema: colorSchema });
    } catch (e) {
      this._settings.set(previous);
      console.warn(`poimen settings: cannot save colorSchema: ${e}`);
    }
  }

  /** Writes `sidebar.layout` of config.json and applies it at once; the watcher confirms. */
  async setSidebarLayout(sidebarLayout: SidebarLayout): Promise<void> {
    const previous = this._settings();
    this._settings.set({ ...previous, sidebarLayout });
    try {
      await this.bridge.invoke("set_sidebar_layout", { layout: sidebarLayout });
      this._writeError.set(null);
    } catch (e) {
      this._settings.set(previous);
      this._writeError.set(`Cannot save the sidebar layout: ${e}`);
      console.warn(`poimen settings: cannot save sidebar.layout: ${e}`);
    }
  }

  /** Writes `explorer.roots` of config.json (home paths as `~/…`) and applies it at once. */
  async setExplorerRoots(explorerRoots: string[]): Promise<void> {
    await this.write({ explorerRoots }, "the explorer folders", "set_explorer_roots", {
      roots: explorerRoots,
    });
  }

  /** Writes `explorer.showHidden` of config.json and applies it at once. */
  async setExplorerShowHidden(explorerShowHidden: boolean): Promise<void> {
    await this.write(
      { explorerShowHidden },
      "the hidden folders option",
      "set_explorer_show_hidden",
      { show: explorerShowHidden },
    );
  }

  /** Writes `terminal.fontFamily` (empty removes it: the default stack) and applies it at once. */
  async setTerminalFontFamily(family: string): Promise<boolean> {
    const trimmed = family.trim();
    return this.write(
      { fontFamily: trimmed || null },
      "the terminal font",
      "set_terminal_font_family",
      { family: trimmed },
    );
  }

  /** Writes `terminal.fontSize`; an out-of-range size is not written and sets the write error. */
  async setTerminalFontSize(size: number): Promise<boolean> {
    const bad = sizeError(
      "The terminal font size",
      size,
      MIN_TERMINAL_FONT_SIZE,
      MAX_TERMINAL_FONT_SIZE,
    );
    if (bad) return this.reject(bad);
    return this.write({ fontSize: size }, "the terminal font size", "set_terminal_font_size", {
      size,
    });
  }

  /** Removes `terminal.fontSize` (the platform default applies again) and applies it at once. */
  async clearTerminalFontSize(): Promise<boolean> {
    return this.write(
      { fontSize: defaultTerminalFontSize() },
      "the terminal font size",
      "clear_terminal_font_size",
      {},
    );
  }

  /** Writes `ui.fontFamily` (empty removes it: the default `--font-ui`) and applies it at once. */
  async setUiFontFamily(family: string): Promise<boolean> {
    const trimmed = family.trim();
    return this.write({ uiFontFamily: trimmed || null }, "the UI font", "set_ui_font_family", {
      family: trimmed,
    });
  }

  /** Writes `ui.fontSize`; an out-of-range size is not written and sets the write error. */
  async setUiFontSize(size: number): Promise<boolean> {
    const bad = sizeError("The UI font size", size, MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE);
    if (bad) return this.reject(bad);
    return this.write({ uiFontSize: size }, "the UI font size", "set_ui_font_size", { size });
  }

  private reject(message: string): false {
    this._writeError.set(message);
    return false;
  }

  /** Writes `herdr.autoStart` of config.json and applies it at once. */
  async setHerdrAutoStart(herdrAutoStart: boolean): Promise<void> {
    await this.write({ herdrAutoStart }, "the herdr auto start option", "set_herdr_auto_start", {
      on: herdrAutoStart,
    });
  }

  /** Writes `spaces.layout` (validated again by the backend); `false` when the write failed. */
  setSpacesLayout(layout: SpaceLayout, config: Record<string, unknown>): Promise<boolean> {
    return this.write({ spacesLayout: layout }, "the new space layout", "set_spaces_layout", {
      layout: config,
    });
  }

  /** The default layout of a new space (seeded for the installed agents); `null` when the backend cannot be asked. */
  async defaultSpacesLayout(): Promise<SpaceLayout | null> {
    try {
      return parseSpaceLayout(await this.bridge.invoke("default_spaces_layout"));
    } catch {
      return null;
    }
  }

  /** Writes `spaceTemplates` (validated again by the backend); `false` when the write failed. */
  setSpaceTemplates(spaceTemplates: SpaceTemplate[]): Promise<boolean> {
    return this.write({ spaceTemplates }, "the space templates", "set_space_templates", {
      templates: spaceTemplates.map(templateConfig),
    });
  }

  /** Drops the trust of the project file of `root` (`trustedProjects`) and applies it at once. */
  async revokeProject(root: string): Promise<void> {
    const rest = Object.fromEntries(
      Object.entries(this._settings().trustedProjects).filter(([r]) => r !== root),
    );
    await this.write({ trustedProjects: rest }, "the revoked project", "revoke_project", { root });
  }

  /** Optimistic write: applies `patch`, calls `command`, goes back and reports a failure. */
  private async write(
    patch: Partial<Settings>,
    what: string,
    command: string,
    args: Record<string, unknown>,
  ): Promise<boolean> {
    const previous = this._settings();
    this._settings.set({ ...previous, ...patch });
    try {
      await this.bridge.invoke(command, args);
      this._writeError.set(null);
      return true;
    } catch (e) {
      this._settings.set(previous);
      this._writeError.set(`Cannot save ${what}: ${e}`);
      console.warn(`poimen settings: cannot save ${what}: ${e}`);
      return false;
    }
  }

  private apply(raw: unknown): void {
    const next = parseSettings(raw);
    for (const w of next.warnings) console.warn(`poimen settings: ${w}`);
    this._settings.set(next);
  }
}
