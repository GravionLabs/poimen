import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  Input,
  effect,
  inject,
  signal,
} from "@angular/core";
import { HxTheme } from "@gravionlabs/helix-ui";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { diffThemeRules } from "../changes/diff-editor";
import { diffHighlight, loadLanguage } from "../changes/diff-language";

/**
 * Read-only source of one file with the syntax highlighting of the diff editor (same theme rules
 * and language loading). A new `text` of the same file replaces the document in place, so the
 * scroll position stays when the file changes on disk.
 */
@Component({
  selector: "app-source-view",
  templateUrl: "./source-view.html",
  styles: `
    :host {
      display: block;
      min-height: 0;
      min-width: 0;
      overflow: hidden;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SourceView {
  // Decorator inputs: the specs run without Angular's compiler transform.
  private readonly source = signal("");
  private readonly filePath = signal("");
  @Input({ required: true }) set text(value: string) {
    this.source.set(value);
  }
  @Input() set path(value: string) {
    this.filePath.set(value);
  }

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly hxTheme = inject(HxTheme);
  private view: EditorView | null = null;
  private builtFor: { path: string; dark: boolean } | null = null;
  private generation = 0;
  private readonly language = new Compartment();

  constructor() {
    effect(() => {
      const path = this.filePath();
      const dark = this.hxTheme.dark();
      const text = this.source();
      const same =
        this.view && this.builtFor?.path === path && this.builtFor.dark === dark ? this.view : null;
      if (same) {
        if (same.state.doc.toString() !== text) {
          same.dispatch({ changes: { from: 0, to: same.state.doc.length, insert: text } });
        }
        return;
      }
      this.build(text, path, dark);
    });
    inject(DestroyRef).onDestroy(() => this.dispose());
  }

  private build(text: string, path: string, dark: boolean): void {
    this.dispose();
    this.builtFor = { path, dark };
    const extensions: Extension[] = [
      this.language.of([]),
      diffHighlight(),
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.theme(diffThemeRules(), { dark }),
      EditorView.lineWrapping,
    ];
    const view = new EditorView({ parent: this.host.nativeElement, doc: text, extensions });
    this.view = view;
    const generation = this.generation;
    void loadLanguage(path).then((support) => {
      if (support && generation === this.generation) {
        view.dispatch({ effects: this.language.reconfigure(support) });
      }
    });
  }

  private dispose(): void {
    this.generation++;
    this.view?.destroy();
    this.view = null;
    this.builtFor = null;
  }
}
