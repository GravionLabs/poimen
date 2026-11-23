import type { TauriBridge } from "../core/tauri";
import { isWindows } from "../core/settings";
import { imagePasteRefusal, quotePaths, savableImage } from "./drop";

/** What a pasted image needs from its view. */
export interface ImagePaste {
  tauri: TauriBridge;
  notify: (text: string) => void;
  remote: boolean;
  /** Whether the view still controls an open stream (checked before and after the save). */
  controls: () => boolean;
  paste: (text: string) => void;
}

/** A pasted image is saved by the backend and its quoted path typed like a drop (#906). */
export async function pasteImage(blob: Blob, mime: string, io: ImagePaste): Promise<void> {
  const refusal = imagePasteRefusal({
    remote: io.remote,
    control: io.controls(),
  });
  if (refusal) return io.notify(refusal);
  if (!savableImage(mime)) {
    return io.notify("This image type cannot be pasted (png, jpeg, gif, webp).");
  }
  try {
    const bytes = new Uint8Array(await blob.arrayBuffer());
    const path = await io.tauri.invokeRaw<string>("paste_image", bytes, {
      "x-mime": mime,
    });
    if (io.controls()) io.paste(quotePaths([path], isWindows()));
  } catch (e) {
    io.notify(`The pasted image was not saved: ${e}`);
  }
}
