import { Injectable } from "@angular/core";

/** Colors of the search highlights; the addon accepts `#RRGGBB` only. */
export interface SearchDecorations {
  matchBackground: string;
  matchBorder: string;
  matchOverviewRuler: string;
  activeMatchBackground: string;
  activeMatchBorder: string;
  activeMatchColorOverviewRuler: string;
}

const HEX6 = /^#[0-9a-f]{6}$/i;

/** `#RGB` and `#RRGGBB` as `#RRGGBB`; anything else (named, rgb(), alpha) is the fallback. */
export function hex6(value: string, fallback: string): string {
  const v = value.trim();
  if (HEX6.test(v)) return v;
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(v);
  return short ? `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}` : fallback;
}

/** Highlight colors from the `--term-*` tokens (`read` is `cssVar`), so they follow the theme. */
export function searchDecorations(
  read: (name: string) => string,
  overrides: { selection?: string } = {},
): SearchDecorations {
  const selection = hex6(overrides.selection ?? read("--term-selection-bg"), "#555555");
  const active = hex6(read("--term-ansi-3"), "#e5c07b");
  const fg = hex6(read("--term-fg"), "#ffffff");
  return {
    matchBackground: selection,
    matchBorder: selection,
    matchOverviewRuler: selection,
    activeMatchBackground: active,
    activeMatchBorder: fg,
    activeMatchColorOverviewRuler: active,
  };
}

/** Text for the match counter: `3 of 12`, `No results`, or empty without a query. */
export function matchLabel(query: string, index: number, count: number): string {
  if (!query) return "";
  if (count === 0) return "No results";
  return `${index + 1} of ${count}`;
}

/** What a terminal view offers the app to open its search bar. */
export interface Searchable {
  openSearch(): void;
}

/**
 * The terminal view that last had the focus. A view exists only while its tab is visible and
 * unregisters when it is destroyed, so with the Settings view in front nothing is found.
 */
@Injectable({ providedIn: "root" })
export class TerminalSearch {
  private current: Searchable | null = null;

  focused(view: Searchable): void {
    this.current = view;
  }

  gone(view: Searchable): void {
    if (this.current === view) this.current = null;
  }

  /** Opens the bar of the last focused terminal view; false when there is none. */
  open(): boolean {
    if (!this.current) return false;
    this.current.openSearch();
    return true;
  }
}
