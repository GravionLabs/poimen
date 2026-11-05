import { StateEffect, StateField, type Extension, type Range } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  WidgetType,
  lineNumbers,
  type DecorationSet,
} from "@codemirror/view";

/** A saved comment as the editor shows it (one side of the diff). */
export interface DiffComment {
  id: string;
  start: number;
  end: number;
  text: string;
}

/** A review finding as the editor shows it: read-only, with a button to send it to the agent. */
export interface FindingMark {
  id: string;
  start: number;
  end: number;
  severity: "blocking" | "note";
  title: string;
  why: string;
  /** Already among the pending comments. */
  added: boolean;
}

/** The range a comment is being written for. */
export interface Draft {
  start: number;
  end: number;
}

/** What the editor tells its owner: line number clicks and the draft box buttons. */
export interface CommentHandlers {
  line(line: number, shift: boolean): void;
  save(text: string): void;
  cancel(): void;
  /** "Send to agent" on a finding. */
  finding(id: string): void;
}

export const setComments = StateEffect.define<readonly DiffComment[]>();
export const setDraft = StateEffect.define<Draft | null>();
export const setFindings = StateEffect.define<readonly FindingMark[]>();

class FindingBox extends WidgetType {
  constructor(
    private readonly mark: FindingMark,
    private readonly handlers: CommentHandlers,
  ) {
    super();
  }
  override eq(other: FindingBox): boolean {
    const a = this.mark;
    const b = other.mark;
    return (
      a.id === b.id &&
      a.start === b.start &&
      a.end === b.end &&
      a.severity === b.severity &&
      a.title === b.title &&
      a.why === b.why &&
      a.added === b.added
    );
  }
  override toDOM(): HTMLElement {
    const m = this.mark;
    const box = document.createElement("div");
    box.className = `cm-comment-box cm-finding-box cm-finding-${m.severity}`;
    const head = document.createElement("div");
    head.className = "cm-comment-range";
    const lines = m.start === m.end ? `Line ${m.start}` : `Lines ${m.start}-${m.end}`;
    head.textContent = `${m.severity} · ${lines}`;
    const title = document.createElement("div");
    title.className = "cm-finding-title";
    title.textContent = m.title;
    const why = document.createElement("div");
    why.className = "cm-comment-text";
    why.textContent = m.why;
    const send = document.createElement("button");
    send.type = "button";
    send.className = "cm-finding-send";
    send.textContent = m.added ? "Added to comments" : "Send to agent";
    send.disabled = m.added;
    send.addEventListener("click", () => this.handlers.finding(m.id));
    const actions = document.createElement("div");
    actions.className = "cm-comment-actions";
    actions.append(send);
    box.append(head, title, why, actions);
    return box;
  }
  override ignoreEvent(): boolean {
    return true;
  }
}

class CommentBox extends WidgetType {
  constructor(private readonly comment: DiffComment) {
    super();
  }
  override eq(other: CommentBox): boolean {
    const a = this.comment;
    const b = other.comment;
    return a.id === b.id && a.text === b.text && a.start === b.start && a.end === b.end;
  }
  override toDOM(): HTMLElement {
    const box = document.createElement("div");
    box.className = "cm-comment-box";
    const range = document.createElement("div");
    range.className = "cm-comment-range";
    range.textContent =
      this.comment.start === this.comment.end
        ? `Line ${this.comment.start}`
        : `Lines ${this.comment.start}-${this.comment.end}`;
    const text = document.createElement("div");
    text.className = "cm-comment-text";
    text.textContent = this.comment.text;
    box.append(range, text);
    return box;
  }
  override ignoreEvent(): boolean {
    return false;
  }
}

class DraftBox extends WidgetType {
  constructor(
    private readonly draft: Draft,
    private readonly handlers: CommentHandlers,
  ) {
    super();
  }
  override eq(other: DraftBox): boolean {
    return other.draft.start === this.draft.start && other.draft.end === this.draft.end;
  }
  override toDOM(): HTMLElement {
    const box = document.createElement("div");
    box.className = "cm-comment-box cm-comment-draft";
    const input = document.createElement("textarea");
    input.className = "cm-comment-input";
    input.rows = 3;
    input.placeholder =
      this.draft.start === this.draft.end
        ? `Comment on line ${this.draft.start}`
        : `Comment on lines ${this.draft.start}-${this.draft.end}`;
    input.setAttribute("aria-label", input.placeholder);
    const save = document.createElement("button");
    save.type = "button";
    save.className = "cm-comment-save";
    save.textContent = "Add comment";
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "cm-comment-cancel";
    cancel.textContent = "Cancel";
    save.addEventListener("click", () => this.handlers.save(input.value));
    cancel.addEventListener("click", () => this.handlers.cancel());
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") this.handlers.cancel();
      else if (event.key === "Enter" && (event.ctrlKey || event.metaKey))
        this.handlers.save(input.value);
      else return;
      event.preventDefault();
    });
    const actions = document.createElement("div");
    actions.className = "cm-comment-actions";
    actions.append(save, cancel);
    box.append(input, actions);
    queueMicrotask(() => input.focus());
    return box;
  }
  override ignoreEvent(): boolean {
    return true;
  }
}

