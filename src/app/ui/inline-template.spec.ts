import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

/**
 * #1051: component templates live in .html files. The lint config fails on an inline `template:` in
 * a `@Component` outside spec files; this spec proves the rule fires and stays scoped.
 */
const component = (meta: string) => `
import { Component } from "@angular/core";

@Component({ selector: "app-probe", ${meta} })
export class Probe {}
`;

async function ruleIds(code: string, filePath: string): Promise<string[]> {
  const [result] = await new ESLint().lintText(code, { filePath });
  return result.messages.map((m) => m.ruleId ?? m.message);
}

describe("inline component templates", () => {
  it("fail the lint in a component source", async () => {
    expect(await ruleIds(component("template: `<p>x</p>`"), "src/app/probe.ts")).toContain(
      "no-restricted-syntax",
    );
  });

  it("pass with templateUrl", async () => {
    expect(await ruleIds(component('templateUrl: "./probe.html"'), "src/app/probe.ts")).toEqual([]);
  });

  it("stay allowed in spec files", async () => {
    expect(await ruleIds(component("template: `<p>x</p>`"), "src/app/probe.spec.ts")).toEqual([]);
  });
});
