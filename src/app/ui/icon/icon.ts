import { ChangeDetectionStrategy, Component, Input } from "@angular/core";

/**
 * 24×24 icon paths from Material Icons (Apache 2.0), as in Ariadne's editor. Inlined so the app
 * needs no icon font or network access.
 */
const PATHS = {
  zoomIn:
    "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14zm2.5-4h-2v2H9v-2H7V9h2V7h1v2h2v1z",
  zoomOut:
    "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14zM7 9h5v1H7z",
  fitScreen:
    "M17 4h3c1.1 0 2 .9 2 2v2h-2V6h-3V4zM4 8V6h3V4H4c-1.1 0-2 .9-2 2v2h2zm16 8v2h-3v2h3c1.1 0 2-.9 2-2v-2h-2zM7 18H4v-2H2v2c0 1.1.9 2 2 2h3v-2zM18 8H6v8h12V8z",
  close:
    "M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z",
  // Ariadne's direction icons.
  directionDown: "M20 12l-1.41-1.41L13 16.17V4h-2v12.17l-5.58-5.59L4 12l8 8 8-8z",
  directionRight: "M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z",
} as const;

/**
 * Outline icons from Lucide (ISC license, https://lucide.dev), 24×24 with a 2px round stroke, for
 * the tab kinds (#1033). Each icon is a list of path data.
 */
const STROKES = {
  github: [
    "M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4",
    "M9 18c-4.51 2-5-2-7-2",
  ],
  runs: ["M6 3l14 9-14 9z"],
  space: ["M3 3h7v9H3z", "M14 3h7v5h-7z", "M14 12h7v9h-7z", "M3 16h7v5H3z"],
  changes: ["M12 3v14", "M5 10h14", "M5 21h14"],
  // Lucide "file-text": a file preview (#1004).
  preview: [
    "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z",
    "M14 2v4a2 2 0 0 0 2 2h4",
    "M10 9H8",
    "M16 13H8",
    "M16 17H8",
  ],
  flow: [
    "M5 3h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
    "M7 11v4a2 2 0 0 0 2 2h4",
    "M15 13h4a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-4a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2z",
  ],
  settings: [
    "M20 7h-9",
    "M14 17H5",
    "M17 14a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
    "M7 4a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  ],
  readOnly: [
    "M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0z",
    "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  ],
  terminal: ["M4 17l6-6-6-6", "M12 19h8"],
  // Lucide "bot": an agent.
  agent: [
    "M12 8V4H8",
    "M6 8h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z",
    "M2 14h2",
    "M20 14h2",
    "M15 13v2",
    "M9 13v2",
  ],
} as const;

export type IconName = keyof typeof PATHS | keyof typeof STROKES;

/**
 * An icon in the colour of its surroundings. Decorative (hidden from assistive technology) unless
 * it has a `label`, which is its accessible name and its tooltip.
 */
@Component({
  selector: "app-icon",
  host: {
    "[attr.aria-hidden]": "label ? null : 'true'",
    "[attr.role]": "label ? 'img' : null",
    "[attr.aria-label]": "label || null",
    "[attr.title]": "label || null",
    "[style.width.px]": "size",
    "[style.height.px]": "size",
  },
  templateUrl: "./icon.html",
  styles: `
    :host {
      display: inline-flex;
      flex: none;
    }
    svg {
      width: 100%;
      height: 100%;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Icon {
  // A decorator input: the component specs run without Angular's compiler transform for signal inputs.
  @Input({ required: true }) name!: IconName;

  /** The accessible name and tooltip; without it the icon is decorative. */
  @Input() label?: string;
  @Input() size = 18;

  protected get path(): string {
    return this.name in PATHS ? PATHS[this.name as keyof typeof PATHS] : "";
  }

  protected get strokes(): readonly string[] | null {
    return this.name in STROKES ? STROKES[this.name as keyof typeof STROKES] : null;
  }
}
