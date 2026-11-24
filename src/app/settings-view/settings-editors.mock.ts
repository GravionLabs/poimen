import { readFileSync } from "node:fs";
import type { Type } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { LayoutTabsEditor } from "./layout-tabs-editor";
import { LayoutsEditor } from "./layouts-editor";
import { PaletteEditor } from "./palette-editor";
import { SettingsImportPanel } from "./settings-import-panel";
import { ShortcutsEditor } from "./shortcuts-editor";
import { NotificationsPage } from "./notifications-page";
import { SnippetsEditor } from "./snippets-editor";
import { SpaceTemplatesEditor } from "./space-templates-editor";

/** The child pages of the settings view with their template file, for specs that render them. */
export const EDITORS: [Type<unknown>, string][] = [
  [LayoutTabsEditor, "layout-tabs-editor"],
  [LayoutsEditor, "layouts-editor"],
  [SettingsImportPanel, "settings-import-panel"],
  [SpaceTemplatesEditor, "space-templates-editor"],
  [SnippetsEditor, "snippets-editor"],
  [NotificationsPage, "notifications-page"],
  [PaletteEditor, "palette-editor"],
  [ShortcutsEditor, "shortcuts-editor"],
];

/** Gives every child page of the settings view its template file (the specs run without a compiler transform). */
export function overrideEditors(): void {
  for (const [component, name] of EDITORS) {
    TestBed.overrideComponent(component, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        styleUrls: undefined,
        template: readFileSync(`src/app/settings-view/${name}.html`, "utf8"),
      },
    });
  }
}
