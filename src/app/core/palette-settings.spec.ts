import { describe, expect, it } from "vitest";
import {
  DEFAULT_PALETTE,
  isHidden,
  paletteWire,
  parsePalette,
  sourceShown,
  withHidden,
} from "./palette-settings";

const build = { source: "npm", name: "build", description: "tsc" };
const skill = { source: "skills", name: "deliver", description: "" };

describe("palette settings", () => {
  it("defaults to both sources on and nothing hidden", () => {
    expect(parsePalette(undefined)).toEqual(DEFAULT_PALETTE);
    expect(parsePalette({})).toEqual(DEFAULT_PALETTE);
    expect(parsePalette(3)).toEqual(DEFAULT_PALETTE);
  });

  it("parses the sources and the hidden entries per repository, skipping bad ones", () => {
    const p = parsePalette({
      scripts: false,
      skills: true,
      hidden: { "/r": [{ source: "npm", name: "build" }, { source: 1 }], "/q": "x", "/e": [] },
    });
    expect(p).toEqual({
      scripts: false,
      skills: true,
      hidden: { "/r": [{ source: "npm", name: "build" }] },
    });
  });

  it("maps a skill to the skills source and everything else to scripts", () => {
    const p = { ...DEFAULT_PALETTE, skills: false };
    expect(sourceShown(p, skill)).toBe(false);
    expect(sourceShown(p, build)).toBe(true);
  });

  it("hides and shows an entry for one repository only", () => {
    const hidden = withHidden(DEFAULT_PALETTE, "/r", build, true);
    expect(isHidden(hidden, "/r", build)).toBe(true);
    expect(isHidden(hidden, "/other", build)).toBe(false);
    expect(isHidden(hidden, "/r", skill)).toBe(false);
    expect(paletteWire(hidden)).toEqual({
      sources: { scripts: true, skills: true },
      hidden: { "/r": [{ source: "npm", name: "build" }] },
    });
    // Hiding twice keeps one entry; showing drops the repository once it is empty.
    expect(withHidden(hidden, "/r", build, true).hidden["/r"]).toHaveLength(1);
    expect(withHidden(hidden, "/r", build, false)).toEqual(DEFAULT_PALETTE);
  });
});
