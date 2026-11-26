import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Text inputs and textareas are `hx-input size="small"` in an `hx-field`, checkboxes are
 * `hx-checkbox` or `hx-switch`, and an error is the `[error]` of its field or an `hx-message` (#1362,
 * `docs/agents/frontend.md`). A radio is left alone. `.html` templates only: a component has no
 * inline template (`poimen/no-inline-template`).
 */
function sources(dir: string, suffix: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path, suffix);
    return name.endsWith(suffix) ? [path] : [];
  });
}

const templates = sources("src/app", ".html").map(
  (file) => [file, readFileSync(file, "utf8").replace(/<!--.*?-->/gs, "")] as const,
);

const ATTRS = `(?:[^>"']|"[^"]*"|'[^']*')*`;

function tags(text: string, name: string): string[] {
  return text.match(new RegExp(`<${name}(?=[\\s/>])${ATTRS}>`, "g")) ?? [];
}

const has = (tag: string, attribute: string) => new RegExp(`\\s${attribute}(?=[\\s=/>])`).test(tag);
const type = (tag: string) => /\stype="([a-z]+)"/.exec(tag)?.[1] ?? "text";

/** The text controls of a template: inputs that are neither a checkbox nor a radio, and textareas. */
function textControls(text: string): string[] {
  const inputs = tags(text, "input").filter((t) => !["checkbox", "radio"].includes(type(t)));
  return [...inputs, ...tags(text, "textarea")];
}

describe("inputs", () => {
  it("are small hx-inputs", () => {
    const bad = templates.flatMap(([file, text]) =>
      textControls(text)
        .filter((tag) => !has(tag, "hx-input") || !tag.includes('size="small"'))
        .map((tag) => `${file}: ${tag}`),
    );
    expect(bad).toEqual([]);
  });

  it("sit in an hx-field", () => {
    const bad = templates.flatMap(([file, text]) =>
      textControls(text.replace(/<hx-field[\s>].*?<\/hx-field>/gs, "")).map(
        (tag) => `${file}: ${tag}`,
      ),
    );
    expect(bad).toEqual([]);
  });

  it("are hx-checkbox or hx-switch for a checkbox", () => {
    const bad = templates.flatMap(([file, text]) =>
      tags(text, "input")
        .filter((t) => type(t) === "checkbox" && !has(t, "hx-checkbox") && !has(t, "hx-switch"))
        .map((tag) => `${file}: ${tag}`),
    );
    expect(bad).toEqual([]);
  });

  it("leave aria-invalid and the error markup to hx-field and hx-message", () => {
    const bad = templates.flatMap(([file, text]) => [
      ...textControls(text)
        .filter((tag) => /aria-(invalid|describedby)/.test(tag))
        .map((tag) => `${file}: ${tag}`),
      ...(text.match(new RegExp(`<(p|span|div|li)\\s${ATTRS}role="alert"${ATTRS}>`, "g")) ?? [])
        // The trust question is a block of text with its own buttons, not an error.
        .filter((tag) => !tag.includes('class="trust"'))
        .map((tag) => `${file}: ${tag}`),
    ]);
    expect(bad).toEqual([]);
  });

  it("have their directive imported by the component", () => {
    const needs: [RegExp, string][] = [
      [/\shx-input(?=[\s/>])/, "HxInput"],
      [/<hx-input-group[\s>]/, "HxInputGroup"],
      [/\shx-checkbox(?=[\s/>])/, "HxCheckbox"],
      [/\shx-switch(?=[\s/>])/, "HxSwitch"],
      [/<hx-message[\s>]/, "HxMessage"],
    ];
    const bad = sources("src/app", ".ts")
      .filter((file) => !file.endsWith(".spec.ts"))
      .flatMap((file) => {
        const code = readFileSync(file, "utf8");
        const imported = [...code.matchAll(/\bimports: \[([^\]]*)\]/g)].flatMap((m) =>
          m[1].split(",").map((name) => name.trim()),
        );
        return [...code.matchAll(/templateUrl: "\.\/([^"]+)"/g)].flatMap(([, url]) => {
          const html = readFileSync(join(file, "..", url), "utf8");
          return needs
            .filter(([pattern, name]) => pattern.test(html) && !imported.includes(name))
            .map(([, name]) => `${file}: ${name} is not imported`);
        });
      });
    expect(bad).toEqual([]);
  });
});
