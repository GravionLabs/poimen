import { Component } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import type { Fixture } from "../../../fixtures/fixture";
import { DialogShell } from "./dialog-shell";

@Component({
  selector: "app-fixture-dialog-confirm",
  imports: [DialogShell, HxButton],
  template: `
    <app-dialog-shell heading="Save layout">
      <p class="hint">The layout is stored in your settings and offered to new spaces.</p>
      <label>Name <input value="review" /></label>
      <div class="dialog-buttons">
        <button hx-button variant="text" severity="secondary" size="small">Cancel</button>
        <button hx-button size="small">Save</button>
      </div>
    </app-dialog-shell>
  `,
})
class ConfirmHost {}

@Component({
  selector: "app-fixture-dialog-danger",
  imports: [DialogShell, HxButton],
  template: `
    <app-dialog-shell heading="Remove space" [alert]="true" [cancelDefault]="true">
      <p>Closing the space does not end its terminals.</p>
      <p class="error">The server could not be reached.</p>
      <div class="dialog-buttons">
        <button hx-button variant="outlined" severity="secondary" size="small">Cancel</button>
        <button hx-button severity="danger" size="small">Remove</button>
      </div>
    </app-dialog-shell>
  `,
})
class DangerHost {}

export const DIALOG_SHELL_FIXTURES: Fixture[] = [
  {
    id: "dialog-shell/confirm",
    title: "Dialog with a field and a primary action",
    component: ConfirmHost,
  },
  {
    id: "dialog-shell/danger",
    title: "Destructive confirmation with an error",
    component: DangerHost,
  },
];
