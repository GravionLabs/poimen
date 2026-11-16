import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  computed,
  effect,
  inject,
  signal,
} from "@angular/core";
import type { LayoutTab } from "../core/named-layouts";
import { hasLayoutErrors, layoutErrors, uniqueName } from "../core/named-layouts-edit";
import { TauriBridge } from "../core/tauri";
import { LayoutTabsEditor } from "../settings-view/layout-tabs-editor";
import type { FieldError } from "./config";
import {
  addPreset,
  changePreset,
  layoutOfTabs,
  presetLayout,
  presetNameError,
  presetsOfText,
  removePreset,
  replacePresetLayout,
  tabsOfLayout,
} from "./layout-config";

/** The tab a new layout starts with: one shell. */
const NEW_TABS: LayoutTab[] = [
  {
    name: "shell",
    root: { kind: "terminal", name: null, command: null, cwd: null },
    children: [],
    place: null,
  },
];

function parsed(source: string): unknown {
  try {
    return JSON.parse(source);
  } catch {
    return undefined;
  }
}

/**
 * The project file's `layout` and `presets[]` in the layout editor of the Layouts page (#611). The
 * JSON texts of the form stay the source of truth: every edit is emitted as the new text, so the
 * "Advanced" textareas of the space overview and this editor always show the same file. Lives in
 * its own chunk (the overview defers it).
 */
@Component({
  selector: "app-project-layouts",
  imports: [LayoutTabsEditor],
  templateUrl: "./project-layouts.html",
  styleUrl: "./space-overview.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ProjectLayouts {
  private readonly bridge = inject(TauriBridge);

  protected readonly layoutText = signal("");
  @Input() set layout(v: string) {
    this.layoutText.set(v);
  }
  protected readonly presetsText = signal("");
  @Input() set presets(v: string) {
    this.presetsText.set(v);
  }
  /** The backend's complaint about `layout` or `presets`, shown under the editor. */
  protected readonly layoutError = signal<string | null>(null);
  @Input() set layoutErr(v: string | null) {
    this.layoutError.set(v);
  }
  protected readonly presetsError = signal<string | null>(null);
  @Input() set presetsErr(v: string | null) {
    this.presetsError.set(v);
  }

  @Output() readonly layoutChange = new EventEmitter<string>();
  @Output() readonly presetsChange = new EventEmitter<string>();
  /** What blocks a save: tabs with errors, blank or repeated preset names. */
  @Output() readonly problemsChange = new EventEmitter<FieldError[]>();

  /** The project `layout` in the editor: its tabs, or `null` when it has none or cannot be shown. */
  protected readonly layoutTabs = computed(() => {
    const raw = parsed(this.layoutText());
    return raw === undefined ? null : tabsOfLayout(raw);
  });
  /** `none` without a layout, `editor`, or `advanced` (only the JSON text can be edited). */
  protected readonly layoutMode = computed<"none" | "editor" | "advanced">(() => {
    if (this.layoutText().trim() === "") return "none";
    return this.layoutTabs() ? "editor" : "advanced";
  });
  protected readonly layoutIssues = computed(() => {
    const tabs = this.layoutTabs();
    return tabs ? layoutErrors({ name: "layout", tabs }, []) : null;
  });
  protected readonly presetList = computed(() => presetsOfText(this.presetsText()));
  protected readonly presetIssues = computed(() =>
    (this.presetList() ?? []).map((p) =>
      p.tabs ? layoutErrors({ name: "layout", tabs: p.tabs }, []) : null,
    ),
  );
  /** Why the last export or import of a layout failed. */
  protected readonly transferError = signal<string | null>(null);

  constructor() {
    effect(() => {
      const problems: FieldError[] = [];
      const layout = this.layoutIssues();
      if (layout && hasLayoutErrors(layout)) {
        problems.push({ field: "layout", message: "Fix the tabs of the layout first." });
      }
      if (this.presetIssues().some((e) => e && hasLayoutErrors(e))) {
        problems.push({ field: "presets", message: "Fix the tabs of the presets first." });
      }
      const names = presetNameError((this.presetList() ?? []).map((p) => p.name));
      if (names) problems.push({ field: "presets", message: names });
      this.problemsChange.emit(problems);
    });
  }

  protected setLayoutTabs(tabs: LayoutTab[]): void {
    this.layoutChange.emit(JSON.stringify(layoutOfTabs(tabs), null, 2));
  }

  protected addLayout(): void {
    this.setLayoutTabs(structuredClone(NEW_TABS));
  }

  protected removeLayout(): void {
    this.layoutChange.emit("");
  }

  protected addPresetEntry(): void {
    const taken = (this.presetList() ?? []).map((p) => p.name);
    this.presetsChange.emit(addPreset(this.presetsText(), uniqueName("preset", taken)));
  }

  protected removePresetEntry(index: number): void {
    this.presetsChange.emit(removePreset(this.presetsText(), index));
  }

  protected renamePreset(index: number, event: Event): void {
    const name = (event.target as HTMLInputElement).value;
    this.presetsChange.emit(changePreset(this.presetsText(), index, { name }));
  }

  protected setPresetTabs(index: number, tabs: LayoutTab[]): void {
    this.presetsChange.emit(changePreset(this.presetsText(), index, { tabs }));
  }

  /** Writes the project `layout` (`target` -1) or preset `target` to a JSON file. */
  protected async exportLayout(target: number): Promise<void> {
    this.transferError.set(null);
    const layout =
      target < 0 ? parsed(this.layoutText()) : presetLayout(this.presetsText(), target);
    if (!layout) {
      this.transferError.set("There is no layout to export.");
      return;
    }
    const name = target < 0 ? "layout" : this.presetList()?.[target]?.name.trim() || "preset";
    try {
      const path = await this.bridge.pickSaveFile(`${name}.json`);
      if (path) await this.bridge.invoke("layout_export", { path, layout });
    } catch (e) {
      this.transferError.set(String(e));
    }
  }

  /** Replaces the project `layout` (`target` -1) or preset `target` by the layout of a JSON file. */
  protected async importLayout(target: number): Promise<void> {
    this.transferError.set(null);
    try {
      const path = await this.bridge.pickJsonFile();
      if (!path) return;
      const layout = await this.bridge.invoke<Record<string, unknown>>("layout_import", { path });
      if (target < 0) this.layoutChange.emit(JSON.stringify(layout, null, 2));
      else this.presetsChange.emit(replacePresetLayout(this.presetsText(), target, layout));
    } catch (e) {
      this.transferError.set(String(e));
    }
  }
}
