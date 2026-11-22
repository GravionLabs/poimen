import { describe, expect, it } from "vitest";
import { fileLinkMenu } from "./file-link-actions";

const ids = (changed: boolean) => fileLinkMenu(changed).flatMap((g) => g.items.map((e) => e.id));

describe("fileLinkMenu", () => {
  it("offers the editor and the preview for any file", () => {
    expect(ids(false)).toEqual(["open", "preview"]);
  });

  it("adds Show in Changes for a changed file, with the same groups on every call", () => {
    expect(ids(true)).toEqual(["open", "preview", "changes"]);
    expect(fileLinkMenu(true)).toBe(fileLinkMenu(true));
  });
});
