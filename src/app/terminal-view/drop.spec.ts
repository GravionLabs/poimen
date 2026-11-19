import { describe, expect, it } from "vitest";
import {
  dropAction,
  imagePasteRefusal,
  overRect,
  pastedImageType,
  quotePaths,
  quotePosix,
  quotePowerShell,
  savableImage,
} from "./drop";

describe("quoting", () => {
  it("quotes spaces and unicode in single quotes", () => {
    expect(quotePaths(["/a b/c", "/tmp/Ünï cödé 🎉.txt"], false)).toBe(
      "'/a b/c' '/tmp/Ünï cödé 🎉.txt'",
    );
  });
  it("escapes a single quote for POSIX and doubles it for PowerShell", () => {
    expect(quotePosix("/it's")).toBe(`'/it'\\''s'`);
    expect(quotePowerShell("C:\\it's")).toBe("'C:\\it''s'");
    expect(quotePaths(["C:\\a b"], true)).toBe("'C:\\a b'");
  });
  it("leaves shell metacharacters inert", () => {
    expect(quotePosix("/a$(x);`y`")).toBe("'/a$(x);`y`'");
  });
});

describe("overRect", () => {
  const rect = { left: 10, top: 20, right: 110, bottom: 120 };
  it("scales physical pixels to CSS pixels", () => {
    expect(overRect({ x: 40, y: 80 }, rect, 2)).toBe(true);
    expect(overRect({ x: 10, y: 80 }, rect, 2)).toBe(false);
    expect(overRect({ x: 220, y: 80 }, rect, 2)).toBe(false);
    expect(overRect({ x: 40, y: 240 }, rect, 2)).toBe(false);
  });
});

describe("dropAction", () => {
  const ok = { remote: false, control: true, windows: false };
  it("types the quoted paths in control mode", () => {
    expect(dropAction(["/a b"], ok)).toEqual({ kind: "type", text: "'/a b'" });
  });
  it("refuses on a remote pane", () => {
    expect(dropAction(["/a"], { ...ok, remote: true })).toMatchObject({ kind: "refuse" });
  });
  it("refuses in observer mode", () => {
    expect(dropAction(["/a"], { ...ok, control: false })).toMatchObject({ kind: "refuse" });
  });
  it("ignores an empty drop", () => {
    expect(dropAction([], ok)).toBeNull();
  });
});

describe("image paste helpers (#906)", () => {
  it("finds the image of a paste without text", () => {
    expect(pastedImageType(["image/png"])).toBe("image/png");
    expect(pastedImageType(["text/plain", "image/png"])).toBeNull();
    expect(pastedImageType(["text/html"])).toBeNull();
  });
  it("refuses remote and read-only", () => {
    expect(imagePasteRefusal({ remote: true, control: true })).toContain("remote");
    expect(imagePasteRefusal({ remote: false, control: false })).toContain("read-only");
    expect(imagePasteRefusal({ remote: false, control: true })).toBeNull();
  });
  it("saves only the four types", () => {
    expect(savableImage("image/webp")).toBe(true);
    expect(savableImage("image/svg+xml")).toBe(false);
  });
});
