import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { SettingsStore } from "../core/settings";
import {
  MAX_RATIO,
  MIN_RATIO,
  addPane,
  isValidLayout,
  layoutErrors,
  movePane,
  removePane,
  sameLayout,
  toConfig,
  updatePane,
  type PaneSpec,
  type SplitDirection,
} from "../core/space-layout";

/**
 * Settings section "New space layout": edits a draft of `spaces.layout` (or the built-in default)
 * and saves or resets it. The draft follows the setting until the user changes it.
 */
@Component({
  selector: "app-space-layout-editor",
  templateUrl: "./space-layout-editor.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SpaceLayoutEditor {
  private readonly store = inject(SettingsStore);

  protected readonly minRatio = MIN_RATIO;
  protected readonly maxRatio = MAX_RATIO;
  private readonly fallback = signal<PaneSpec[]>([]);
  /** The layout in effect: `spaces.layout`, else the built-in default. */
  protected readonly saved = computed<PaneSpec[]>(
    () => this.store.settings().spacesLayout?.panes ?? this.fallback(),
  );
  /** The layout is the default when unset or when it is the seeded default layout. */
  protected readonly isDefault = computed(() => {
    const layout = this.store.settings().spacesLayout;
    return layout === null || sameLayout(layout.panes, this.fallback());
  });
  protected readonly draft = signal<PaneSpec[]>([]);
  protected readonly errors = computed(() => layoutErrors(this.draft()));
  protected readonly dirty = computed(() => !sameLayout(this.draft(), this.saved()));
  protected readonly valid = computed(() => isValidLayout(this.draft()));

  constructor() {
    void this.store.defaultSpacesLayout().then((l) => this.fallback.set(l?.panes ?? []));
    // Follow the setting while there are no unsaved edits.
    let shown: PaneSpec[] = [];
    effect(() => {
      const saved = this.saved();
      untracked(() => {
        if (sameLayout(this.draft(), shown)) this.draft.set(saved);
        shown = saved;
      });
    });
  }

  /** Earlier panes a pane can split from. */
  protected earlier(index: number): string[] {
    return this.draft()
      .slice(0, index)
      .map((p) => p.name);
  }

  protected setName(index: number, event: Event): void {
    this.draft.update((d) => updatePane(d, index, { name: value(event) }));
  }

  protected setCommand(index: number, event: Event): void {
    const command = value(event);
    this.draft.update((d) => updatePane(d, index, { command: command === "" ? null : command }));
  }

  protected setFrom(index: number, event: Event): void {
    this.draft.update((d) => updatePane(d, index, { split: { from: value(event) } }));
  }

  protected setDirection(index: number, event: Event): void {
    const direction = value(event) as SplitDirection;
    this.draft.update((d) => updatePane(d, index, { split: { direction } }));
  }

  protected setRatio(index: number, event: Event): void {
    const ratio = Number(value(event));
    this.draft.update((d) => updatePane(d, index, { split: { ratio } }));
  }

  protected add(): void {
    this.draft.update(addPane);
  }

  protected remove(index: number): void {
    this.draft.update((d) => removePane(d, index));
  }

  protected move(index: number, delta: number): void {
    this.draft.update((d) => movePane(d, index, delta));
  }

  protected discard(): void {
    this.draft.set(this.saved());
  }

  protected async save(): Promise<void> {
    if (!this.valid()) return;
    const panes = this.draft();
    await this.store.setSpacesLayout({ panes }, toConfig(panes));
  }

  /** Writes the default layout back, so the setting is the default again. */
  protected async reset(): Promise<void> {
    const panes = this.fallback();
    if (panes.length === 0) return;
    if (await this.store.setSpacesLayout({ panes }, toConfig(panes))) this.draft.set(panes);
  }
}

function value(event: Event): string {
  return (event.target as HTMLInputElement | HTMLSelectElement).value;
}
