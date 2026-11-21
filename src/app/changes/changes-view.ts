import { NgTemplateOutlet } from "@angular/common";
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
import { ExternalEditor, joinPath } from "../core/external-editor";
import { ContextMenu } from "../ui/context-menu/context-menu";
import { plainGroups } from "../ui/context-menu/menu-model";
import { UiState } from "../core/ui-state";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { sectionsOf, splitPath } from "./change-sections";
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
import { CreatePullRequest } from "./pull-request";
import { PullRequestDialog } from "./pull-request-dialog";
import { LoadingRows } from "../ui/loading/loading-rows";

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
  imports: [
    LoadingRows,
    DiffEditor,
    CommentsPanel,
    FindingsPanel,
    HxButton,
    HxSelectButton,
    ContextMenu,
    DialogShell,
    PullRequestDialog,
    NgTemplateOutlet,
  ],
  providers: [Changes, LineComments, Findings, CreatePullRequest],
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
      margin: 0;
      padding: 0;
      width: 280px;
      min-width: 160px;
      overflow: auto;
      border-right: 1px solid var(--h-content-border-color);
    }
    .section-head {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 2px 4px;
      position: sticky;
      top: 0;
      background: var(--h-content-background);
      border-bottom: 1px solid var(--h-content-border-color);
    }
    .tag {
      color: var(--h-text-muted-color);
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
    .file.picked {
      outline: 1px solid var(--h-content-border-color);
      outline-offset: -1px;
      background: var(--h-content-hover-background);
    }
    .actions {
      display: none;
      gap: 2px;
    }
    .file:hover .actions,
    .file:focus-within .actions,
    .files:focus .file.selected .actions {
      display: inline-flex;
    }
    .file:hover .stat,
    .file:focus-within .stat {
      display: none;
    }
    .discard-list {
      max-height: 160px;
      overflow: auto;
      margin: 0 0 8px;
      padding-left: 18px;
      font-family: var(--font-mono);
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
      display: flex;
      gap: 6px;
      white-space: nowrap;
    }
    .name {
      flex: 0 1 auto;
      min-width: 4ch;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .dir {
      flex: 1 1 0;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      color: var(--h-text-muted-color);
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
    .note.large {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 8px;
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
  protected readonly pullRequest = inject(CreatePullRequest);
  private readonly ui = inject(UiState);
  private readonly externalEditor = inject(ExternalEditor);
  /** The line the last click on a line number of the diff picked, and its file (#1002). */
  private lastLine: { path: string; line: number } | null = null;
  protected readonly menuAt = signal<{
    path: string;
    x: number;
    y: number;
  } | null>(null);
  protected readonly menuGroups = plainGroups([{ id: "open-in-editor", label: "Open in editor" }]);
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
  protected readonly split = splitPath;
  protected readonly sections = computed(() => sectionsOf(this.changes.files()));
  /** The files a Discard waits to confirm. */
  protected readonly discarding = signal<ChangedFile[] | null>(null);

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
    // The branch may have been pushed or got a pull request: read the state with every list.
    effect(() => {
      if (this.changes.list()) untracked(() => void this.pullRequest.load());
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

  protected readonly chooseLine = (chosen: { path: string; line: number }): void => {
    this.lastLine = chosen;
  };

  protected openFileMenu(event: MouseEvent, path: string): void {
    event.preventDefault();
    this.menuAt.set({ path, x: event.clientX, y: event.clientY });
  }

  /** Opens a changed file in the editor, at the picked line when it is a line of that file. */
  protected openFile(path: string): void {
    this.menuAt.set(null);
    const chosen = this.lastLine;
    const line = chosen?.path === path ? chosen.line : undefined;
    void this.externalEditor.open(joinPath(this.path(), path), line === undefined ? {} : { line });
  }

  protected openFinding(item: IndexedFinding): void {
    const { file, line } = item.finding;
    if (file) void this.externalEditor.open(joinPath(this.path(), file), line ? { line } : {});
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

  protected clickRow(event: MouseEvent, file: ChangedFile): void {
    this.changes.pick(file.path, event.shiftKey);
    void this.changes.select(file.path);
  }

  protected stage(file: ChangedFile): void {
    const targets = this.changes.targets(file);
    const stage = !file.staged;
    void this.changes.setStagedMany(
      targets.filter((f) => f.staged !== stage),
      stage,
    );
  }

  protected askDiscard(file: ChangedFile): void {
    this.discarding.set(this.changes.targets(file));
  }

  protected confirmDiscard(): void {
    const files = this.discarding();
    this.discarding.set(null);
    if (files) void this.changes.discard(files);
  }

  /** Up and Down move through the file list, Escape clears the picked files. */
  protected onListKey(event: KeyboardEvent): void {
    if (event.key === "Escape" && this.changes.picked().size) {
      event.preventDefault();
      this.changes.clearPicked();
      return;
    }
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
