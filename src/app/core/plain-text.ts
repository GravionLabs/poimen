/**
 * Text a program reported (OSC 7501, hooks) as plain text for display: bidi overrides, isolates,
 * zero-width and other invisible formatting characters and control characters are removed, so
 * a report cannot reorder or hide what the user reads (#682).
 */
export function plainText(text: string | undefined): string {
  return (text ?? "")
    .replace(
      // oxlint-disable-next-line no-control-regex
      /[\u0000-\u001f\u007f-\u009f\u00ad\u061c\u180e\u200b-\u200f\u202a-\u202e\u2060-\u206f\ufeff\ufff9-\ufffb]|\udb40[\udc00-\udc7f]/g,
      "",
    )
    .trim();
}
