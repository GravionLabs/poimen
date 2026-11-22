import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  Input,
  computed,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { HxButton, HxSelectButton } from "@gravionlabs/helix-ui";
import { ExternalEditor } from "../core/external-editor";
import { TauriBridge } from "../core/tauri";
import { renderMarkdown } from "../github/markdown";
import {
  fileName,
  isMarkdown,
  previewNotice,
  type FilePreviewData,
  type PreviewChanged,
} from "./preview-model";
import { SourceView } from "./source-view";

type ViewMode = "rendered" | "source";

/**
 * A read-only preview tab (`preview:<path>`, #1004) of one file: Markdown rendered through the
 * sanitizing renderer of the GitHub views with a toggle to the source, other text as highlighted
 * source. Binary files and files over 1 MiB show a notice. It follows the file: the backend
 * watches it (`preview_watch`) and a `file-preview` event reads it again. Never writes.
 */
@Component({
  selector: "app-file-preview",
  imports: [HxButton, HxSelectButton, SourceView],
  templateUrl: "./file-preview.html",
  styles: `
    :host {
      flex: 1;
      display: flex;
      flex-direction: column;
      min-height: 0;
      min-width: 0;
      background: var(--h-content-background);
    }
    header {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 4px 8px;
      border-bottom: 1px solid var(--h-content-border-color);
      font-size: 12px;
    }
    .file {
      min-width: 0;
      display: flex;
      gap: 8px;
      white-space: nowrap;
    }
    .name {
      flex: none;
      font-weight: 600;
    }
    .dir {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      color: var(--h-text-muted-color);
    }
    .spacer {
      flex: 1;
    }
    .body {
      flex: 1;
      min-height: 0;
      display: flex;
      flex-direction: column;
    }
    .body app-source-view {
      flex: 1;
    }
    .markdown {
      flex: 1;
      overflow: auto;
      padding: 12px 20px;
      overflow-wrap: anywhere;
      font-size: 13px;
      line-height: 1.5;
    }
    .markdown :is(img) {
      max-width: 100%;
    }
    .markdown :is(pre) {
      overflow: auto;
      padding: 8px;
      background: var(--term-bg);
      color: var(--term-fg);
    }
    .markdown :is(table) {
      border-collapse: collapse;
    }
    .markdown :is(th, td) {
      border: 1px solid var(--h-content-border-color);
      padding: 2px 8px;
    }
    .note {
      margin: auto;
      max-width: 420px;
      text-align: center;
      color: var(--h-text-muted-color);
    }
    .note.problem {
      color: var(--status-error);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FilePreview {
  private readonly bridge = inject(TauriBridge);
  private readonly editor = inject(ExternalEditor);

  protected readonly filePath = signal("");
  /** The absolute path of the file. A decorator input: the specs run without the compiler transform. */
  @Input({ required: true }) set path(value: string) {
    this.filePath.set(value);
  }

  protected readonly data = signal<FilePreviewData | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly view = signal<ViewMode>("rendered");
  protected readonly viewOptions = [
    { label: "Rendered", value: "rendered" },
    { label: "Source", value: "source" },
  ];

  protected readonly name = computed(() => fileName(this.filePath()));
  protected readonly dir = computed(() => this.filePath().slice(0, -this.name().length));
  protected readonly notice = computed(() => {
    const data = this.data();
    return data ? previewNotice(data) : null;
  });
  /** Markdown text: the toggle is offered. */
  protected readonly canToggle = computed(
    () => this.data()?.state === "text" && isMarkdown(this.filePath()),
  );
  protected readonly rendered = computed(() => this.canToggle() && this.view() === "rendered");
  protected readonly html = computed(() => {
    const data = this.data();
    return data?.state === "text" && this.rendered() ? renderMarkdown(data.text) : "";
  });

  private run = 0;

  constructor() {
    let unlisten: (() => void) | undefined;
    let destroyed = false;
    void this.bridge
      .listen<PreviewChanged>("file-preview", (event) => {
        if (event.path === this.filePath()) void this.read();
      })
      .then((stop) => (destroyed ? stop() : (unlisten = stop)));
    effect((onCleanup) => {
      const path = this.filePath();
      if (!path) return;
      untracked(() => {
        this.data.set(null);
        this.error.set(null);
        void this.bridge.invoke("preview_watch", { path });
        void this.read();
      });
      onCleanup(() => void this.bridge.invoke("preview_unwatch", { path }));
    });
    inject(DestroyRef).onDestroy(() => {
      destroyed = true;
      unlisten?.();
    });
  }

  private async read(): Promise<void> {
    const path = this.filePath();
    const run = ++this.run;
    try {
      const data = await this.bridge.invoke<FilePreviewData>("preview_read", { path });
      if (run !== this.run) return;
      this.data.set(data);
      this.error.set(null);
    } catch (e) {
      if (run !== this.run) return;
      this.data.set(null);
      this.error.set(e instanceof Error ? e.message : String(e));
    }
  }

  protected openInEditor(): void {
    void this.editor.open(this.filePath());
  }

  /** Links in the rendered text open in the browser, never inside the app. */
  @HostListener("click", ["$event"])
  protected onLink(event: MouseEvent): void {
    const link = (event.target as HTMLElement).closest<HTMLAnchorElement>(".markdown a");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href") ?? "";
    if (/^https?:/i.test(href)) void this.bridge.openUrl(href);
  }
}
