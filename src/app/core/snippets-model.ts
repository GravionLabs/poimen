import { agentKind, type Terminal } from "./snapshot";

/** A saved prompt or command (`snippets[]`, wire shape of `snippets.rs`). */
export interface Snippet {
  label: string;
  text: string;
  /** Press Enter after the text; only a `terminal` snippet reads it (an agent prompt is submitted). */
  enter: boolean;
  target: "terminal" | "agent";
}

/** The snippets of a settings answer; malformed entries and repeated labels are skipped. */
export function parseSnippets(raw: unknown): Snippet[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  return raw.flatMap((item): Snippet[] => {
    if (typeof item !== "object" || item === null) return [];
    const r = item as Record<string, unknown>;
    const label = typeof r["label"] === "string" ? r["label"].trim() : "";
    const text = r["text"];
    if (label === "" || typeof text !== "string" || text.trim() === "" || seen.has(label)) {
      return [];
    }
    seen.add(label);
    return [
      {
        label,
        text,
        enter: r["enter"] !== false,
        target: r["target"] === "agent" ? "agent" : "terminal",
      },
    ];
  });
}

/** Whether `snippet` is offered for `pane`: an agent snippet only for an agent pane. */
export function offeredFor(snippet: Snippet, pane: Terminal | undefined): boolean {
  if (!pane) return false;
  return snippet.target === "terminal" || agentKind(pane) !== undefined;
}
