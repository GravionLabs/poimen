import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  effect,
  inject,
  Input,
  signal,
} from "@angular/core";
import { HxTheme } from "@gravionlabs/helix-ui";
import { MergeView, goToNextChunk, goToPreviousChunk, unifiedMergeView } from "@codemirror/merge";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { diffHighlight, loadLanguage } from "./diff-language";
import type { DiffJump, DiffLayout } from "./changes";

/**
 * The CodeMirror theme as style rules over poimen's color tokens (`--term-*` on `<html>`, set by
 * `styles.css` and the color scheme), so a scheme or light/dark switch restyles it without a
 * rebuild. Added lines use the ANSI green, removed lines the ANSI red.
 */
export function diffThemeRules(): Record<string, Record<string, string>> {
  const tint = (color: string, share: number) =>
    `color-mix(in srgb, var(${color}) ${share}%, transparent)`;
  return {
    "&": {
      color: "var(--term-fg)",
      backgroundColor: "var(--term-bg)",
      height: "100%",
      fontSize: "12px",
    },
    ".cm-scroller": { fontFamily: "var(--font-mono)", overflow: "auto" },
    ".cm-content": { caretColor: "var(--term-cursor)" },
    "&.cm-focused": { outline: "none" },
    ".cm-gutters": {
      backgroundColor: "var(--term-bg)",
      color: "var(--term-ansi-8)",
      border: "none",
    },
    ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "transparent" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection":
      { backgroundColor: "var(--term-selection-bg)" },
    ".cm-insertedLine, .cm-changedLine": { backgroundColor: tint("--term-ansi-2", 14) },
    ".cm-deletedLine, .cm-deletedChunk": { backgroundColor: tint("--term-ansi-1", 14) },
    "ins.cm-insertedLine, ins.cm-changedText, .cm-insertedLine .cm-changedText": {
      backgroundColor: tint("--term-ansi-2", 32),
    },
    "del.cm-deletedLine, del, .cm-deletedLine .cm-changedText, .cm-deletedChunk .cm-changedText": {
      backgroundColor: tint("--term-ansi-1", 32),
    },
    ".cm-changedLineGutter": { background: "var(--term-ansi-2)" },
    ".cm-deletedLineGutter": { background: "var(--term-ansi-1)" },
    ".cm-collapsedLines": {
      color: "var(--term-ansi-8)",
      backgroundColor: "var(--term-bg)",
      cursor: "pointer",
    },
  };
}

/**
 * A read-only diff of `old` against `new` with CodeMirror's merge view: inline (one pane with the
 * removed lines in between) or side by side. The only place that imports CodeMirror.
 */
@Component({
  selector: "app-diff-editor",
  template: "",
  styles: `
    :host {
      display: block;
      min-height: 0;
      min-width: 0;
      overflow: hidden;
    }
    :host ::ng-deep .cm-mergeView {
      height: 100%;
    }
    :host ::ng-deep .cm-mergeViewEditors {
      height: 100%;
    }
    :host ::ng-deep .cm-mergeViewEditor {
      flex: 1;
      min-width: 0;
    }
    :host ::ng-deep .cm-mergeViewEditor > .cm-editor {
      height: 100%;
    }
    :host ::ng-deep .cm-mergeView .cm-scroller {
      overflow: auto;
    }
    :host ::ng-deep .cm-merge-gutter {
      width: 4px;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DiffEditor {
  // Decorator inputs (set into signals): the specs run without Angular's compiler transform.
  private readonly oldText = signal("");
  private readonly newText = signal("");
  private readonly filePath = signal("");
  private readonly mode = signal<DiffLayout>("split");
  @Input({ required: true }) set old(value: string) {
    this.oldText.set(value);
  }
  @Input({ required: true }) set new(value: string) {
    this.newText.set(value);
  }
  @Input() set path(value: string) {
    this.filePath.set(value);
  }
  @Input() set layout(value: DiffLayout) {
    this.mode.set(value);
  }
  @Input() set jump(request: DiffJump | null) {
    if (request?.step === "next") this.next();
    else if (request?.step === "previous") this.previous();
  }

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly hxTheme = inject(HxTheme);
  private view: EditorView | null = null;
  private merge: MergeView | null = null;
  private generation = 0;

  constructor() {
    effect(() => {
      const old = this.oldText();
      const next = this.newText();
      const layout = this.mode();
      const dark = this.hxTheme.dark();
      const path = this.filePath();
      this.build(old, next, layout, dark, path);
    });
    inject(DestroyRef).onDestroy(() => this.dispose());
  }

  /** Scrolls to the next changed chunk and puts the cursor there. */
  next(): void {
    const view = this.active();
    if (view) goToNextChunk(view);
  }

  /** Scrolls to the previous changed chunk. */
  previous(): void {
    const view = this.active();
    if (view) goToPreviousChunk(view);
  }

  /** Gives the diff the keyboard focus. */
  focus(): void {
    this.active()?.focus();
  }

  /** The view the chunk commands act on: the editor of the new content. */
  private active(): EditorView | null {
    return this.merge?.b ?? this.view;
  }

  private build(old: string, next: string, layout: DiffLayout, dark: boolean, path: string): void {
    this.dispose();
    const language = new Compartment();
    const common: Extension[] = [
      language.of([]),
      diffHighlight(),
      EditorState.readOnly.of(true),
      EditorView.theme(diffThemeRules(), { dark }),
      EditorView.lineWrapping,
    ];
    const collapse = { margin: 3, minSize: 4 };
    if (layout === "inline") {
      this.view = new EditorView({
        parent: this.host.nativeElement,
        doc: next,
        extensions: [
          ...common,
          unifiedMergeView({
            original: old,
            collapseUnchanged: collapse,
            mergeControls: false,
            highlightChanges: true,
            gutter: true,
          }),
        ],
      });
    } else {
      this.merge = new MergeView({
        parent: this.host.nativeElement,
        a: { doc: old, extensions: common },
        b: { doc: next, extensions: common },
        collapseUnchanged: collapse,
        highlightChanges: true,
        gutter: true,
      });
    }
    const generation = this.generation;
    void loadLanguage(path).then((support) => {
      if (!support || generation !== this.generation) return;
      const effects = language.reconfigure(support);
      for (const view of [this.view, this.merge?.a, this.merge?.b]) view?.dispatch({ effects });
    });
  }

  private dispose(): void {
    this.generation++;
    this.view?.destroy();
    this.merge?.destroy();
    this.view = null;
    this.merge = null;
  }
}
