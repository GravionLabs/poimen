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
import { DiffEditor } from "./diff-editor";

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
  imports: [DiffEditor],
  providers: [Changes],
  template: `
    <header>
      <div class="base" [title]="path()">
        <span class="label">Base</span>
        <code class="ref">{{ changes.list()?.base ?? "…" }}</code>
      </div>
      <div class="group" role="group" aria-label="Scope">
        @for (scope of scopes; track scope) {
          <button
            type="button"
            class="toggle"
            [class.on]="changes.scope() === scope"
            [attr.aria-pressed]="changes.scope() === scope"
            (click)="changes.setScope(scope)"
          >
            {{ scopeLabel[scope] }}
          </button>
        }
      </div>
      <span class="totals">
        <span class="add">+{{ changes.totals().additions }}</span>
        <span class="del">−{{ changes.totals().deletions }}</span>
        <span class="count"
          >{{ changes.totals().files }} {{ changes.totals().files === 1 ? "file" : "files" }}</span
        >
      </span>
      <span class="spacer"></span>
      <div class="group" role="group" aria-label="Diff layout">
        <button
          type="button"
          class="toggle"
          [class.on]="layout() === 'inline'"
          [attr.aria-pressed]="layout() === 'inline'"
          (click)="setLayout('inline')"
        >
          Inline
        </button>
        <button
          type="button"
          class="toggle"
          [class.on]="layout() === 'split'"
          [attr.aria-pressed]="layout() === 'split'"
          [disabled]="narrow()"
          [title]="narrow() ? 'The tab is too narrow for side by side' : ''"
          (click)="setLayout('split')"
        >
          Side by side
        </button>
      </div>
      <button
        type="button"
        class="refresh"
        aria-label="Refresh"
        title="Refresh"
        [disabled]="changes.loading()"
        (click)="changes.refresh()"
      >
        Refresh
      </button>
    </header>

    @if (changes.error(); as error) {
      <p class="problem" role="alert">{{ error }}</p>
    } @else if (changes.list() && !changes.files().length) {
      <p class="empty">No changes against {{ changes.list()?.base }}.</p>
    } @else if (changes.list()) {
      <div class="body">
        <ul
          class="files"
          role="listbox"
          aria-label="Changed files"
          tabindex="0"
          [attr.aria-activedescendant]="changes.selected() ? 'file-' + changes.selected() : null"
          (keydown)="onListKey($event)"
        >
          @for (file of changes.files(); track file.path) {
            <li
              role="option"
              class="file"
              [id]="'file-' + file.path"
              [class.selected]="file.path === changes.selected()"
              [attr.aria-selected]="file.path === changes.selected()"
              [title]="title(file)"
              tabindex="-1"
              (click)="changes.select(file.path)"
              (keydown.enter)="changes.select(file.path)"
            >
              <span class="status s-{{ file.status }}" [attr.aria-label]="file.status">{{
                statusLetter(file.status)
              }}</span>
              <span class="path">{{ file.path }}</span>
              @if (file.binary) {
                <span class="stat">binary</span>
              } @else {
                <span class="stat"
                  ><span class="add">+{{ file.additions }}</span>
                  <span class="del">−{{ file.deletions }}</span></span
                >
              }
            </li>
          }
        </ul>
        <section class="diff" aria-label="Diff">
          @if (changes.contentError(); as error) {
            <p class="problem" role="alert">{{ error }}</p>
          } @else {
            @let content = changes.content();
            @if (content?.state === "binary") {
              <p class="note">Binary file, no diff shown.</p>
            } @else if (content?.state === "too_large") {
              <p class="note">File too large, no diff shown (limit 1 MiB).</p>
            } @else if (content?.state === "text") {
              @defer {
                <app-diff-editor
                  [old]="content.old"
                  [new]="content.new"
                  [layout]="layout()"
                  [jump]="jump()"
                />
              }
            }
          }
        </section>
      </div>
    }
  `,
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
      border-bottom: 1px solid var(--h-border-color, var(--h-surface-300));
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
    .group {
      display: inline-flex;
    }
    button {
      font: inherit;
      color: inherit;
      background: transparent;
      border: 1px solid var(--h-border-color, var(--h-surface-300));
      padding: 2px 10px;
      cursor: pointer;
    }
    button:disabled {
      opacity: 0.5;
      cursor: default;
    }
    .group button:not(:first-child) {
      margin-left: -1px;
    }
    .toggle.on {
      background: var(--h-surface-200);
      font-weight: 600;
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
      border-right: 1px solid var(--h-border-color, var(--h-surface-300));
    }
    .file {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 3px 8px;
      cursor: pointer;
    }
    .file.selected {
      background: var(--h-surface-200);
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
    .note,
    .empty,
    .problem {
      margin: auto;
      max-width: 420px;
      text-align: center;
      color: var(--h-text-muted-color);
    }
    .problem {
      color: var(--h-danger-color, #d33);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChangesView {
  protected readonly changes = inject(Changes);
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

  protected readonly scopes = CHANGES_SCOPES;
  protected readonly scopeLabel = SCOPE_LABEL;
  protected readonly statusLetter = statusLetter;

  /** Width of the tab; unknown (no resize observer) counts as wide. */
  private readonly width = signal(Number.POSITIVE_INFINITY);
  protected readonly chosen = this.ui.changesLayout;
  protected readonly layout = computed<DiffLayout>(() =>
    effectiveLayout(this.chosen(), this.width()),
  );
  protected readonly narrow = computed(() => this.layout() !== this.chosen());

  constructor() {
    // Opening the tab (it is created when it gets focus) reads the changes.
    effect(() => {
      const dir = this.path();
      if (dir) untracked(() => void this.changes.follow(dir));
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

  protected setLayout(layout: DiffLayout): void {
    this.ui.setChangesLayout(layout);
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
    if (event.target instanceof HTMLElement && event.target.closest("button, input, select"))
      return;
    if (event.key === "n") this.jump.set({ step: "next" });
    else if (event.key === "p") this.jump.set({ step: "previous" });
    else return;
    event.preventDefault();
  }
}
