import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { SettingsStore } from "../core/settings";
import {
  duplicateLayout,
  emptyLayout,
  flatTabs,
  hasLayoutErrors,
  layoutErrors,
  uniqueName,
  usedMessage,
  type LayoutTab,
  type NamedLayout,
} from "../core/named-layouts-edit";
import { LayoutTabsEditor } from "./layout-tabs-editor";

interface Editing {
  /** Index in `layouts`; `null` for a new layout. */
  index: number | null;
  draft: NamedLayout;
}

function value(event: Event): string {
  return (event.target as HTMLInputElement | HTMLSelectElement).value;
}

/**
 * Settings page "Layouts": the named layouts of `layouts[]` (ADR 0011), listed and edited one at a
 * time: tabs with nested child tabs, per tab one terminal or a split tree, and where the tab goes
 * in the editor groups of the app. Every change writes only `layouts`.
 */
@Component({
  selector: "app-layouts-editor",
  imports: [LayoutTabsEditor],
  templateUrl: "./layouts-editor.html",
  styleUrls: ["./settings-view.css", "./layouts-editor.css"],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LayoutsEditor {
  private readonly store = inject(SettingsStore);

  protected readonly writeError = this.store.writeError;
  protected readonly layouts = computed(() => this.store.settings().layouts);
  protected readonly templates = computed(() => this.store.settings().spaceTemplates);
  protected readonly warnings = computed(() =>
    this.store.settings().warnings.filter((w) => w.startsWith("layouts")),
  );
  protected readonly editing = signal<Editing | null>(null);
  protected readonly renaming = signal<{ index: number; name: string } | null>(null);
  /** Why the last delete or rename was refused. */
  protected readonly refusal = signal<string | null>(null);

  protected readonly errors = computed(() => {
    const e = this.editing();
    if (!e) return null;
    const others = this.layouts()
      .filter((_, i) => i !== e.index)
      .map((l) => l.name);
    return layoutErrors(e.draft, others);
  });
  protected readonly valid = computed(() => {
    const e = this.errors();
    return e !== null && !hasLayoutErrors(e);
  });

  /** The tabs of the layout in words for the list: `agent · tests`. */
  protected summary(l: NamedLayout): string {
    return flatTabs(l.tabs)
      .map((t) => t.tab.name)
      .join(" · ");
  }

  /** The templates that use the layout, for the list. */
  protected usedBy(name: string): string[] {
    return this.templates()
      .filter((t) => t.layout === name)
      .map((t) => t.name);
  }

  protected add(): void {
    this.refusal.set(null);
    const name = uniqueName(
      "layout",
      this.layouts().map((l) => l.name),
    );
    this.editing.set({ index: null, draft: emptyLayout(name) });
  }

  protected edit(index: number): void {
    this.refusal.set(null);
    this.editing.set({ index, draft: structuredClone(this.layouts()[index]) });
  }

  protected cancel(): void {
    this.editing.set(null);
  }

  protected async save(): Promise<void> {
    const e = this.editing();
    if (!e || !this.valid()) return;
    const list = [...this.layouts()];
    const draft: NamedLayout = { ...e.draft, name: e.draft.name.trim() };
    if (e.index === null) list.push(draft);
    else {
      const old = list[e.index].name;
      const refused = old === draft.name ? null : usedMessage(old, this.templates(), "rename");
      if (refused) {
        this.refusal.set(refused);
        return;
      }
      list[e.index] = draft;
    }
    this.refusal.set(null);
    if (await this.store.setLayouts(list)) this.editing.set(null);
  }

  protected async duplicate(index: number): Promise<void> {
    this.refusal.set(null);
    const source = this.layouts()[index];
    const name = uniqueName(
      `${source.name} copy`,
      this.layouts().map((l) => l.name),
    );
    await this.store.setLayouts([...this.layouts(), duplicateLayout(source, name)]);
  }

  /** Deletes the layout unless a template refers to it; the refusal names the templates. */
  protected async remove(index: number): Promise<void> {
    const name = this.layouts()[index].name;
    const refused = usedMessage(name, this.templates(), "delete");
    this.refusal.set(refused);
    if (refused) return;
    await this.store.setLayouts(this.layouts().filter((_, i) => i !== index));
  }

  protected startRename(index: number): void {
    this.refusal.set(null);
    this.renaming.set({ index, name: this.layouts()[index].name });
  }

  protected renameInput(event: Event): void {
    const r = this.renaming();
    if (r) this.renaming.set({ ...r, name: value(event) });
  }

  protected cancelRename(): void {
    this.renaming.set(null);
  }

  protected renameError(): string | null {
    const r = this.renaming();
    if (!r) return null;
    const name = r.name.trim();
    if (!name) return "A name is required";
    const taken = this.layouts().some((l, i) => i !== r.index && l.name === name);
    return taken ? `Another layout is called "${name}"` : null;
  }

  protected async confirmRename(): Promise<void> {
    const r = this.renaming();
    if (!r || this.renameError()) return;
    const old = this.layouts()[r.index].name;
    const name = r.name.trim();
    if (name === old) {
      this.renaming.set(null);
      return;
    }
    const refused = usedMessage(old, this.templates(), "rename");
    this.refusal.set(refused);
    if (refused) return;
    const list = this.layouts().map((l, i) => (i === r.index ? { ...l, name } : l));
    if (await this.store.setLayouts(list)) this.renaming.set(null);
  }

  private change(edit: (d: NamedLayout) => NamedLayout): void {
    const e = this.editing();
    if (e) this.editing.set({ ...e, draft: edit(e.draft) });
  }

  protected setName(event: Event): void {
    this.change((d) => ({ ...d, name: value(event) }));
  }

  protected setTabs(tabs: LayoutTab[]): void {
    this.change((d) => ({ ...d, tabs }));
  }
}
