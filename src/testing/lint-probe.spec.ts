import { execFileSync } from "node:child_process";
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

/**
 * #1052: oxlint runs the Angular class rules through an alpha JS plugin and ESLint the template
 * rules. `lint-probe/` holds deliberate violations (left out of `pnpm lint`); this spec lints it and
 * expects every rule id, so a JS plugin or config regression fails here instead of linting nothing.
 */
const PROBE = "src/testing/lint-probe";

interface OxlintReport {
  diagnostics: { code: string; filename: string; severity: string }[];
}

function oxlint(): OxlintReport["diagnostics"] {
  let out: string;
  try {
    out = execFileSync("node_modules/.bin/oxlint", ["-f", "json", PROBE], { encoding: "utf8" });
  } catch (error) {
    // oxlint exits 1 when it reports errors; the report is still on stdout.
    out = (error as { stdout: string }).stdout;
  }
  return (JSON.parse(out) as OxlintReport).diagnostics;
}

describe("lint probe", () => {
  it("oxlint reports the Angular class violations and the inline template", () => {
    const codes = oxlint().map((d) => `${d.filename.replace(`${PROBE}/`, "")} ${d.code}`);
    expect(codes.sort()).toEqual([
      "inline-template.component.ts poimen(no-inline-template)",
      "probe.component.ts @angular-eslint(component-selector)",
      "probe.component.ts @angular-eslint(no-empty-lifecycle-method)",
      "probe.component.ts @angular-eslint(no-output-on-prefix)",
      "probe.component.ts @angular-eslint(prefer-inject)",
      "probe.component.ts @angular-eslint(use-lifecycle-interface)",
    ]);
  });

  it("ESLint reports the template violations", async () => {
    const [result] = await new ESLint({ ignore: false }).lintFiles([
      `${PROBE}/probe.component.html`,
    ]);
    expect(result.messages.map((m) => m.ruleId).sort()).toEqual([
      "@angular-eslint/template/alt-text",
      "@angular-eslint/template/click-events-have-key-events",
      "@angular-eslint/template/interactive-supports-focus",
    ]);
  });
});
