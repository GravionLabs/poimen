import { Injectable, inject } from "@angular/core";
import {
  paletteWire,
  withHidden,
  type HiddenEntry,
  type PaletteSettings,
} from "./palette-settings";
import { SettingsStore } from "./settings";

/** Writes `palette` of the user config: the sources shown and the hidden detected entries. */
@Injectable({ providedIn: "root" })
export class PaletteVisibility {
  private readonly settings = inject(SettingsStore);

  private write(next: PaletteSettings): Promise<boolean> {
    return this.settings.write({ palette: next }, "the palette", "set_palette", {
      palette: paletteWire(next),
    });
  }

  /** Shows or hides a whole detected source (`scripts`, `skills`). */
  setSource(source: "scripts" | "skills", shown: boolean): Promise<boolean> {
    return this.write({ ...this.settings.settings().palette, [source]: shown });
  }

  /** Hides (or shows again) `entry` for the repository `root`. */
  setHidden(root: string, entry: HiddenEntry, hidden: boolean): Promise<boolean> {
    return this.write(withHidden(this.settings.settings().palette, root, entry, hidden));
  }
}
