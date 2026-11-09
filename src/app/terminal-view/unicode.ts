import { Unicode11Addon } from "@xterm/addon-unicode11";
import type { Terminal } from "@xterm/xterm";

/**
 * Makes xterm.js measure characters like the server grid and current programs do. Its default
 * is Unicode 6, where emoji and many symbols are one cell wide; Claude Code's classic renderer
 * counts them as two and moves the cursor up by the rows it computed, so a different wrap makes
 * every frame land on the previous one (#502).
 */
export function useUnicode11(term: Terminal): void {
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";
}
