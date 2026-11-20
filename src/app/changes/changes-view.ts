import {
  ChangeDetectionStrategy,
  Component,
  Input,
  DestroyRef,
  ElementRef,
  HostListener,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { HxButton, HxSelectButton } from "@gravionlabs/helix-ui";
import { UiState } from "../core/ui-state";
import {
  CHANGES_SCOPES,
  effectiveLayout,
  type DiffJump,
  type DiffLayout,
  Changes,
  statusLetter,
  type ChangedFile,
  type ChangesScope,
} from "./changes";
import { CommentsPanel } from "./comments-panel";
import { DiffEditor } from "./diff-editor";
import type { FindingMark } from "./comment-extension";
import {
  Findings,
  countLines,
  findingComment,
  lineCounts,
  mapFindings,
  type IndexedFinding,
} from "./findings";
import { FindingsPanel } from "./findings-panel";
import { LineComments } from "./line-comments";

const SCOPE_LABEL: Record<ChangesScope, string> = {
  base: "Base",
  uncommitted: "Uncommitted",
};

/**
 * The Changes editor tab of one directory (space or worktree): the changed files on the left and
 * the diff of the selected one on the right, inline or side by side.
 */
@Component({
  selector: "app-changes-view",
  imports: [DiffEditor, CommentsPanel, FindingsPanel, HxButton, HxSelectButton],
  providers: [Changes, LineComments, Findings],
  templateUrl: "./changes-view.html",
  styles: `
    :host {
      flex: 1;
      display: flex;
      flex-direction: column;
      min-height: 0;
      min-width: 0;
      font-size: 12px;
      background: var(--h-content-background);
      color: var(--h-text-color);
    }
    header {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 12px;
      padding: 8px 12px;
      border-bottom: 1px solid var(--h-content-border-color);
    }
    .label,
    .count {
      color: var(--h-text-muted-color);
    }
    .ref {
      font-family: var(--font-mono);
    }
    .spacer {
      flex: 1;
    }
    .add {
      color: var(--term-ansi-2);
    }
    .del {
      color: var(--term-ansi-1);
    }
    .totals {
      display: inline-flex;
      gap: 6px;
    }
    .body {
      flex: 1;
      display: flex;
      min-height: 0;
    }
    .files {
      list-style: none;
      margin: 0;
      padding: 0;
      width: 280px;
      min-width: 160px;
      overflow: auto;
      border-right: 1px solid var(--h-content-border-color);
    }
    .file {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 3px 8px;
      cursor: pointer;
    }
    .file.selected {
      background: var(--h-content-hover-background);
    }
    .status {
      width: 14px;
      text-align: center;
      font-family: var(--font-mono);
      font-weight: 600;
    }
    .s-added,
    .s-untracked {
      color: var(--term-ansi-2);
    }
    .s-deleted {
      color: var(--term-ansi-1);
    }
    .s-renamed {
      color: var(--term-ansi-4);
    }
    .s-modified {
      color: var(--term-ansi-3);
    }
    .path {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      direction: rtl;
      text-align: left;
    }
    .stat {
      display: inline-flex;
      gap: 4px;
      font-family: var(--font-mono);
      color: var(--h-text-muted-color);
    }
    .diff {
      flex: 1;
      min-width: 0;
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .diff app-diff-editor {
      flex: 1;
    }
    .git {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 12px;
      border-bottom: 1px solid var(--h-content-border-color);
    }
    .message {
      flex: 1;
      min-width: 0;
      font: inherit;
      color: inherit;
      background: transparent;
      border: 1px solid var(--h-content-border-color);
      padding: 2px 6px;
      resize: vertical;
    }
    .result {
      margin: 0;
      padding: 4px 12px;
      max-height: 120px;
      overflow: auto;
      white-space: pre-wrap;
      font-family: var(--font-mono);
      border-bottom: 1px solid var(--h-content-border-color);
    }
    .result.failed {
      color: var(--status-error);
    }
    .findings-note {
      margin: 0;
      padding: 4px 12px;
      color: var(--term-ansi-3);
      border-bottom: 1px solid var(--h-content-border-color);
    }
    .findings-note.invalid {
      color: var(--status-error);
    }
    .note,
    .empty,
    .problem {
      margin: auto;
      max-width: 420px;
      text-align: center;
      color: var(--h-text-muted-color);
    }
    .problem {
      color: var(--status-error);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChangesView {
  protected readonly changes = inject(Changes);
  protected readonly lineComments = inject(LineComments);
  protected readonly findings = inject(Findings);
  private readonly ui = inject(UiState);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** The latest `n`/`p` press, handed to the diff as an input (a new object each time). */
  protected readonly jump = signal<DiffJump | null>(null);

  /**
   * The directory whose changes the tab shows. A decorator input (set into a signal) because the
   * specs run without Angular's compiler transform, which signal inputs need.
   */
  protected readonly path = signal("");
  @Input({ required: true }) set dir(dir: string) {
    this.path.set(dir);
  }

  /** The findings split into markers of the open diff and "Other findings". */
  protected readonly mapped = computed(() => {
    const path = this.changes.selected();
    const content = this.changes.content();
    // A file shown without a diff (binary, too large) cannot hold markers.
    let selected: { path: string; lines: number } | null = null;
    if (path && content?.state === "text") selected = { path, lines: countLines(content.new) };
    else if (path && content) selected = { path, lines: 0 };
    return mapFindings(this.findings.all(), lineCounts(this.changes.files(), selected));
  });
  protected readonly marks = computed<FindingMark[]>(() => {
    const path = this.changes.selected();
    const added = this.findings.added();
    return (path ? (this.mapped().located.get(path) ?? []) : []).map(({ id, finding }) => ({
      id,
      start: finding.line as number,
      end: Math.max(finding.line as number, finding.endLine ?? (finding.line as number)),
      severity: finding.severity,
      title: finding.title,
      why: finding.why,
      added: added.has(id),
    }));
  });

  protected readonly scopeOptions = CHANGES_SCOPES.map((value) => ({
    value,
    label: SCOPE_LABEL[value],
  }));
  protected readonly statusLetter = statusLetter;

  /** Width of the tab; unknown (no resize observer) counts as wide. */
  private readonly width = signal(Number.POSITIVE_INFINITY);
  protected readonly chosen = this.ui.changesLayout;
  protected readonly layout = computed<DiffLayout>(() =>
    effectiveLayout(this.chosen(), this.width()),
  );
  protected readonly narrow = computed(() => this.layout() !== this.chosen());
  protected readonly layoutOptions = computed(() => [
    { value: "inline", label: "Inline", disabled: false },
    { value: "split", label: "Side by side", disabled: this.narrow() },
  ]);

  constructor() {
    // Opening the tab (it is created when it gets focus) reads the changes.
    effect(() => {
      const dir = this.path();
      if (!dir) return;
      this.lineComments.follow(dir);
      untracked(() => void this.changes.follow(dir));
    });
    // Every read of the changes (open, refresh button, finished agent) reads the findings too.
    effect(() => {
      this.findings.follow(this.path());
      if (this.changes.list()) untracked(() => void this.findings.refresh());
    });
    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver((entries) => {
        const width = entries[0]?.contentRect.width;
        if (width !== undefined) this.width.set(width);
      });
      observer.observe(this.host.nativeElement);
      inject(DestroyRef).onDestroy(() => observer.disconnect());
    }
  }

  protected sendMark(id: string, quote: string): void {
    const entry = this.findings.all()[Number(id)];
    if (!entry) return;
    this.lineComments.add(findingComment(entry, quote));
    this.findings.markAdded(id);
  }

  protected sendOther(item: IndexedFinding): void {
    this.lineComments.add(findingComment(item.finding));
    this.findings.markAdded(item.id);
  }

  protected setLayout(layout: DiffLayout): void {
    this.ui.setChangesLayout(layout);
  }

  protected generateHint(): string {
    if (!this.changes.stagedCount()) return "Nothing is staged";
    return "Draft a Conventional Commit message from the staged diff (does not commit)";
  }

  protected commitHint(): string {
    if (!this.changes.stagedCount()) return "Nothing is staged";
    if (!this.changes.commitMessage().trim()) return "Enter a commit message";
    return "Commit the staged files";
  }

  protected title(file: ChangedFile): string {
    return file.old_path ? `${file.old_path} → ${file.path}` : file.path;
  }

  /** Up and Down move through the file list. */
  protected onListKey(event: KeyboardEvent): void {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      void this.changes.move(event.key === "ArrowDown" ? 1 : -1);
    }
  }

  /** `n` and `p` jump to the next and previous change of the open diff. */
  @HostListener("keydown", ["$event"])
  protected onKey(event: KeyboardEvent): void {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (
      event.target instanceof HTMLElement &&
      event.target.closest("button, input, select, textarea")
    )
      return;
    if (event.key === "n") this.jump.set({ step: "next" });
    else if (event.key === "p") this.jump.set({ step: "previous" });
    else return;
    event.preventDefault();
  }
}
