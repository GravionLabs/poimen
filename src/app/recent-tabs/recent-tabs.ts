import {
  ChangeDetectionStrategy,
  Component,
  HostListener,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import {
  changesDir,
  EditorTabs,
  flowPane,
  previewPath,
  spaceOfTab,
  tabKind,
} from "../core/editor-tabs";
import { fileName } from "../file-preview/preview-model";
import { SpacesStore } from "../core/spaces-store";
import { paneTitle } from "../core/snapshot";
import { Keymap } from "../core/keymap";
import { parseChord } from "../core/shortcuts";
import { holdKey, stepIndex, switchList, touch } from "./recent-order";

interface Switching {
  ids: string[];
  index: number;
  /** `KeyboardEvent.key` of the held modifier; releasing it selects. */
  hold: string | null;
}

/**
 * Ctrl+Tab (#1006): cycles the tabs of the window in most-recently-used order while the modifier is
 * held, shows the order in a small list and selects on release. UI state only.
 */
@Component({
  selector: "app-recent-tabs",
  templateUrl: "./recent-tabs.html",
  styleUrl: "./recent-tabs.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RecentTabs {
  private readonly editor = inject(EditorTabs);
  private readonly keymap = inject(Keymap);
  private readonly store = inject(SpacesStore);

  private order: string[] = [];
  private readonly state = signal<Switching | null>(null);
  protected readonly list = computed(() => this.state()?.ids ?? null);
  protected readonly index = computed(() => this.state()?.index ?? 0);

  constructor() {
    effect(() => {
      const id = this.editor.activeId();
      if (id && untracked(() => this.state()) === null) this.order = touch(this.order, id);
    });
  }

  private titleOf(id: string): string {
    const pane = this.store.panes().get(id);
    return pane ? paneTitle(this.store.all(), pane) : id;
  }

  protected label(id: string): string {
    switch (tabKind(id)) {
      case "settings":
        return "Settings";
      case "github":
        return "GitHub";
      case "deliver":
        return "Runs";
      case "space":
        return `Space: ${spaceOfTab(id)}`;
      case "changes":
        return `Changes: ${changesDir(id)?.split("/").filter(Boolean).pop() ?? ""}`;
      case "preview":
        return `Preview: ${fileName(previewPath(id) ?? "")}`;
      case "flow":
        return `Flow: ${this.titleOf(flowPane(id) ?? id)}`;
      default:
        return this.titleOf(id);
    }
  }

  private openIds(): string[] {
    return this.editor.groups().flatMap((g) => g.tabs.map((t) => t.paneId));
  }

  @HostListener("window:keydown", ["$event"])
  protected onKeyDown(event: KeyboardEvent): void {
    const current = this.state();
    if (current && event.key === "Escape") {
      event.preventDefault();
      this.state.set(null);
      return;
    }
    const target = event.target instanceof Element ? event.target : null;
    const action = this.keymap.match(event, target?.closest(".xterm") != null);
    if (action !== "recentTab" && action !== "recentTabBack") return;
    event.preventDefault();
    const delta = action === "recentTab" ? 1 : -1;
    if (current) {
      this.state.set({ ...current, index: stepIndex(current.index, delta, current.ids.length) });
      return;
    }
    const ids = switchList(this.order, this.openIds());
    if (ids.length < 2) return;
    const chord = parseChord(this.keymap.shortcuts()[action], this.keymap.mac);
    const hold = chord ? holdKey(chord) : null;
    const index = stepIndex(0, delta, ids.length);
    if (hold === null) {
      // No modifier to release (a plain key): select at once.
      this.editor.activate(ids[index]);
      return;
    }
    this.state.set({ ids, index, hold });
  }

  @HostListener("window:keyup", ["$event"])
  protected onKeyUp(event: KeyboardEvent): void {
    const current = this.state();
    if (!current || event.key !== current.hold) return;
    this.state.set(null);
    const id = current.ids[current.index];
    if (this.openIds().includes(id)) this.editor.activate(id);
  }

  @HostListener("window:blur")
  protected cancel(): void {
    this.state.set(null);
  }
}
