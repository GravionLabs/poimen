import { HxButton, HxMessage } from "@gravionlabs/helix-ui";
import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  inject,
  signal,
} from "@angular/core";
import { Keymap } from "../core/keymap";
import { SettingsStore } from "../core/settings";
import { recordAccelerator, shortcutConflict } from "../core/shortcut-record";
import { SHORTCUT_GROUPS, shortcutTitle } from "../core/shortcuts-overview";
import { DEFAULT_SHORTCUTS, formatShortcut, type ShortcutAction } from "../core/shortcuts";

interface Recording {
  action: ShortcutAction;
  /** The combination taken so far; `null` until a key is pressed. */
  accelerator: string | null;
  /** Why the last key press was not taken. */
  error: string | null;
}

/**
 * Settings page "Keyboard" (#1009): every action with its shortcut. "Record" takes the next key
 * combination, shows a conflict with another action before saving, and writes only that entry of
 * `shortcuts`; Reset restores the default, Clear unbinds.
 */
@Component({
  selector: "app-shortcuts-editor",
  imports: [HxButton, HxMessage],
  templateUrl: "./shortcuts-editor.html",
  styleUrls: ["./settings-view.css", "./shortcuts-editor.css"],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ShortcutsEditor {
  private readonly store = inject(SettingsStore);
  private readonly keymap = inject(Keymap);

  protected readonly groups = SHORTCUT_GROUPS;
  protected readonly title = shortcutTitle;
  protected readonly writeError = this.store.writeError;
  protected readonly resetError = signal<string | null>(null);
  protected readonly recording = signal<Recording | null>(null);

  /** The action that already has the recorded combination. */
  protected readonly conflict = computed(() => {
    const r = this.recording();
    if (!r?.accelerator) return null;
    return shortcutConflict(this.keymap.shortcuts(), r.action, r.accelerator);
  });

  /** Shown while recording; the capture listener stops the keys from reaching the app. */
  private readonly capture = (event: KeyboardEvent): void => {
    const r = this.recording();
    if (!r) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && !(event.metaKey || event.ctrlKey || event.altKey)) {
      this.cancel();
      return;
    }
    const got = recordAccelerator(event, this.keymap.mac);
    if (got === null) return;
    this.recording.set(
      "accelerator" in got
        ? { ...r, accelerator: got.accelerator, error: null }
        : { ...r, accelerator: null, error: got.error },
    );
  };

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stopListening());
  }

  protected label(action: ShortcutAction): string {
    return this.keymap.label(action);
  }

  protected display(accelerator: string): string {
    return formatShortcut(accelerator, this.keymap.mac);
  }

  protected isDefault(action: ShortcutAction): boolean {
    return this.keymap.shortcuts()[action] === DEFAULT_SHORTCUTS[action];
  }

  protected record(action: ShortcutAction): void {
    this.stopListening();
    this.recording.set({ action, accelerator: null, error: null });
    window.addEventListener("keydown", this.capture, true);
  }

  protected cancel(): void {
    this.stopListening();
    this.recording.set(null);
  }

  /** Writes only `shortcuts.<action>`: `null` resets it, `""` unbinds it. */
  private set(action: ShortcutAction, accelerator: string | null): Promise<boolean> {
    const shortcuts = { ...this.keymap.shortcuts() };
    shortcuts[action] = accelerator ?? DEFAULT_SHORTCUTS[action];
    return this.store.write({ shortcuts }, "the shortcut", "set_shortcut", { action, accelerator });
  }

  protected async save(): Promise<void> {
    const r = this.recording();
    if (!r?.accelerator || this.conflict()) return;
    this.cancel();
    await this.set(r.action, r.accelerator);
  }

  protected async clear(action: ShortcutAction): Promise<void> {
    this.cancel();
    await this.set(action, "");
  }

  protected async reset(action: ShortcutAction): Promise<void> {
    this.cancel();
    const other = shortcutConflict(this.keymap.shortcuts(), action, DEFAULT_SHORTCUTS[action]);
    if (other) {
      this.resetError.set(`${this.title(action)}: the default is used by ${this.title(other)}`);
      return;
    }
    this.resetError.set(null);
    await this.set(action, null);
  }

  private stopListening(): void {
    window.removeEventListener("keydown", this.capture, true);
  }
}
