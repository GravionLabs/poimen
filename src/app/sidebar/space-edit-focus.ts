import { Directive, ElementRef, afterNextRender, inject } from "@angular/core";

/** Focuses and selects the inline rename input once it is in the DOM. */
@Directive({ selector: "input[appSpaceEditFocus]" })
export class SpaceEditFocus {
  constructor() {
    const el = inject<ElementRef<HTMLInputElement>>(ElementRef).nativeElement;
    afterNextRender(() => {
      el.focus();
      el.select();
    });
  }
}