interface Shown {
  comments: readonly DiffComment[];
  draft: Draft | null;
  findings: readonly FindingMark[];
  decorations: DecorationSet;
}

function decorate(
  view: { doc: EditorView["state"]["doc"] },
  comments: readonly DiffComment[],
  draft: Draft | null,
  findings: readonly FindingMark[],
  handlers: CommentHandlers,
): DecorationSet {
  const doc = view.doc;
  const ranges: Range<Decoration>[] = [];
  const mark = (start: number, end: number, widget: WidgetType, cls = "cm-commented-line") => {
    if (start < 1 || end < start || end > doc.lines) return;
    for (let n = start; n <= end; n++) {
      ranges.push(Decoration.line({ class: cls }).range(doc.line(n).from));
    }
    ranges.push(Decoration.widget({ widget, block: true, side: 1 }).range(doc.line(end).to));
  };
  for (const c of comments) mark(c.start, c.end, new CommentBox(c));
  for (const f of findings) {
    mark(
      f.start,
      f.end,
      new FindingBox(f, handlers),
      `cm-finding-line cm-finding-line-${f.severity}`,
    );
  }
  if (draft) mark(draft.start, draft.end, new DraftBox(draft, handlers));
  return Decoration.set(ranges, true);
}

/**
 * Line comments on a CodeMirror editor: a line number click (with Shift, a range) reports through
 * `handlers.line`, saved comments and the draft box are block widgets below their last line.
 * Fed with the {@link setComments} and {@link setDraft} effects.
 */
export function commentExtension(handlers: CommentHandlers): Extension {
  const field = StateField.define<Shown>({
    create: () => ({ comments: [], draft: null, findings: [], decorations: Decoration.none }),
    update(value, tr) {
      let { comments, draft, findings } = value;
      let changed = false;
      for (const effect of tr.effects) {
        if (effect.is(setComments)) {
          comments = effect.value;
          changed = true;
        } else if (effect.is(setFindings)) {
          findings = effect.value;
          changed = true;
        } else if (effect.is(setDraft)) {
          draft = effect.value;
          changed = true;
        }
      }
      if (!changed) return value;
      return {
        comments,
        draft,
        findings,
        decorations: decorate(tr.state, comments, draft, findings, handlers),
      };
    },
    provide: (f) => EditorView.decorations.from(f, (v) => v.decorations),
  });
  return [
    field,
    lineNumbers({
      domEventHandlers: {
        mousedown(view, block, event) {
          handlers.line(view.state.doc.lineAt(block.from).number, (event as MouseEvent).shiftKey);
          return true;
        },
      },
    }),
    EditorView.baseTheme({
      ".cm-lineNumbers .cm-gutterElement": { cursor: "pointer" },
      ".cm-commented-line": {
        backgroundColor: "color-mix(in srgb, var(--term-ansi-4) 14%, transparent)",
      },
      ".cm-finding-line-blocking": {
        backgroundColor: "color-mix(in srgb, var(--term-ansi-1) 14%, transparent)",
      },
      ".cm-finding-line-note": {
        backgroundColor: "color-mix(in srgb, var(--term-ansi-3) 14%, transparent)",
      },
      ".cm-finding-blocking": { borderColor: "var(--term-ansi-1)" },
      ".cm-finding-note": { borderColor: "var(--term-ansi-3)" },
      ".cm-finding-title": { fontWeight: "600" },
      ".cm-finding-send:disabled": { opacity: "0.6", cursor: "default" },
      ".cm-comment-box": {
        margin: "4px 8px 4px 48px",
        padding: "6px 8px",
        border: "1px solid var(--term-ansi-4)",
        borderRadius: "4px",
        backgroundColor: "var(--term-bg)",
        color: "var(--term-fg)",
        fontFamily: "var(--font-sans, sans-serif)",
      },
      ".cm-comment-range": { color: "var(--term-ansi-8)", fontSize: "11px" },
      ".cm-comment-text": { whiteSpace: "pre-wrap" },
      ".cm-comment-input": {
        display: "block",
        width: "100%",
        boxSizing: "border-box",
        font: "inherit",
        color: "inherit",
        background: "transparent",
        border: "1px solid var(--term-ansi-8)",
        resize: "vertical",
      },
      ".cm-comment-actions": { display: "flex", gap: "6px", marginTop: "6px" },
      ".cm-comment-actions button": {
        font: "inherit",
        color: "inherit",
        background: "transparent",
        border: "1px solid var(--term-ansi-8)",
        padding: "1px 8px",
        cursor: "pointer",
      },
    }),
  ];
}
