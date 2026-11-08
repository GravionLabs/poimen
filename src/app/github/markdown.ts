import DOMPurify from "dompurify";
import { Marked } from "marked";

const marked = new Marked({ gfm: true, breaks: false });

/** Only these URL schemes survive in links and images; `javascript:` and `data:` do not. */
const SAFE_URI = /^(?:https?:|mailto:|#|\/|\.)/i;

// DOMPurify lets `data:` through on images whatever the URI pattern says; bodies get none.
DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.getAttribute?.("src")?.trim().toLowerCase().startsWith("data:")) {
    node.removeAttribute("src");
  }
});

/**
 * Issue and pull request bodies are untrusted: they are rendered as Markdown and always
 * sanitized (no script, no event handlers, no `javascript:` URLs). The only place that turns
 * Markdown into HTML.
 */
export function renderMarkdown(source: string): string {
  const html = marked.parse(source ?? "", { async: false });
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style", "form", "input", "button", "iframe", "object", "embed"],
    ALLOWED_URI_REGEXP: SAFE_URI,
  });
}
