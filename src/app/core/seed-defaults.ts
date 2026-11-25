import type { TauriBridge } from "./tauri";
import type { UiState } from "./ui-state";

/**
 * The first start of this version writes the default layout and template into config.json when
 * it has neither key (`seed_space_defaults`). The UI state remembers that it ran; a failure is
 * retried at the next start.
 */
export async function seedSpaceDefaults(bridge: TauriBridge, ui: UiState): Promise<void> {
  if (ui.defaultsSeeded()) return;
  try {
    await bridge.invoke("seed_space_defaults");
    ui.markDefaultsSeeded();
  } catch {
    // retried at the next start
  }
}
