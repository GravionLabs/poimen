import type { ILink, ILinkProvider, Terminal } from "@xterm/xterm";

/** A file path found in a line of terminal text (#1003). `start` and `end` are string offsets (end exclusive). */
export interface PathMatch {
  path: string;
  line?: number;
  column?: number;
  start: number;
  end: number;
}

const POSITION = String.raw`(?::(\d+)(?::(\d+))?)?`;
const PATH_CHAR = String.raw`[\w.@+~-]`;
/** A path in quotes: it may hold spaces; a drive letter and its colon are allowed. */
const QUOTED = new RegExp(String.raw`(["'\`])([^"'\`\n]+?)${POSITION}\1`, "g");
/** A path without quotes: no spaces; starts after a character that cannot be part of a path or URL. */
const PLAIN = new RegExp(
  String.raw`(?<![\w/\\.:@+~-])((?:[A-Za-z]:[\\/]|~[\\/]|\.{1,2}[\\/]|[\\/])?${PATH_CHAR}+(?:[\\/]+${PATH_CHAR}+)*)${POSITION}(?![\w/\\@+~-])`,
  "g",
);

/** Whether `path` looks like a file: it has a directory part or an extension (`README.md`). */
function looksLikeFile(path: string): boolean {
  if (/^\.{1,2}$/.test(path) || /[\\/]$/.test(path)) return false;
  if (/^[A-Za-z]:$/.test(path)) return false;
  if (/[\\/]/.test(path)) return true;
  // A bare name: `name.ext` (the extension starts with a letter: not `3.14`, not an e-mail address) or a dotfile.
  return (
    (/^[^.@].*\.[A-Za-z][A-Za-z0-9]*$/.test(path) && !path.includes("@")) ||
    /^\.\w[\w.-]*$/.test(path)
  );
}

function toNumber(text: string | undefined): number | undefined {
  const n = text === undefined ? NaN : Number(text);
  return Number.isSafeInteger(n) && n > 0 ? n : undefined;
}

/**
 * The file paths in `text`: `path`, `path:line` and `path:line:column`, relative, absolute, `~/`
 * or Windows (`C:\dir\file.ts`). A quoted path may hold spaces (the link covers what is inside the
 * quotes); trailing punctuation (`. , ; ) ]`) is not part of an unquoted path; URLs are not paths.
 * Whether the file exists is for the caller to check.
 */
export function findPaths(text: string): PathMatch[] {
  const found: PathMatch[] = [];
  const taken = (start: number, end: number) => found.some((m) => start < m.end && end > m.start);
  for (const m of text.matchAll(QUOTED)) {
    const path = m[2];
    if (!looksLikeFile(path) || /^\w+:\/\//.test(path)) continue;
    const start = m.index + 1;
    found.push({
      path,
      line: toNumber(m[3]),
      column: toNumber(m[4]),
      start,
      end: start + m[0].length - 2,
    });
  }
  for (const m of text.matchAll(PLAIN)) {
    let path = m[1];
    let end = m.index + m[0].length;
    const line = toNumber(m[2]);
    const column = toNumber(m[3]);
    if (m[2] === undefined) {
      // Sentence punctuation after a bare path ("see src/a.ts.") is not part of it.
      const trimmed = path.replace(/[.-]+$/, "");
      end -= path.length - trimmed.length;
      path = trimmed;
    }
    if (!looksLikeFile(path) || taken(m.index, end)) continue;
    found.push({ path, line, column, start: m.index, end });
  }
  return found.sort((a, b) => a.start - b.start);
}

/** What the backend answers for a path that is a file (`resolve_file_links`). */
export interface ResolvedFile {
  path: string;
  repo?: { root: string; rel: string };
}

export interface FileLinkTarget {
  file: ResolvedFile;
  line?: number;
  column?: number;
}

/**
 * The link provider for file paths. It offers a link only for a path that exists (one backend
 * call per line); `activate` gets the click and the target. `dir` is the directory of the pane,
 * empty or undefined when there is none (no links then).
 */
export function fileLinkProvider(
  term: Terminal,
  dir: () => string | undefined,
  resolve: (dir: string, paths: string[]) => Promise<(ResolvedFile | null)[]>,
  activate: (event: MouseEvent, target: FileLinkTarget) => void,
): ILinkProvider {
  return {
    provideLinks(y, callback) {
      const base = dir();
      const line = term.buffer.active.getLine(y - 1);
      if (!base || !line) return callback(undefined);
      const matches = findPaths(line.translateToString(true));
      if (matches.length === 0) return callback(undefined);
      resolve(
        base,
        matches.map((m) => m.path),
      )
        .then((files) => {
          const links: ILink[] = [];
          matches.forEach((m, i) => {
            const file = files[i];
            if (!file) return;
            links.push({
              range: { start: { x: m.start + 1, y }, end: { x: m.end, y } },
              text: line.translateToString(true).slice(m.start, m.end),
              decorations: { underline: true, pointerCursor: true },
              activate: (event) => activate(event, { file, line: m.line, column: m.column }),
            });
          });
          callback(links.length > 0 ? links : undefined);
        })
        .catch(() => callback(undefined));
    },
  };
}

/** Whether a pane gets file links: not a pane of a remote machine, whose files are not on this one (#1003). */
export function offersFileLinks(pane: { machine?: string }): boolean {
  return !pane.machine;
}
