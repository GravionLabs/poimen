import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  OnDestroy,
  afterNextRender,
  EventEmitter,
  Input,
  Output,
  ViewChild,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { openedByKeyboard, returnFocus } from "../focus-return";
import { Icon } from "../icon/icon";

const FIELDS =
  "input:not([type=hidden]):not([disabled]), select:not([disabled]), textarea:not([disabled])";
const FOCUSABLE = `${FIELDS}, button:not([disabled]):not([tabindex="-1"]), a[href], [tabindex]:not([tabindex="-1"])`;

/**
 * The one shell of the app's dialogs: a native modal `<dialog>` (the page behind is inert), a heading
 * with the same close icon button everywhere, and the content of the dialog projected. The buttons of
 * the dialog are a `.dialog-buttons` element in that content (styles in `styles.css`): Cancel first,
 * the confirming action last, as the one primary (or danger) button.
 *
 * Keyboard, without a click first: opening moves the focus to the first field, else to the safe
 * default button (the confirming one; Cancel when `cancelDefault`), Tab stays inside, Escape and the
 * backdrop emit `cancelled`, Enter outside a form emits `confirmed` (never when Cancel is the default), and
 * closing gives the focus back to the element that opened the dialog. A key the content handled
 * itself (`preventDefault`) is left alone.
 */
@Component({
  selector: "app-dialog-shell",
  host: { "(keydown)": "key($event)", "(click)": "backdrop($event)" },
  imports: [HxButton, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./dialog-shell.html",
  styles: `
    .dialog {
      max-height: 80vh;
      margin: 20vh auto auto;
      padding: 0;
      border: 1px solid var(--h-content-border-color);
      border-radius: 8px;
      background: var(--h-content-background);
      color: var(--h-text-color);
      box-shadow: 0 10px 30px rgb(0 0 0 / 30%);
      font-size: 13px;
      overflow: auto;
    }

    .dialog::backdrop {
      background: rgb(0 0 0 / 35%);
    }

    .inner {
      padding: 16px;
    }

    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      margin: 0 0 10px;
    }

    h2 {
      min-width: 0;
      margin: 0;
      font-size: 14px;
      overflow-wrap: anywhere;
    }
  `,
})
export class DialogShell implements OnDestroy {
  /** The visible title and the accessible name of the dialog. */
  @Input() heading = "";
  /** The accessible name when the heading is richer than a string; defaults to `heading`. */
  @Input() label: string | null = null;
  /** `alertdialog` for a question the user has to answer. */
  @Input() alert = false;
  /** A destructive or risky confirmation: Cancel is the default and Enter does not confirm. */
  @Input() cancelDefault = false;
  /** Width of the dialog, a CSS length. */
  @Input() width = "480px";
  /** Escape, the backdrop or the close icon. */
  @Output() readonly cancelled = new EventEmitter<void>();
  /** Enter outside a form and a button. */
  @Output() readonly confirmed = new EventEmitter<void>();

  @ViewChild("dialog") private readonly dialogRef?: ElementRef<HTMLDialogElement>;
  private readonly opener =
    typeof document === "undefined" ? null : (document.activeElement as HTMLElement | null);

  private readonly byKeyboard = openedByKeyboard();

  constructor() {
    afterNextRender(() => {
      const el = this.dialogRef?.nativeElement;
      if (!el) return;
      el.addEventListener("cancel", (event) => this.nativeCancel(event));
      if (typeof el.showModal === "function") el.showModal();
      else el.setAttribute("open", "");
      this.focusDefault(el);
    });
  }

  ngOnDestroy(): void {
    const el = this.dialogRef?.nativeElement;
    if (el && typeof el.close === "function") el.close();
    else el?.removeAttribute("open");
    if (this.opener?.isConnected) returnFocus(this.opener, this.byKeyboard);
  }

  protected backdrop(event: MouseEvent): void {
    if (event.target === this.dialogRef?.nativeElement) this.cancelled.emit();
  }

  /** The native Escape of a browser when the content did not handle the key. */
  protected nativeCancel(event: Event): void {
    event.preventDefault();
    this.cancelled.emit();
  }

  protected key(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      this.cancelled.emit();
    } else if (event.key === "Tab") {
      this.trap(event);
    } else if (event.key === "Enter" && !this.cancelDefault) {
      const target = event.target as HTMLElement;
      if (target.closest("button, textarea, a, select, form, [role=option]")) return;
      event.preventDefault();
      this.confirmed.emit();
    }
  }

  private trap(event: KeyboardEvent): void {
    const el = this.dialogRef?.nativeElement;
    if (!el) return;
    const items = Array.from(el.querySelectorAll<HTMLElement>(FOCUSABLE));
    if (items.length === 0) {
      event.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !el.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !el.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  private focusDefault(el: HTMLDialogElement): void {
    const body = el.querySelector<HTMLElement>(".body");
    const wanted =
      body?.querySelector<HTMLElement>("[data-dialog-focus]") ??
      body?.querySelector<HTMLElement>(FIELDS) ??
      this.defaultButton(el);
    (wanted ?? el.querySelector<HTMLElement>(".dialog-close"))?.focus();
  }

  private defaultButton(el: HTMLDialogElement): HTMLElement | null {
    const buttons = Array.from(
      el.querySelectorAll<HTMLButtonElement>(".dialog-buttons button:not([disabled])"),
    );
    return (this.cancelDefault ? buttons[0] : buttons[buttons.length - 1]) ?? null;
  }
}
