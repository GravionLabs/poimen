// Replaces `src/app/core/tauri.ts` in the `demo` build configuration (#1109, `fileReplacements`): the
// app runs in a browser against the canned answers of `poimen/`. Not part of the production bundle.
import { DemoBridge } from "./kit/bridge";
import { createPoimenDemo } from "./poimen/demo";

export type { DragDrop, Unlisten } from "./kit/bridge";
export { DemoBridge as TauriBridge };

DemoBridge.backend = createPoimenDemo();
