import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { ChipPicker } from "../chip-picker/chip-picker";
import { GithubIssues, formReady } from "../issues";
import { renderMarkdown } from "../markdown";
import { HxButton, HxSelectButton, HxField, HxInput } from "@gravionlabs/helix-ui";

/**
 * The form for a new issue (#341): title, Markdown body, labels and milestone. Opened by the "+" of
 * a repository group or "Add sub-issue" in the issue pane; the level label and title prefix are
 * preset from the parent and stay editable. State lives in `GithubIssues.form`.
 */
@Component({
  selector: "app-issue-form",
  templateUrl: "./issue-form.html",
  styleUrl: "./issue-form.css",
  imports: [HxButton, HxSelectButton, ChipPicker, HxField, HxInput],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class IssueFormPane {
  protected readonly viewModes = [
    { label: "Write", value: "write" },
    { label: "Preview", value: "preview" },
  ];
  protected readonly issues = inject(GithubIssues);
  protected readonly preview = computed(() => renderMarkdown(this.issues.form()?.body ?? ""));
  protected readonly ready = computed(() => {
    const f = this.issues.form();
    return f !== null && formReady(f);
  });

  protected addLabel(name: string): void {
    const f = this.issues.form();
    if (f) this.issues.patchForm({ labels: [...f.labels, name] });
  }

  protected removeLabel(name: string): void {
    const f = this.issues.form();
    if (f) this.issues.patchForm({ labels: f.labels.filter((l) => l !== name) });
  }

  protected value(event: Event): string {
    return (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  }
}
