import { ChangeDetectionStrategy, Component, Input, computed, inject } from "@angular/core";
import {
  SettingsTransfer,
  hasChanges,
  planSummary,
  shortValue,
  type ImportMode,
  type ImportScope,
} from "../core/settings-transfer";

/**
 * The confirmation of a pending settings import of `scope`: the diff (added, changed, removed
 * keys), the invalid and skipped entries, merge or replace, Import and Cancel. Nothing is written
 * before Import.
 */
@Component({
  selector: "app-settings-import-panel",
  templateUrl: "./settings-import-panel.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsImportPanel {
  @Input() scope: ImportScope = "all";

  protected readonly transfer = inject(SettingsTransfer);
  protected readonly pending = computed(() => {
    const p = this.transfer.pending();
    return p && p.scope === this.scope ? p : null;
  });
  protected readonly modes: readonly { value: ImportMode; label: string }[] = [
    { value: "merge", label: "Merge" },
    { value: "replace", label: "Replace" },
  ];
  protected readonly hasChanges = hasChanges;
  protected readonly planSummary = planSummary;
  protected readonly shortValue = shortValue;

  protected setMode(mode: ImportMode): void {
    void this.transfer.setMode(mode);
  }

  protected apply(): void {
    void this.transfer.apply();
  }

  protected cancel(): void {
    this.transfer.cancel();
  }
}
