import { Unicode11Addon } from "@xterm/addon-unicode11";

/** The part of an xterm.js terminal (browser or headless) the widths are set on. */
export interface UnicodeTerminal {
  loadAddon(addon: Unicode11Addon): void;
  readonly unicode: { activeVersion: string };
}

/**
 * Makes xterm.js measure characters like the server grid and current programs do. Its default
 * is Unicode 6, where emoji and many symbols are one cell wide; Claude Code's classic renderer
 * counts them as two and moves the cursor up by the rows it computed, so a different wrap makes
 * every frame land on the previous one (#502). Per-codepoint widths like `alacritty_terminal`,
 * so not the graphemes addon.
 */
export function useUnicode11(term: UnicodeTerminal): void {
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";
}
