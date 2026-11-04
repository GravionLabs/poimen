export const SHORTCUT_ACTIONS = [
  "newTerminal",
  "closeTab",
  "closeAll",
  "refresh",
  "toggleSidebar",
  "toggleOutput",
  "splitRight",
  "splitDown",
  "zoomIn",
  "zoomOut",
  "zoomReset",
  "commandPalette",
  "nextWaiting",
  "nextTab",
  "previousTab",
  "showShortcuts",
] as const;

export type ShortcutAction = (typeof SHORTCUT_ACTIONS)[number];
export type Shortcuts = Record<ShortcutAction, string>;

/** VS Code style defaults; `settings.rs` holds the same table and validates the overrides. */
export const DEFAULT_SHORTCUTS: Shortcuts = {
  newTerminal: "CmdOrCtrl+T",
  closeTab: "CmdOrCtrl+W",
  closeAll: "CmdOrCtrl+Shift+W",
  refresh: "CmdOrCtrl+R",
  toggleSidebar: "CmdOrCtrl+B",
  toggleOutput: "CmdOrCtrl+J",
  splitRight: "CmdOrCtrl+\\",
  splitDown: "CmdOrCtrl+Shift+\\",
  zoomIn: "CmdOrCtrl+=",
  zoomOut: "CmdOrCtrl+-",
  zoomReset: "CmdOrCtrl+0",
  commandPalette: "CmdOrCtrl+Shift+P",
  nextWaiting: "CmdOrCtrl+Shift+A",
  nextTab: "CmdOrCtrl+Alt+Right",
  previousTab: "CmdOrCtrl+Alt+Left",
  showShortcuts: "CmdOrCtrl+/",
};

/** Accepts the resolved `shortcuts` of the settings payload; anything else keeps the default. */
export function parseShortcuts(raw: unknown): Shortcuts {
  const out = { ...DEFAULT_SHORTCUTS };
  if (typeof raw !== "object" || raw === null) return out;
  const r = raw as Record<string, unknown>;
  for (const action of SHORTCUT_ACTIONS) {
    if (typeof r[action] === "string") out[action] = r[action];
  }
  return out;
}

export interface Chord {
  meta: boolean;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  /** Lowercase `KeyboardEvent.key`. */
  key: string;
}

const NAMED_KEYS: Record<string, string> = {
  left: "arrowleft",
  right: "arrowright",
  up: "arrowup",
  down: "arrowdown",
  plus: "+",
  space: " ",
  esc: "escape",
};

/** `event.code` of punctuation keys, for layouts where Shift or Alt changes `event.key`. */
const CODES: Record<string, string> = {
  "\\": "Backslash",
  "=": "Equal",
  "-": "Minus",
  "[": "BracketLeft",
  "]": "BracketRight",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  ";": "Semicolon",
  "'": "Quote",
  "`": "Backquote",
};

/** Parses `CmdOrCtrl+Shift+W` style text; `null` for an empty (unbound) or unreadable one. */
export function parseChord(accelerator: string, mac: boolean): Chord | null {
  const parts = accelerator.split("+").map((p) => p.trim());
  const key = parts.pop();
  if (!key) return null;
  const chord: Chord = { meta: false, ctrl: false, alt: false, shift: false, key: "" };
  for (const part of parts) {
    switch (part.toLowerCase()) {
      case "cmdorctrl":
        if (mac) chord.meta = true;
        else chord.ctrl = true;
        break;
      case "cmd":
      case "meta":
      case "super":
        chord.meta = true;
        break;
      case "ctrl":
        chord.ctrl = true;
        break;
      case "alt":
        chord.alt = true;
        break;
      case "shift":
        chord.shift = true;
        break;
      default:
        return null;
    }
  }
  const lower = key.toLowerCase();
  chord.key = NAMED_KEYS[lower] ?? lower;
  return chord;
}

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"> &
  Partial<Pick<KeyboardEvent, "code">>;

function matches(event: KeyLike, chord: Chord): boolean {
  if (
    event.metaKey !== chord.meta ||
    event.ctrlKey !== chord.ctrl ||
    event.altKey !== chord.alt ||
    event.shiftKey !== chord.shift
  ) {
    return false;
  }
  if (event.key.toLowerCase() === chord.key) return true;
  const code = CODES[chord.key];
  return code !== undefined && event.code === code;
}

/** Shortcut text for a menu or tooltip: `⌘⇧W` on macOS, `Ctrl+Shift+W` elsewhere. */
export function formatShortcut(accelerator: string, mac: boolean): string {
  const chord = parseChord(accelerator, mac);
  if (!chord) return "";
  const key = chord.key.startsWith("arrow") ? chord.key.slice(5) : chord.key;
  const label = key.length === 1 ? key.toUpperCase() : key[0].toUpperCase() + key.slice(1);
  if (mac) {
    return `${chord.ctrl ? "⌃" : ""}${chord.alt ? "⌥" : ""}${chord.shift ? "⇧" : ""}${chord.meta ? "⌘" : ""}${label}`;
  }
  const mods = [
    chord.meta && "Super",
    chord.ctrl && "Ctrl",
    chord.alt && "Alt",
    chord.shift && "Shift",
  ].filter(Boolean);
  return [...mods, label].join("+");
}

export interface MatchContext {
  mac: boolean;
  /** The key event happened inside a terminal that has focus. */
  inTerminal: boolean;
}

/**
 * The action a key event triggers, or `null`. Two rules keep terminals usable: a focused terminal
 * only yields shortcuts that use Cmd (everything else belongs to the program in the pane), and on
 * macOS Cmd shortcuts are the native menu's (its accelerators), so the webview skips them.
 */
export function matchShortcut(
  event: KeyLike,
  shortcuts: Shortcuts,
  context: MatchContext,
): ShortcutAction | null {
  if (context.mac && event.metaKey) return null;
  for (const action of SHORTCUT_ACTIONS) {
    const chord = parseChord(shortcuts[action], context.mac);
    if (!chord || !matches(event, chord)) continue;
    if (context.inTerminal && !chord.meta) return null;
    return action;
  }
  return null;
}

export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);
}
