import { Injectable, computed, inject, signal } from "@angular/core";
import { TauriBridge } from "../core/tauri";
import type { LineComment } from "../core/ui-state";
import type { ChangedFile } from "./changes";

export type Severity = "blocking" | "note";

/** One finding of `.agents/state/findings.json` (`Finding` of `changes.rs`, `docs/findings.md`). */
export interface Finding {
  file?: string;
  line?: number;
  endLine?: number;
  severity: Severity;
  title: string;
  why: string;
  rule?: string;
}

export interface FindingsFile {
  version: number;
  sha: string;
  source: string;
  findings: Finding[];
}

/** The answer of `changes_findings`. */
export type FindingsResult =
  | { state: "none" }
  | { state: "ok"; head: string; stale: boolean; findings: FindingsFile }
  | { state: "invalid"; error: string };

const NONE: FindingsResult = { state: "none" };

/** A finding with its position in the file, which identifies it. */
export interface IndexedFinding {
  id: string;
  finding: Finding;
}

export interface MappedFindings {
  /** Findings with a line in a changed file, by file path. */
  located: Map<string, IndexedFinding[]>;
  /** Everything else: files without a diff, no location, lines beyond the file. */
  other: IndexedFinding[];
}

/**
 * Maps findings onto the diff. `lines` holds the changed files and, where known, the line count of
 * their new content (a deleted file has none): a finding sits at its line when the file is changed
 * and the line exists, else it is "other".
 */
export function mapFindings(
  findings: readonly Finding[],
  lines: ReadonlyMap<string, number | undefined>,
): MappedFindings {
  const located = new Map<string, IndexedFinding[]>();
  const other: IndexedFinding[] = [];
  findings.forEach((finding, i) => {
    const entry = { id: String(i), finding };
    const count = finding.file !== undefined ? lines.get(finding.file) : undefined;
    const known = finding.file !== undefined && lines.has(finding.file);
    if (known && finding.line !== undefined && (count === undefined || finding.line <= count)) {
      const list = located.get(finding.file as string) ?? [];
      list.push(entry);
      located.set(finding.file as string, list);
    } else {
      other.push(entry);
    }
  });
  return { located, other };
}

/** The line counts `mapFindings` takes: the selected file's is known from its content. */
export function lineCounts(
  files: readonly ChangedFile[],
  selected: { path: string; lines: number } | null,
): Map<string, number | undefined> {
  const map = new Map<string, number | undefined>();
  for (const f of files) map.set(f.path, f.status === "deleted" ? 0 : undefined);
  if (selected && map.has(selected.path) && map.get(selected.path) !== 0) {
    map.set(selected.path, selected.lines);
  }
  return map;
}

/** Number of lines of a text (a final newline does not start another one). */
export function countLines(text: string): number {
  if (!text) return 0;
  return text.endsWith("\n") ? text.split("\n").length - 1 : text.split("\n").length;
}

/** `file:line`, `file:start-end`, `file` or `no location`. */
export function findingLocation(f: Finding): string {
  if (!f.file) return "no location";
  if (f.line === undefined) return f.file;
  return f.endLine !== undefined && f.endLine !== f.line
    ? `${f.file}:${f.line}-${f.endLine}`
    : `${f.file}:${f.line}`;
}

/** The first characters of a commit id. */
export function shortSha(sha: string): string {
  return sha.slice(0, 7);
}

/**
 * A finding as a pending comment of the worktree (#245). Start and end 0 mean "no line"; a
 * finding without a file gets the path `(general)`. `quote` is the code at its lines, if known.
 */
export function findingComment(f: Finding, quote = ""): Omit<LineComment, "id"> {
  const start = f.line ?? 0;
  const end = f.line === undefined ? 0 : Math.max(start, f.endLine ?? start);
  return {
    path: f.file ?? "(general)",
    side: "new",
    start,
    end,
    quote,
    text: `[${f.severity}] ${f.title}\n\n${f.why}`,
  };
}

/** The review findings of one worktree, read from `.agents/state/findings.json`. Provided per tab. */
@Injectable()
export class Findings {
  private readonly bridge = inject(TauriBridge);

  readonly dir = signal("");
  readonly result = signal<FindingsResult>(NONE);
  /** Ids of findings already added to the pending comments (until the next change of the file). */
  readonly added = signal<ReadonlySet<string>>(new Set());

  readonly file = computed(() => {
    const r = this.result();
    return r.state === "ok" ? r.findings : null;
  });
  readonly all = computed(() => this.file()?.findings ?? []);
  readonly stale = computed(() => {
    const r = this.result();
    return r.state === "ok" && r.stale;
  });
  readonly invalid = computed(() => {
    const r = this.result();
    return r.state === "invalid" ? r.error : null;
  });
  /** Why the findings are stale, for the note. */
  readonly staleNote = computed(() => {
    const r = this.result();
    if (r.state !== "ok" || !r.stale) return null;
    return `These findings are for ${shortSha(r.findings.sha)}, but HEAD is ${shortSha(r.head)}.`;
  });

  private run = 0;

  follow(dir: string): void {
    this.dir.set(dir);
  }

  /** Reads the file again; a failed read counts as an invalid file, never as a failed diff. */
  async refresh(): Promise<void> {
    const dir = this.dir();
    if (!dir) return;
    const run = ++this.run;
    let result: FindingsResult;
    try {
      result =
        (await this.bridge.invoke<FindingsResult | undefined>("changes_findings", { dir })) ?? NONE;
    } catch (error) {
      result = { state: "invalid", error: String(error) };
    }
    if (run !== this.run) return;
    // Ids are positions in the file: a changed file starts over.
    if (JSON.stringify(result) !== JSON.stringify(this.result())) this.added.set(new Set());
    this.result.set(result);
  }

  markAdded(id: string): void {
    this.added.update((set) => new Set(set).add(id));
  }
}
