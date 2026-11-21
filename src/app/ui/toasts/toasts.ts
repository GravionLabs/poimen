import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  afterRenderEffect,
  inject,
  viewChild,
} from "@angular/core";
import { HxToast } from "@gravionlabs/helix-ui";
import { MESSAGES } from "../../core/toast";

/**
 * The one `<hx-toast>` of the app (#1040). The stack sits in a manual popover, which is in the
 * top layer like a modal `<dialog>`: a toast shown while a dialog is open would be hidden behind
 * it with a plain z-index. Each new message moves the popover to the front again (the top layer
 * orders by when an element entered it), so a dialog opened earlier or later never covers it.
 */
@Component({
  selector: "app-toasts",
  imports: [HxToast],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./toasts.html",
  styles: `
    .layer {
      position: fixed;
      inset: 0 auto auto 0;
      width: 0;
      height: 0;
      margin: 0;
      padding: 0;
      border: 0;
      overflow: visible;
      background: transparent;
      pointer-events: none;
    }
  `,
})
export class Toasts {
  private readonly messages = inject(MESSAGES);
  private readonly layer = viewChild.required<ElementRef<HTMLElement>>("layer");

  constructor() {
    afterRenderEffect(() => {
      const count = this.messages.messages().length;
      const el = this.layer().nativeElement;
      if (typeof el.showPopover !== "function") return;
      if (count === 0) {
        if (el.matches(":popover-open")) el.hidePopover();
        return;
      }
      if (el.matches(":popover-open")) el.hidePopover();
      el.showPopover();
    });
  }
}
