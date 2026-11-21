import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { bundled, colorDistance, contrast, deriveTokens } from "../core/color-scheme";

/** Status colors come from the `--status-*` tokens, never a hex value (#859). */
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : /\.(css|ts|html)$/.test(name) ? [path] : [];
  });
}

const SKIP = /(\.spec\.ts|src\/styles\.css|core\/schemes?[^/]*|core\/fixtures\/.*)$/;
const STATUS = /status|\.dot\b|connected|reconnecting|unreachable|\.failed|\.warn|\.channel/i;

describe("status colors", () => {
  it("uses no hex color in a rule about a status", () => {
    const bad: string[] = [];
    for (const file of files("src").filter((f) => !SKIP.test(f))) {
      let selector = "";
      readFileSync(file, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (/\{\s*$/.test(line)) selector = line;
          if (
            /(color|background)\s*:[^;]*#[0-9a-fA-F]{3,8}\b/.test(line) &&
            STATUS.test(selector + line)
          ) {
            bad.push(`${file}:${i + 1}: ${line.trim()}`);
          }
        });
    }
    expect(bad).toEqual([]);
  });

  it("gives the unread mark its own color, 3:1 on the surface, in every bundled scheme (#1031)", () => {
    for (const scheme of bundled.values()) {
      const t = deriveTokens(scheme);
      const unread = t["--status-unread"];
      expect(contrast(unread, scheme.background), scheme.name).toBeGreaterThanOrEqual(3);
      for (const state of ["working", "blocked", "done", "error"]) {
        const color = state === "error" ? scheme.ansi[1] : t[`--status-${state}`];
        expect(colorDistance(unread, color), `${scheme.name} vs ${state}`).toBeGreaterThan(40);
      }
    }
  });
});
