import { HxButton, HxCheckbox, HxInput } from "@gravionlabs/helix-ui";
import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { SettingsStore } from "../core/settings";
import type { Snippet } from "../core/snippets-model";

interface Editing {
  /** Index in `snippets`; `null` for a new snippet. */
  index: number | null;
  draft: Snippet;
}

const EMPTY: Snippet = { label: "", text: "", enter: true, target: "terminal" };

/**
 * Settings page "Snippets": the saved prompts and commands of `snippets[]` (#1007), listed and
 * edited one at a time. Every change writes only `snippets`.
 */
@Component({
  selector: "app-snippets-editor",
  imports: [HxButton, HxCheckbox, HxInput],
  templateUrl: "./snippets-editor.html",
  styleUrls: ["./settings-view.css", "./snippets-editor.css"],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SnippetsEditor {
  private readonly store = inject(SettingsStore);

  protected readonly snippets = computed(() => this.store.settings().snippets);
  protected readonly warnings = computed(() =>
    this.store.settings().warnings.filter((w) => w.startsWith("snippets")),
  );
  protected readonly writeError = this.store.writeError;
  protected readonly editing = signal<Editing | null>(null);

  /** Why the draft cannot be saved, per field; `null` when it can. */
  protected readonly errors = computed(() => {
    const e = this.editing();
    if (!e) return null;
    const label = e.draft.label.trim();
    const taken = this.snippets().some((s, i) => i !== e.index && s.label === label);
    return {
      label: label === "" ? "A label is required" : taken ? `Another snippet is "${label}"` : null,
      text: e.draft.text.trim() === "" ? "The text is required" : null,
    };
  });
  protected readonly valid = computed(() => {
    const errors = this.errors();
    return errors !== null && errors.label === null && errors.text === null;
  });

  protected add(): void {
    this.editing.set({ index: null, draft: { ...EMPTY } });
  }

  protected edit(index: number): void {
    this.editing.set({ index, draft: { ...this.snippets()[index] } });
  }

  protected cancel(): void {
    this.editing.set(null);
  }

  protected setLabel(event: Event): void {
    this.patch({ label: (event.target as HTMLInputElement).value });
  }

  protected setText(event: Event): void {
    this.patch({ text: (event.target as HTMLTextAreaElement).value });
  }

  protected setTarget(event: Event): void {
    const target = (event.target as HTMLSelectElement).value === "agent" ? "agent" : "terminal";
    this.patch({ target });
  }

  protected setEnter(event: Event): void {
    this.patch({ enter: (event.target as HTMLInputElement).checked });
  }

  protected async save(): Promise<void> {
    const e = this.editing();
    if (!e || !this.valid()) return;
    const list = [...this.snippets()];
    const draft: Snippet = { ...e.draft, label: e.draft.label.trim() };
    if (e.index === null) list.push(draft);
    else list[e.index] = draft;
    if (await this.store.setSnippets(list)) this.editing.set(null);
  }

  protected async remove(index: number): Promise<void> {
    await this.store.setSnippets(this.snippets().filter((_, i) => i !== index));
  }

  /** The row's second line: where it goes and whether it presses Enter. */
  protected summary(s: Snippet): string {
    if (s.target === "agent") return "agent prompt";
    return s.enter ? "terminal, with Enter" : "terminal, without Enter";
  }

  private patch(change: Partial<Snippet>): void {
    const e = this.editing();
    if (e) this.editing.set({ ...e, draft: { ...e.draft, ...change } });
  }
}
