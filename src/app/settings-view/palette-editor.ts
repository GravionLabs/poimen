import { HxButton, HxCheckbox } from "@gravionlabs/helix-ui";
import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { PALETTE_SOURCES, type HiddenEntry } from "../core/palette-settings";
import { PaletteVisibility } from "../core/palette-visibility";
import { SettingsStore } from "../core/settings";

/**
 * Settings page "Palette" (#1218): which detected sources the palette shows and which single
 * detected entries the user hid per repository, with "Show" to undo.
 */
@Component({
  selector: "app-palette-editor",
  imports: [HxButton, HxCheckbox],
  templateUrl: "./palette-editor.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PaletteEditor {
  private readonly store = inject(SettingsStore);
  private readonly visibility = inject(PaletteVisibility);

  protected readonly sources = computed(() =>
    PALETTE_SOURCES.map((s) => ({ ...s, shown: this.store.settings().palette[s.id] })),
  );
  /** The repositories with hidden entries, by root. */
  protected readonly repositories = computed(() =>
    Object.entries(this.store.settings().palette.hidden).map(([root, entries]) => ({
      root,
      entries,
    })),
  );
  protected readonly writeError = this.store.writeError;

  protected toggle(source: "scripts" | "skills", event: Event): void {
    void this.visibility.setSource(source, (event.target as HTMLInputElement).checked);
  }

  protected show(root: string, entry: HiddenEntry): void {
    void this.visibility.setHidden(root, entry, false);
  }
}
