import { HighlightStyle, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import type { Extension } from "@codemirror/state";
import { tags as t } from "@lezer/highlight";

/** The language for a file name, or null for an unknown one (plain text). Not loaded yet. */
export function languageFor(
  path: string,
  available: readonly LanguageDescription[] = languages,
): LanguageDescription | null {
  const name = path.split("/").pop() ?? path;
  return name ? LanguageDescription.matchFilename(available, name) : null;
}

/** Loads the language support of a file on demand; plain text (null) when unknown or failing. */
export async function loadLanguage(
  path: string,
  available: readonly LanguageDescription[] = languages,
): Promise<Extension | null> {
  const description = languageFor(path, available);
  if (!description) return null;
  try {
    return await description.load();
  } catch {
    return null;
  }
}

/** Maps a palette index (ANSI 0-15) to a CSS color. */
export type AnsiColor = (index: number) => string;

/** The live scheme: the `--term-ansi-*` tokens, so a scheme or light/dark switch needs no rebuild. */
export const cssAnsi: AnsiColor = (index) => `var(--term-ansi-${index})`;

/** Highlight rules over the ANSI palette (normal colors; comments use bright black). */
export function highlightRules(color: AnsiColor = cssAnsi) {
  return [
    { tag: [t.keyword, t.operatorKeyword, t.modifier, t.controlKeyword], color: color(5) },
    { tag: [t.string, t.special(t.string), t.regexp], color: color(2) },
    { tag: [t.number, t.bool, t.null, t.atom], color: color(3) },
    { tag: [t.comment, t.docComment], color: color(8), fontStyle: "italic" },
    {
      tag: [t.function(t.variableName), t.function(t.propertyName), t.labelName],
      color: color(4),
    },
    { tag: [t.typeName, t.className, t.namespace, t.tagName], color: color(6) },
    { tag: [t.propertyName, t.attributeName], color: color(4) },
    { tag: [t.definition(t.variableName), t.macroName], color: color(6) },
    { tag: [t.operator, t.punctuation, t.separator], color: color(7) },
    { tag: [t.heading], color: color(4), fontWeight: "bold" },
    { tag: [t.link, t.url], color: color(6), textDecoration: "underline" },
    { tag: t.emphasis, fontStyle: "italic" },
    { tag: t.strong, fontWeight: "bold" },
    { tag: t.invalid, color: color(1) },
  ];
}

/** The syntax highlighting extension for the diff. */
export function diffHighlight(color: AnsiColor = cssAnsi): Extension {
  return syntaxHighlighting(HighlightStyle.define(highlightRules(color)));
}
