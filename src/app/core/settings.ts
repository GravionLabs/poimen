import { Injectable, inject, signal } from "@angular/core";
import { injectMessages, TOAST_ERROR_MS } from "./toast";
import { DEFAULT_SHORTCUTS, parseShortcuts, type Shortcuts } from "./shortcuts";
import { parseSpaceLayout, type SpaceLayout } from "./space-layout";
import { layoutConfig, parseNamedLayouts, type NamedLayout } from "./named-layouts";
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

/** `sidebar.sort`: the user's own space order, alphabetical, by attention or by last focus. */
export type SidebarSort = "manual" | "name" | "attention" | "recent";

const SIDEBAR_SORTS: readonly SidebarSort[] = ["manual", "name", "attention", "recent"];

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
  /** Text typed at the agent's prompt to end its session. */
  exitInput: string | null;
  /** Argv template appended to the command to resume a session (`{session}`); empty: none. */
  resumeArgs: string[];
  /** Argv template appended to the command for a headless one-shot call (`{prompt}`); empty: none. */
  printArgs: string[];
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
        exitInput: typeof r["exitInput"] === "string" ? r["exitInput"] : null,
        resumeArgs: strings(r["resumeArgs"]),
        printArgs: strings(r["printArgs"]),
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
  conflicting: boolean;
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
    conflicting: r["conflicting"] === true,
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
  /** `sidebar.sort`: how the spaces tree is ordered. */
  sidebarSort: SidebarSort;
  /** `sidebar.groupWorktrees`: list linked worktrees below the space of their repository. */
  sidebarGroupWorktrees: boolean;
  /** `sidebar.showBaseBranch`: show the branch token on the base branch too. */
  sidebarShowBaseBranch: boolean;
  /** Roots of the explorer; a leading `~` is the home directory (resolved by the backend). */
  explorerRoots: string[];
  /** Whether the explorer shows hidden directories. */
  explorerShowHidden: boolean;
  /** `agents.claude.classicRenderer`: new terminals set `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`. */
  claudeClassicRenderer: boolean;
  /** `agents.keepAwake`: the server keeps the machine awake while an agent works (#987). */
  keepAwake: boolean;
  /** Lines of scrollback xterm.js keeps (`terminal.scrollback`). */
  scrollback: number;
  /** `spaces.layout`: the pane layout of a new space; `null` uses the built-in default. */
  spacesLayout: SpaceLayout | null;
  /** `layouts[]`: named layouts with tabs, nested tabs and split trees (ADR 0011). */
  layouts: NamedLayout[];
  /** `spaceTemplates`: directory or remote matched templates with agent and layout. */
  spaceTemplates: SpaceTemplate[];
  /** `trustedProjects`: repository root to the hash of the project file the user trusted. */
  trustedProjects: Record<string, string>;
  /** `tasks`: the task modes and the worktree directory template. */
  tasks: TaskModes;
  /** `machines`: remote machines reached over SSH; invalid entries are settings warnings. */
  machines: Machine[];
  /** `sounds`: what plays when an agent is blocked or done and not viewed. */
  sounds: SoundSettings;
  /** `usage`: which provider limits the status bar asks for (#784). */
  usage: UsageSettings;
  /** `terminalHistory`: the server logs terminal output to disk (#1196). */
  terminalHistory: TerminalHistorySettings;
  /** The home directory `~` stands for; `null` until the backend knows it. */
  home: string | null;
  warnings: string[];
}

/** `terminalHistory` (defaults: on, 8 and 256 MiB). The server keeps the output on disk. */
export interface TerminalHistorySettings {
  enabled: boolean;
  perTerminalMib: number;
  totalMib: number;
}

export const DEFAULT_TERMINAL_HISTORY: TerminalHistorySettings = {
  enabled: true,
  perTerminalMib: 8,
  totalMib: 256,
};

function mib(v: unknown, fallback: number): number {
  return typeof v === "number" && Number.isInteger(v) && v >= 1 ? v : fallback;
}

function parseTerminalHistory(v: unknown): TerminalHistorySettings {
  const r = v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  return {
    enabled: r["enabled"] !== false,
    perTerminalMib: mib(r["perTerminalMib"], DEFAULT_TERMINAL_HISTORY.perTerminalMib),
    totalMib: mib(r["totalMib"], DEFAULT_TERMINAL_HISTORY.totalMib),
  };
}

/** `usage.claude` (default true): off reads no token and sends no request. */
export interface UsageSettings {
  claude: boolean;
}

function parseUsage(v: unknown): UsageSettings {
  const r = v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  return { claude: r["claude"] !== false };
}

/** `blocked` or `done`: the agent transitions that can play a sound (#724). */
export type SoundKind = "blocked" | "done";

/** One entry of `sounds`: off, the built-in tone, or the path of an audio file. */
export type SoundSetting = boolean | string;

export interface SoundSettings {
  blocked: SoundSetting;
  done: SoundSetting;
}

function parseSound(v: unknown): SoundSetting {
  return v === true || (typeof v === "string" && v.trim() !== "") ? v : false;
}

function parseSounds(v: unknown): SoundSettings {
  const r = v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  return { blocked: parseSound(r["blocked"]), done: parseSound(r["done"]) };
}

