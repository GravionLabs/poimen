// The generic part of a browser stand-in for a Tauri backend (#1109): an answer registry, an event
// emitter, a `Channel` stand-in and the failure on a command nobody answered. It imports nothing from
// the app, so it can move to a package when a second project needs it.

/** What an answer gets: the arguments of the call and the backend (to push events or channel messages). */
export type Answer = (
  args: Record<string, unknown> | undefined,
  backend: DemoBackend,
) => unknown | Promise<unknown>;

/** A table of answers by command. */
export type Answers = Record<string, Answer>;

/** A table of fixed values by command; each call gets a copy. */
export type Fixed = Record<string, unknown>;

/** Stand-in for Tauri's `Channel`: the backend pushes messages through `send`, the view reads `onmessage`. */
export class DemoChannel<T = unknown> {
  private static nextId = 1;
  readonly id = DemoChannel.nextId++;
  onmessage: (message: T) => void = () => {};
  send(message: T): void {
    this.onmessage(message);
  }
}

/** What the page tells the runner (`window.__demoFailures`). */
declare global {
  interface Window {
    __demoFailures?: string[];
  }
}

/** A command that has no answer. The run fails with its name (`window.__demoFailures`). */
export class UnansweredCommandError extends Error {
  constructor(readonly command: string) {
    super(`demo backend has no answer for command "${command}"`);
  }
}

type Handler = (payload: unknown) => void;

export class DemoBackend {
  private readonly answers = new Map<string, Answer>();
  private readonly handlers = new Map<string, Set<Handler>>();
  /** The commands called so far with no answer (also in `window.__demoFailures`). */
  readonly unanswered = new Set<string>();

  answer(table: Answers): this {
    for (const [command, answer] of Object.entries(table)) this.answers.set(command, answer);
    return this;
  }

  fixed(table: Fixed): this {
    for (const [command, value] of Object.entries(table)) {
      this.answers.set(command, () => (value === undefined ? value : structuredClone(value)));
    }
    return this;
  }

  async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const answer = this.answers.get(command);
    if (!answer) {
      this.unanswered.add(command);
      (window.__demoFailures ??= []).push(command);
      console.error(new UnansweredCommandError(command).message);
      throw new UnansweredCommandError(command);
    }
    return (await answer(args, this)) as T;
  }

  listen(event: string, handler: Handler): () => void {
    let set = this.handlers.get(event);
    if (!set) this.handlers.set(event, (set = new Set()));
    set.add(handler);
    return () => set.delete(handler);
  }

  emit(event: string, payload: unknown): void {
    for (const handler of this.handlers.get(event) ?? []) handler(payload);
  }
}
