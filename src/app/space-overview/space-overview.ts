import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  Input,
  computed,
  effect,
  inject,
  output,
  signal,
} from "@angular/core";
import { SpaceActions, type TrustRequest } from "../core/space-actions";
import { EditorTabs } from "../core/editor-tabs";
import { SpacesStore } from "../core/spaces-store";
import { PaneActions } from "../core/pane-actions";
import { TauriBridge } from "../core/tauri";
import { isGithubUrl } from "../github/pulls";
import {
  EMPTY_FORM,
  formOf,
  patchOf,
  type ConfigForm,
  type FieldError,
  type ProjectFile,
  type SaveOutcome,
} from "./config";
import { activityText, overviewRows, spaceDirectory, spacePullUrl } from "./overview";

/**
 * The overview of one space as an editor tab: its panes with kind, label, session status and
 * actions, plus the space actions. Everything is read from the store, so it follows the backend.
 */
@Component({
  selector: "app-space-overview",
  templateUrl: "./space-overview.html",
  styleUrl: "./space-overview.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SpaceOverview {
  private readonly spaceId = signal("");
  @Input({ required: true }) set space(id: string) {
    this.spaceId.set(id);
  }

  /** Bumped by the host to scroll to the configuration section (gear icon). */
  private readonly nonce = signal(0);
  @Input() set configNonce(n: number) {
    this.nonce.set(n);
  }

  /** Show the Output panel for a pane. */
  readonly outputRequested = output<string>();
  /** Show a directory in the explorer. */
  readonly explorerRequested = output<string>();
  /** Delete (close) the space. */
  readonly deleteRequested = output<string>();

  private readonly store = inject(SpacesStore);
  private readonly editor = inject(EditorTabs);
  private readonly paneActions = inject(PaneActions);
  private readonly bridge = inject(TauriBridge);
  private readonly spaceActions = inject(SpaceActions);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly node = computed(() =>
    this.store.spaces().find((s) => s.workspace.space_id === this.spaceId()),
  );
  protected readonly rows = computed(() => {
    const n = this.node();
    return n ? overviewRows(n) : [];
  });
  protected readonly pullUrl = computed(() => {
    const n = this.node();
    return n ? spacePullUrl(n) : undefined;
  });
  protected readonly directory = computed(() => {
    const n = this.node();
    return n ? spaceDirectory(n) : undefined;
  });

  protected readonly form = signal<ConfigForm>({ ...EMPTY_FORM });
  protected readonly name = signal<string | null>(null);
  protected readonly nameError = signal<string | null>(null);
  protected readonly fileError = signal<string | null>(null);
  protected readonly fieldErrors = signal<FieldError[]>([]);
  protected readonly trust = signal<TrustRequest | null>(null);
  protected readonly status = signal<string | null>(null);
  protected readonly fileExists = signal(false);

  constructor() {
    effect(() => {
      const dir = this.directory();
      if (dir) void this.load(dir);
    });
    effect(() => {
      if (this.nonce() > 0 && this.node()) {
        queueMicrotask(() =>
          this.host.nativeElement.querySelector("#config")?.scrollIntoView?.({ block: "start" }),
        );
      }
    });
  }

  protected errorOf(field: string): string | undefined {
    return this.fieldErrors().find((e) => e.field === field)?.message;
  }

  protected setName(event: Event): void {
    this.name.set((event.target as HTMLInputElement).value);
  }

  protected setField(field: keyof ConfigForm, event: Event): void {
    const value = (event.target as HTMLInputElement | HTMLTextAreaElement).value;
    this.form.update((f) => ({ ...f, [field]: value }));
    this.status.set(null);
  }

  private async load(dir: string): Promise<void> {
    try {
      const file = await this.bridge.invoke<ProjectFile>("project_file", { dir });
      const { form, error } = formOf(file.text);
      this.form.set(form);
      this.fileExists.set(file.exists);
      this.fileError.set(error);
      this.fieldErrors.set([]);
      this.trust.set(null);
    } catch (e) {
      this.fileError.set(String(e));
    }
  }

  protected async renameSpace(): Promise<void> {
    const n = this.node();
    const next = this.name() ?? n?.workspace.label ?? "";
    if (!n) return;
    this.nameError.set(null);
    const ok = await this.spaceActions.rename(n.workspace.space_id, next);
    if (!ok) this.nameError.set(this.spaceActions.renameError() ?? "The rename failed.");
    else this.name.set(null);
  }

  /** Saves the project file; `trusted` is the answer to an open trust question. */
  protected async save(trusted = false): Promise<void> {
    const dir = this.directory();
    if (!dir) return;
    const { patch, errors } = patchOf(this.form());
    this.fieldErrors.set(errors);
    this.status.set(null);
    if (errors.length > 0) return;
    const request = this.trust();
    const decision = trusted && request ? { kind: "trust", hash: request.hash } : undefined;
    try {
      const outcome = await this.bridge.invoke<SaveOutcome>("project_save", {
        dir,
        patch,
        decision,
      });
      this.fieldErrors.set(outcome.errors);
      this.trust.set(outcome.trust);
      if (outcome.saved) {
        this.fileExists.set(true);
        this.status.set("Saved to .poimen/config.json.");
        await this.load(dir);
      }
    } catch (e) {
      this.status.set(String(e));
    }
  }

  protected cancelTrust(): void {
    this.trust.set(null);
  }

  protected activity(at: number | undefined): string {
    return activityText(at, Date.now());
  }

  protected focus(paneId: string): void {
    this.editor.open(paneId, true);
  }
  protected flow(paneId: string): void {
    this.editor.openFlow(paneId);
  }
  protected rename(paneId: string): void {
    this.paneActions.askRename(paneId);
  }
  protected close(paneId: string): void {
    this.paneActions.askClose(paneId);
  }
  /** The backend has no restart: starting an agent in the pane is the closest existing action. */
  protected restart(paneId: string): void {
    this.paneActions.askStartAgent(paneId);
  }
  protected newPane(): void {
    this.paneActions.askNew(this.spaceId());
  }
  protected openPull(): void {
    const url = this.pullUrl();
    if (url && isGithubUrl(url)) void this.bridge.openUrl(url);
  }
}
