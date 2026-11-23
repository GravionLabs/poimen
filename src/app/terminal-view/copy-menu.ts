import type { MenuGroup } from "../ui/context-menu/menu-model";

/** The context menu of an ended terminal (#1233): its history can only be looked at and copied. */
export function copyMenu(hasSelection: boolean): MenuGroup[] {
  return [
    {
      id: "copy",
      items: [
        {
          id: "copy",
          label: "Copy",
          disabled: hasSelection ? false : "Select text first",
          testId: "terminal-copy",
        },
      ],
    },
  ];
}

/** Puts `text` on the clipboard; nothing for an empty selection. */
export function copyText(text: string | undefined): void {
  if (text) void navigator.clipboard?.writeText(text).catch(() => undefined);
}

/** Where the menu opens and whether there is something to copy. */
export function copyMenuAt(event: MouseEvent, selection: string | undefined) {
  return { x: event.clientX, y: event.clientY, selected: !!selection };
}
