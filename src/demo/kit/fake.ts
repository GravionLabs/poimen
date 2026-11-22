import { DemoBackend, type Answers, type Fixed } from "./backend";

/**
 * The fake variant for interaction tests (#1156): it answers only what the test sets up (`fixed`
 * values, or `answers` that compute from the call), fails on any other command by name as the demo
 * does, and logs every call (`calls`). The test replaces an answer with `backend.set(command, value)`
 * or pushes an event with `backend.emit(event, payload)` at runtime through `window.__demo`.
 */
export function createFakeBackend(fixed: Fixed = {}, answers: Answers = {}): DemoBackend {
  return new DemoBackend().fixed(fixed).answer(answers);
}