/** The theme a click on the toolbar button selects: system, then light, then dark. */
export function nextTheme(theme: ThemeSetting): ThemeSetting {
  return theme === "system" ? "light" : theme === "light" ? "dark" : "system";
}

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
  sidebarSort: "manual",
  sidebarGroupWorktrees: true,
  sidebarShowBaseBranch: false,
  explorerRoots: ["~"],
  explorerShowHidden: false,
  claudeClassicRenderer: false,
  keepAwake: false,
  scrollback: 10000,
  spacesLayout: null,
  layouts: [],
  spaceTemplates: [],
  trustedProjects: {},
  tasks: DEFAULT_TASKS,
  machines: [],
  sounds: { blocked: false, done: false },
  usage: { claude: true },
  terminalHistory: DEFAULT_TERMINAL_HISTORY,
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
    sidebarSort: SIDEBAR_SORTS.find((v) => v === r["sidebarSort"]) ?? "manual",
    sidebarGroupWorktrees: r["sidebarGroupWorktrees"] !== false,
    sidebarShowBaseBranch: r["sidebarShowBaseBranch"] === true,
    explorerRoots: parseRoots(r["explorerRoots"]),
    explorerShowHidden: r["explorerShowHidden"] === true,
    claudeClassicRenderer: r["claudeClassicRenderer"] === true,
    keepAwake: r["keepAwake"] === true,
    scrollback:
      typeof r["scrollback"] === "number" &&
      Number.isInteger(r["scrollback"]) &&
      r["scrollback"] >= 0
        ? r["scrollback"]
        : DEFAULT_SETTINGS.scrollback,
    spacesLayout: parseSpaceLayout(r["spacesLayout"]),
    layouts: parseNamedLayouts(r["layouts"]),
    spaceTemplates: parseSpaceTemplates(r["spaceTemplates"]),
    trustedProjects: parseTrusted(r["trustedProjects"]),
    tasks: parseTaskModes(r["tasks"]),
    machines: Array.isArray(r["machines"])
      ? r["machines"].map(parseMachine).filter((m) => m !== null)
      : [],
    sounds: parseSounds(r["sounds"]),
    usage: parseUsage(r["usage"]),
    terminalHistory: parseTerminalHistory(r["terminalHistory"]),
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
  private readonly messages = injectMessages();
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
      this.failed("the theme", e);
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
      this.failed("the color scheme", e);
    }
  }

  /** Writes `sidebar.sort` of config.json and applies it at once; the watcher confirms. */
  async setSidebarSort(sidebarSort: SidebarSort): Promise<void> {
    const previous = this._settings();
    this._settings.set({ ...previous, sidebarSort });
    try {
      await this.bridge.invoke("set_sidebar_sort", { sort: sidebarSort });
      this._writeError.set(null);
    } catch (e) {
      this._settings.set(previous);
      this.failed("the sidebar sort", e);
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

  /** Writes `agents.claude.classicRenderer` of config.json; it applies to terminals created later. */
  async setClaudeClassicRenderer(claudeClassicRenderer: boolean): Promise<void> {
    await this.write(
      { claudeClassicRenderer },
      "the classic renderer option",
      "set_claude_classic_renderer",
      { on: claudeClassicRenderer },
    );
  }

  /** Writes `agents.keepAwake` of config.json; the server reads it and holds or releases. */
  async setKeepAwake(keepAwake: boolean): Promise<void> {
    await this.write({ keepAwake }, "the keep awake option", "set_keep_awake", { on: keepAwake });
  }

  /**
   * Writes `terminalHistory` of config.json; the server applies it to new output without a
   * restart (#1196). The limits are whole MiB from 1; anything else is refused and not written.
   */
  async setTerminalHistory(value: TerminalHistorySettings): Promise<boolean> {
    if (
      !Number.isInteger(value.perTerminalMib) ||
      !Number.isInteger(value.totalMib) ||
      value.perTerminalMib < 1 ||
      value.totalMib < 1
    ) {
      this.failed("the terminal history", "the limits must be whole numbers of MiB, at least 1");
      return false;
    }
    return this.write({ terminalHistory: value }, "the terminal history", "set_terminal_history", {
      enabled: value.enabled,
      perTerminalMib: value.perTerminalMib,
      totalMib: value.totalMib,
    });
  }

  /** Writes `sounds.<kind>` (`false`, `true` or an audio file path) and applies it at once. */
  async setSound(kind: SoundKind, value: SoundSetting): Promise<boolean> {
    const sounds = { ...this._settings().sounds, [kind]: value };
    return this.write({ sounds }, "the sound", "set_sound", { kind, value });
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
    this.toastError(message);
    return false;
  }

  /** A write that failed: the inline error of the editors and an error toast with the reason. */
  private failed(what: string, reason: unknown): void {
    const message = `Cannot save ${what}: ${reason}`;
    this._writeError.set(message);
    this.toastError(message);
  }

  private toastError(message: string): void {
    this.messages.add({ severity: "error", summary: message, life: TOAST_ERROR_MS });
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

  /** Writes `layouts` (validated again by the backend); `false` when the write failed. */
  setLayouts(layouts: NamedLayout[]): Promise<boolean> {
    return this.write({ layouts }, "the layouts", "set_layouts", {
      layouts: layouts.map(layoutConfig),
    });
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
      this.failed(what, e);
      return false;
    }
  }

  private apply(raw: unknown): void {
    const next = parseSettings(raw);
    for (const w of next.warnings) console.warn(`poimen settings: ${w}`);
    this._settings.set(next);
  }
}
