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

/** The image types `paste_image` accepts. */
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/**
 * The image item of a paste that has no text: a paste with `text/plain` stays a text paste.
 * `types` are the MIME types of the clipboard items.
 */
export function pastedImageType(types: readonly string[]): string | null {
  if (types.includes("text/plain")) return null;
  return types.find((t) => t.startsWith("image/")) ?? null;
}

/** What an image paste does: type the quoted path of the saved file, or refuse. */
export function imagePasteRefusal(opts: { remote: boolean; control: boolean }): string | null {
  if (opts.remote) {
    return "A pasted image is saved on this Mac and does not exist on a remote machine.";
  }
  if (!opts.control) return "This terminal is read-only: the pasted image was not typed.";
  return null;
}

/** Whether `mime` is an image type poimen saves. */
export function savableImage(mime: string): boolean {
  return IMAGE_TYPES.includes(mime);
}
