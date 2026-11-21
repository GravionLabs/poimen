/**
 * How the status bar gives way on a narrow window (#1034). The level only goes up in this order:
 * 1. the Claude limits show one percentage instead of every window,
 * 2. the Runs summary shows the mark and the count only,
 * 3. further low-priority items (Output button, mode, awake) show the icon only.
 * "N waiting" and the connection are never shortened at any level. Everything shortened keeps its
 * full text in the tooltip.
 */
export type DensityLevel = 0 | 1 | 2 | 3;

export const MAX_DENSITY: DensityLevel = 3;

/** What a level shortens. */
export interface Density {
  /** One percentage for the limits. */
  limitsShort: boolean;
  /** Mark and count for the Runs summary. */
  runsShort: boolean;
  /** Icon only for the low-priority items. */
  iconOnly: boolean;
}

export function densityFlags(level: DensityLevel): Density {
  return { limitsShort: level >= 1, runsShort: level >= 2, iconOnly: level >= 3 };
}

/** The widths (px) the bar needed at each level, as far as they were measured. */
export type Needed = readonly (number | undefined)[];

/**
 * The next level for a bar of `available` px that currently shows `level` and needs `needed` px
 * there. Overflow steps down one level; the bar steps back up one level only when the width it
 * measured there fits, so it does not flip between two levels.
 */
export function nextLevel(level: DensityLevel, needed: Needed, available: number): DensityLevel {
  const here = needed[level];
  if (here !== undefined && here > available && level < MAX_DENSITY) {
    return (level + 1) as DensityLevel;
  }
  if (level > 0) {
    const wider = needed[level - 1];
    if (wider !== undefined && wider <= available) return (level - 1) as DensityLevel;
  }
  return level;
}

/**
 * The Runs summary as mark and count (`▶ 7/19`, `▶ 3`) from the text of `barSummary`
 * (`▶ M5 7/19`, `▶ 3 runs`); a text of another form keeps only the mark.
 */
export function shortRuns(text: string): string {
  const [mark = ""] = text.split(" ");
  const count = /(\d+) runs$/.exec(text)?.[1] ?? /(\d+\/\d+)$/.exec(text)?.[1];
  return count ? `${mark} ${count}` : mark;
}
