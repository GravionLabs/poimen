// Replaces `src/app/core/tauri.ts` in the `demo` build configuration (#1109, `fileReplacements`): the
// app runs in a browser against a backend stand-in. Not part of the production bundle. One build, two
// variants (#1156): the canned answers of `poimen/` (default), or with `?backend=fake` the fake that
// answers only what the test put into `window.__demoFake` before the page ran.
import { DemoBridge } from "./kit/bridge";
import { createFakeBackend } from "./kit/fake";
import { createPoimenDemo } from "./poimen/demo";

export type { DragDrop, Unlisten } from "./kit/bridge";
export { DemoBridge as TauriBridge };

DemoBridge.backend =
  new URLSearchParams(window.location.search).get("backend") === "fake"
    ? createFakeBackend(window.__demoFake)
    : createPoimenDemo();
window.__demo = DemoBridge.backend;
