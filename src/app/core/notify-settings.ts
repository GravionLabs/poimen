/** What `notify.<status>` does (#1017): a command (argv) and/or a URL that gets a JSON POST. */
export interface NotifyHook {
  command: string[] | null;
  url: string | null;
}

/** `notify`: a hook for `blocked` and `done`; the server runs them, the app only shows them. */
export interface NotifySettings {
  blocked: NotifyHook;
  done: NotifyHook;
  timeoutSeconds: number;
}

export const DEFAULT_NOTIFY: NotifySettings = {
  blocked: { command: null, url: null },
  done: { command: null, url: null },
  timeoutSeconds: 10,
};

function parseHook(v: unknown): NotifyHook {
  const r = v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  const command = r["command"];
  return {
    command: Array.isArray(command)
      ? command.filter((c): c is string => typeof c === "string")
      : null,
    url: typeof r["url"] === "string" ? r["url"] : null,
  };
}

/** The `notify` of the backend's settings (already validated by it). */
export function parseNotify(v: unknown): NotifySettings {
  const r = v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  return {
    blocked: parseHook(r["blocked"]),
    done: parseHook(r["done"]),
    timeoutSeconds:
      typeof r["timeoutSeconds"] === "number" ? r["timeoutSeconds"] : DEFAULT_NOTIFY.timeoutSeconds,
  };
}
