import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  effect,
  inject,
  signal,
  viewChildren,
} from "@angular/core";
import { Title } from "@angular/platform-browser";
import { HxTheme } from "@gravionlabs/helix-ui";
import type { Pane } from "./core/herdr";
import { HerdrStore } from "./core/herdr-store";
import { TauriBridge } from "./core/tauri";
import { Sidebar } from "./sidebar/sidebar";
import { TerminalView } from "./terminal-view/terminal-view";

/** Ids the native menu emits on the `menu` event (see `src-tauri/src/lib.rs`). */
type MenuAction = "expand_all" | "collapse_all" | "close_all" | "refresh";

/** The window: the sidebar and the list of open terminal cards. */
@Component({
  selector: "app-root",
  imports: [Sidebar, TerminalView],
  templateUrl: "./app.html",
  styleUrl: "./app.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly store = inject(HerdrStore);
  private readonly bridge = inject(TauriBridge);
  private readonly theme = inject(HxTheme);

  /** The opened panes in opening order, as they were when opened. */
  private readonly opened = signal<Pane[]>([]);
  /** The opened panes with their current herdr state (the last known one once herdr drops them). */
  protected readonly views = computed(() =>
    this.opened().map((pane) => this.store.panes().get(pane.pane_id) ?? pane),
  );
  protected readonly openIds = computed(() => new Set(this.opened().map((p) => p.pane_id)));
  private readonly cards = viewChildren(TerminalView);

  constructor() {
    const title = inject(Title);
    effect(() => {
      const waiting = this.store.waiting();
      title.setTitle(waiting > 0 ? `poimen (${waiting} waiting)` : "poimen");
    });

    const destroyRef = inject(DestroyRef);
    const menu = this.bridge.listen<MenuAction>("menu", (action) => this.onMenu(action));
    destroyRef.onDestroy(() => void menu.then((stop) => stop()));
    this.followSystemTheme(destroyRef);
    void this.store.start();
  }

  protected open(pane: Pane): void {
    const card = this.card(pane.pane_id);
    if (card) {
      card.expand();
      card.scrollIntoView();
      return;
    }
    this.opened.update((list) => [...list, pane]);
  }

  protected closed(paneId: string): void {
    this.opened.update((list) => list.filter((p) => p.pane_id !== paneId));
  }

  private card(paneId: string): TerminalView | undefined {
    return this.cards().find((card) => card.pane().pane_id === paneId);
  }

  private onMenu(action: MenuAction): void {
    switch (action) {
      case "expand_all":
        this.cards().forEach((card) => card.expand());
        break;
      case "collapse_all":
        this.cards().forEach((card) => card.collapse());
        break;
      case "close_all":
        [...this.cards()].forEach((card) => card.close());
        break;
      case "refresh":
        void this.store.refresh();
        break;
    }
  }

  /** HxTheme reads the system setting once; keep light/dark in step when it changes. */
  private followSystemTheme(destroyRef: DestroyRef): void {
    if (typeof matchMedia !== "function") return;
    const query = matchMedia("(prefers-color-scheme: dark)");
    const update = () => this.theme.setDark(query.matches);
    update();
    query.addEventListener("change", update);
    destroyRef.onDestroy(() => query.removeEventListener("change", update));
  }
}
