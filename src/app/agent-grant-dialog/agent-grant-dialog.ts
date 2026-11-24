import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { AgentGrants, durationText, type GrantRequest } from "../core/agent-grants";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

/** What the user changed in the request on screen: spaces left out and a lower limit. */
interface Edit {
  id: string;
  left: ReadonlySet<string>;
  limit: number | null;
}

/**
 * A lead agent asks for a scoped write grant (ADR 0018, #1211): who asks, the plan, each space with
 * its path, the tools, the limit and the end. The user can only deny or narrow it (uncheck spaces,
 * lower the limit); Cancel has the focus, Escape and the backdrop deny.
 */
@Component({
  selector: "app-agent-grant-dialog",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-grant-dialog.html",
  styleUrl: "./agent-grant-dialog.css",
})
export class AgentGrantDialog {
  protected readonly grants = inject(AgentGrants);
  private readonly edit = signal<Edit | null>(null);
  protected readonly duration = durationText;

  private editOf(request: GrantRequest): Edit {
    const edit = this.edit();
    return edit?.id === request.id ? edit : { id: request.id, left: new Set(), limit: null };
  }

  protected checked(request: GrantRequest, spaceId: string): boolean {
    return !this.editOf(request).left.has(spaceId);
  }

  protected chosen(request: GrantRequest): string[] {
    const { left } = this.editOf(request);
    return request.spaces.map((s) => s.id).filter((id) => !left.has(id));
  }

  protected limit(request: GrantRequest): number {
    return Math.min(this.editOf(request).limit ?? request.maxAgents, request.maxAgents);
  }

  protected toggle(request: GrantRequest, spaceId: string): void {
    const edit = this.editOf(request);
    const left = new Set(edit.left);
    if (!left.delete(spaceId)) left.add(spaceId);
    this.edit.set({ ...edit, left });
  }

  protected setLimit(request: GrantRequest, value: string): void {
    const n = Math.floor(Number(value));
    if (!Number.isFinite(n) || n < 1) return;
    this.edit.set({ ...this.editOf(request), limit: Math.min(n, request.maxAgents) });
  }

  protected allow(request: GrantRequest): void {
    const spaces = this.chosen(request);
    if (spaces.length === 0) return;
    void this.grants.answer(
      true,
      spaces.length === request.spaces.length ? undefined : spaces,
      this.limit(request) === request.maxAgents ? undefined : this.limit(request),
    );
  }
}
