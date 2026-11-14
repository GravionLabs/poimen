import { vi } from "vitest";

/**
 * A stand-in for `@tauri-apps/api`: records `invoke` calls, answers them from `answers`, and lets a
 * test emit events to the registered `listen` handlers.
 */
function tauriMock() {
  const handlers = new Map<string, Set<(event: { payload: unknown }) => void>>();
  const answers = new Map<string, (args?: Record<string, unknown>) => unknown>();

  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    const answer = answers.get(command);
    if (!answer) return undefined;
    return await answer(args);
  });
  const unlisten = vi.fn();
  const listen = vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    if (!handlers.has(event)) handlers.set(event, new Set());
    handlers.get(event)!.add(handler);
    return () => {
      unlisten(event);
      handlers.get(event)?.delete(handler);
    };
  });

  /** Stand-in for Tauri's `Channel`: a test pushes messages through `onmessage`. */
  class Channel<T = unknown> {
    onmessage: (message: T) => void = () => {};
  }

  return {
    invoke,
    Channel,
    listen,
    unlisten,
    /** Sets how `command` answers (a thrown error rejects the call). */
    answer(command: string, fn: (args?: Record<string, unknown>) => unknown) {
      answers.set(command, fn);
    },
    emit(event: string, payload: unknown) {
      handlers.get(event)?.forEach((handler) => handler({ payload }));
    },
    calls(command: string) {
      return invoke.mock.calls.filter(([c]) => c === command).map(([, args]) => args);
    },
    reset() {
      handlers.clear();
      answers.clear();
      invoke.mockClear();
      listen.mockClear();
      unlisten.mockClear();
    },
  };
}

/**
 * Shared by the `vi.mock` factories and the test, so both see the same instance:
 *
 * ```ts
 * vi.mock("@tauri-apps/api/core", async () => ({ invoke: (await import("./tauri.mock")).tauri.invoke }));
 * ```
 */
export const tauri = tauriMock();
