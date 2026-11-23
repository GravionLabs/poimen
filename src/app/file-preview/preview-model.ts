/** What `preview_read` (`src-tauri/src/file_preview.rs`) sends for one file. */
export type FilePreviewData =
  | { state: "text"; text: string; size: number }
  | { state: "binary"; size: number }
  | { state: "too_large"; size: number };

/** The `file-preview` event: the file at `path` changed. */
export interface PreviewChanged {
  path: string;
}

/** Whether the file is rendered as Markdown (with a toggle to the source). */
export function isMarkdown(path: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(path);
}

/** The file name of a path with `/` or `\` separators. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** `1536` as `1.5 KiB`. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

/** The notice for a file that is not shown, `null` for a text file. */
export function previewNotice(data: FilePreviewData): string | null {
  switch (data.state) {
    case "binary":
      return `Binary file (${formatSize(data.size)}): there is no preview. Open it in the editor.`;
    case "too_large":
      return `Too large to preview (${formatSize(data.size)}, the limit is 1 MiB). Open it in the editor.`;
    default:
      return null;
  }
}
