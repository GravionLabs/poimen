import { Injectable, inject } from "@angular/core";
import { TauriBridge } from "./tauri";
import type { GhosttyColors } from "./settings";

/** Wire shape of `server.set_colors` (`#rrggbb` strings). */
export interface ServerColors {
  foreground: string;
  background: string;
  cursor: string;
  palette?: string[];
}

const HEX = /^#[0-9a-f]{6}$/i;

/**
 * The colors a terminal shows, as poimen-server needs them to answer color queries while no app
 * is attached: the Ghostty colors of the settings over the scheme tokens (`--term-*`), the same
 * order the terminal view uses. `null` when a base color is not a `#rrggbb` value. An invalid
 * palette entry drops the whole palette (the server keeps its own).
 */
export function serverColors(
  colors: GhosttyColors | null,
  token: (name: string) => string,
): ServerColors | null {
  const foreground = colors?.foreground ?? token("--term-fg");
  const background = colors?.background ?? token("--term-bg");
  const cursor = colors?.cursor ?? (token("--term-cursor") || foreground);
  if (![foreground, background, cursor].every((c) => HEX.test(c))) return null;
  const palette = Array.from(
    { length: 16 },
    (_, i) => colors?.palette[i] || token(`--term-ansi-${i}`),
  );
  return {
    foreground,
    background,
    cursor,
    ...(palette.every((c) => HEX.test(c)) ? { palette } : {}),
  };
}

/**
 * Sends the colors of the active scheme to poimen-server (`set_terminal_colors`) on start and on
 * every change; the same colors are sent only once. Nothing happens for the herdr backend (the
 * command ignores them).
 */
@Injectable({ providedIn: "root" })
export class TerminalColorSync {
  private readonly bridge = inject(TauriBridge);
  private last = "";

  /** Reads the current colors and sends them when they changed. */
  update(colors: GhosttyColors | null): void {
    const style = getComputedStyle(document.documentElement);
    const next = serverColors(colors, (name) => style.getPropertyValue(name).trim());
    if (!next) return;
    const key = JSON.stringify(next);
    if (key === this.last) return;
    this.last = key;
    void this.bridge
      .invoke("set_terminal_colors", { colors: next })
      .catch((error: unknown) => console.warn("poimen: could not send the terminal colors", error));
  }
}
