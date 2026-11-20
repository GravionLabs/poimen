import { Component } from "@angular/core";

/** Lint probe (#1051): an inline template outside a spec file fails `poimen/no-inline-template`. */
@Component({
  selector: "app-inline-template-probe",
  template: `<p>inline</p>`,
})
export class InlineTemplateProbe {}
