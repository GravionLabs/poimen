/** Files dropped on a terminal type their quoted paths (#905). Pure helpers; the view wires them. */

/** POSIX shell: single quotes, a single quote inside closes, escapes and reopens. */
export function quotePosix(path: string): string {
  return `'${path.replace(/'/g, `'\\''`)}'`;
}

/** PowerShell: single quotes, a single quote inside is doubled. */
export function quotePowerShell(path: string): string {
  return `'${path.replace(/'/g, "''")}'`;
}

/** The paths quoted for the shell and joined by spaces; empty for no path. */
export function quotePaths(paths: readonly string[], windows: boolean): string {
  const quote = windows ? quotePowerShell : quotePosix;
  return paths.map(quote).join(" ");
}

/** Whether a drop position (physical pixels of the webview) lies over `rect` (CSS pixels). */
export function overRect(
  position: { x: number; y: number },
  rect: { left: number; top: number; right: number; bottom: number },
  scale: number,
): boolean {
  const x = position.x / scale;
  const y = position.y / scale;
  return x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
}

/** What a drop does: type the paths, or refuse with the notice. */
export type DropAction = { kind: "type"; text: string } | { kind: "refuse"; notice: string };

export function dropAction(
  paths: readonly string[],
  opts: { remote: boolean; control: boolean; windows: boolean },
): DropAction | null {
  if (paths.length === 0) return null;
  if (opts.remote) {
    return {
      kind: "refuse",
      notice: "Dropped files are local paths and do not exist on a remote machine.",
    };
  }
  if (!opts.control) {
    return {
      kind: "refuse",
      notice: "This terminal is read-only: the dropped paths were not typed.",
    };
  }
  return { kind: "type", text: quotePaths(paths, opts.windows) };
}
