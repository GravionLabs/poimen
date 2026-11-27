import { Component } from "@angular/core";
import { HxButton, HxField, HxInput, HxMessage } from "@gravionlabs/helix-ui";
import type { Fixture } from "../../../fixtures/fixture";
import { DialogShell } from "./dialog-shell";

@Component({
  selector: "app-fixture-dialog-confirm",
  imports: [DialogShell, HxButton, HxField, HxInput],
  template: `
    <app-dialog-shell size="md" heading="Save layout">
      <p class="hint">The layout is stored in your settings and offered to new spaces.</p>
      <hx-field label="Name"><input hx-input size="small" value="review" /></hx-field>
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
  imports: [DialogShell, HxButton, HxMessage],
  template: `
    <app-dialog-shell size="sm" heading="Remove space" [alert]="true" [cancelDefault]="true">
      <p>Closing the space does not end its terminals.</p>
      <hx-message severity="error" size="small">The server could not be reached.</hx-message>
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
