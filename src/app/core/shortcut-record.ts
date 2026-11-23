import { SHORTCUT_ACTIONS, type ShortcutAction, type Shortcuts } from "./shortcuts";

type RecordedKey = Pick<
  KeyboardEvent,
  "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"
>;

/** Punctuation by `KeyboardEvent.code`, as the accelerator spells it. */
const PUNCTUATION: Record<string, string> = {
  Backslash: "\\",
  Equal: "=",
  Minus: "-",
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
};

const NAMED: Record<string, string> = {
  ArrowLeft: "Left",
  ArrowRight: "Right",
  ArrowUp: "Up",
  ArrowDown: "Down",
  " ": "Space",
  "+": "Plus",
  Tab: "Tab",
  Enter: "Enter",
  Backspace: "Backspace",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
};

const MODIFIER_KEYS = ["Meta", "Control", "Alt", "Shift", "AltGraph", "OS"];

export type Recorded = { accelerator: string } | { error: string } | null;

/**
 * The accelerator a key event spells (`CmdOrCtrl+Shift+N`), `null` while only modifiers are held,
 * or an error when the combination is not allowed. The rule is the settings loader's: Cmd, Ctrl
 * or Alt is needed, a function key stands alone. The primary modifier is `CmdOrCtrl` (Cmd on
 * macOS, Ctrl elsewhere), so a recorded shortcut keeps working across systems.
 */
export function recordAccelerator(event: RecordedKey, mac: boolean): Recorded {
  if (MODIFIER_KEYS.includes(event.key)) return null;
  const letter = /^Key([A-Z])$/.exec(event.code ?? "")?.[1];
  const digit = /^Digit([0-9])$/.exec(event.code ?? "")?.[1];
  const fn = /^F([1-9]|1[0-2])$/.test(event.key) ? event.key : undefined;
  const key =
    letter ??
    digit ??
    PUNCTUATION[event.code ?? ""] ??
    fn ??
    NAMED[event.key] ??
    (event.key.length === 1 ? event.key.toUpperCase() : undefined);
  if (key === undefined) return { error: `${event.key} cannot be a shortcut` };
  const primary = mac ? event.metaKey : event.ctrlKey;
  // The other of Cmd/Ctrl is spelled literally; Super on Linux and Windows is `Cmd`.
  const other = mac ? event.ctrlKey : event.metaKey;
  if (!(primary || other || event.altKey || fn)) {
    return {
      error: "A shortcut needs Cmd, Ctrl or Alt (a function key stands alone)",
    };
  }
  const parts = [
    primary && "CmdOrCtrl",
    other && (mac ? "Ctrl" : "Cmd"),
    event.altKey && "Alt",
    event.shiftKey && "Shift",
    key,
  ];
  return {
    accelerator: parts.filter((p): p is string => p !== false).join("+"),
  };
}

/** The other action that already has `accelerator`, or `null`; an unbound shortcut never clashes. */
export function shortcutConflict(
  shortcuts: Shortcuts,
  action: ShortcutAction,
  accelerator: string,
): ShortcutAction | null {
  if (accelerator === "") return null;
  const lower = accelerator.toLowerCase();
  return SHORTCUT_ACTIONS.find((a) => a !== action && shortcuts[a].toLowerCase() === lower) ?? null;
}
