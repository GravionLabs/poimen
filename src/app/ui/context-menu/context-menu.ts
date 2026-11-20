import {
  ChangeDetectionStrategy,
  Component,
  DOCUMENT,
  ElementRef,
  EventEmitter,
  Injector,
  Input,
  OnChanges,
  OnDestroy,
  Output,
  afterNextRender,
  inject,
  signal,
} from "@angular/core";
import { clampMenu, menuTarget, type MenuGroup, type MenuPick } from "./menu-model";

/**
 * The one context menu of the app. The parent renders it (`@if`) where a menu should be open and
 * hands it the point (`at`) and the groups; it reports `picked` or `closed` and the parent closes it.
 *
 * Keyboard: the menu takes the focus when it opens, Arrow keys, Home/End and a first letter move
 * over the entries, Enter picks, Escape or Tab close, and the focus goes back to the element that
 * had it. A disabled entry stays focusable, shows as dimmed and gives its reason as the tooltip;
 * it picks nothing. The menu is kept inside the window, flipped left of `at` with `alignRight`.
 * Groups keep their separator and heading. Decorator inputs: the specs run without Angular's
 * compiler transform.
 */
@Component({
  selector: "app-context-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "(document:keydown.escape)": "close()" },
  templateUrl: "./context-menu.html",
  styles: `
    .menu-backdrop {
      all: unset;
      position: fixed;
      inset: 0;
      z-index: 50;
    }

    .menu {
      position: fixed;
      z-index: 51;
      box-sizing: border-box;
      min-width: 160px;
      max-width: min(420px, 100vw - 8px);
      max-height: calc(100vh - 8px);
      overflow-y: auto;
      padding: 4px;
      border: 1px solid var(--h-content-border-color);
      border-radius: 6px;
      background: var(--h-content-background);
      color: var(--h-text-color);
      box-shadow: 0 6px 20px rgb(0 0 0 / 25%);
      font-size: 12px;
      outline: none;
    }

    .menu-item {
      all: unset;
      box-sizing: border-box;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 24px;
      width: 100%;
      padding: 5px 10px;
      border-radius: 4px;
      font-size: 12px;
      cursor: pointer;
    }

    .menu-item:hover,
    .menu-item:focus-visible {
      background: color-mix(in srgb, currentColor 12%, transparent);
    }

    .menu-item[aria-disabled="true"] {
      opacity: 0.45;
      cursor: default;
    }

    .menu-item[aria-disabled="true"]:hover {
      background: transparent;
    }

    .menu-item[aria-disabled="true"]:focus-visible {
      background: color-mix(in srgb, currentColor 8%, transparent);
    }

    .menu-item.active {
      font-weight: 600;
    }

    .label {
      min-width: 0;
    }

    .shortcut {
      color: var(--h-text-muted-color);
    }

    .menu-separator {
      height: 1px;
      margin: 4px 0;
      background: var(--h-content-border-color);
    }

    .menu-heading {
      padding: 4px 10px 2px;
      font-size: 10px;
      letter-spacing: 0.04em;
      text-transform: uppercase;
      opacity: 0.6;
      cursor: default;
      user-select: none;
    }

    .menu-empty {
      padding: 5px 10px;
      opacity: 0.7;
    }

    .mark {
      flex: none;
      width: 8px;
      height: 8px;
      border-radius: 50%;
    }

    .attention-mark {
      background: var(--status-blocked);
    }

    .unread {
      background: var(--status-done);
    }
  `,
})
export class ContextMenu implements OnChanges, OnDestroy {
  /** The point the menu opens at (the pointer, or the edge of the button that opened it). */
  @Input({ required: true }) at!: { x: number; y: number };
  @Input({ required: true }) groups: readonly MenuGroup[] = [];
  /** Accessible name of the menu. */
  @Input() label: string | null = null;
  /** Extra class of the menu element. */
  @Input() cssClass = "";
  @Input() testId: string | null = null;
  /** The menu ends at `at.x` instead of starting there. */
  @Input() alignRight = false;
  /** Text shown when the menu has no entry. */
  @Input() empty = "";
  @Output() readonly picked = new EventEmitter<MenuPick>();
  @Output() readonly closed = new EventEmitter<void>();

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly injector = inject(Injector);
  private readonly doc = inject(DOCUMENT);
  private readonly opener: Element | null = this.doc.activeElement;
  private done = false;
  protected readonly pos = signal({ left: 0, top: 0 });

  constructor() {
    afterNextRender(
      () => {
        this.place();
        this.host.nativeElement.querySelector<HTMLElement>(".menu")?.focus();
      },
      { injector: this.injector },
    );
  }

  ngOnChanges(): void {
    this.pos.set({ left: this.at.x, top: this.at.y });
    // Entries may arrive or go while the menu is open: keep it inside the window.
    afterNextRender(() => this.place(), { injector: this.injector });
  }

  ngOnDestroy(): void {
    const opener = this.opener;
    if (opener instanceof HTMLElement && opener.isConnected && opener !== this.doc.body) {
      opener.focus({ preventScroll: true });
    }
  }

  protected hasItems(): boolean {
    return this.groups.some((g) => g.items.length > 0);
  }

  protected reason(disabled: boolean | string | undefined): string | null {
    return typeof disabled === "string" && disabled !== "" ? disabled : null;
  }

  protected pick(id: string, disabled: boolean, event: MouseEvent): void {
    // The parent closes the menu; a pick it ignores must not leave a menu that cannot be closed.
    if (disabled) return;
    this.picked.emit({ id, shiftKey: event.shiftKey });
  }

  protected close(): void {
    if (this.done) return;
    this.done = true;
    this.closed.emit();
  }

  protected key(event: KeyboardEvent, menu: HTMLElement): void {
    if (event.key === "Escape") {
      event.preventDefault();
      this.close();
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      this.close();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const items = [
      ...menu.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]'),
    ];
    const current = items.findIndex((e) => e === this.doc.activeElement);
    const to = menuTarget(
      items.map((e) => e.textContent?.trim() ?? ""),
      current,
      event.key,
    );
    if (to === null) return;
    event.preventDefault();
    items[to].focus();
  }

  private place(): void {
    const menu = this.host.nativeElement.querySelector<HTMLElement>(".menu");
    const view = this.doc.defaultView;
    if (!menu || !view) return;
    const box = menu.getBoundingClientRect();
    this.pos.set(
      clampMenu(
        this.at,
        { width: box.width, height: box.height },
        { width: view.innerWidth, height: view.innerHeight },
        this.alignRight,
      ),
    );
  }
}
