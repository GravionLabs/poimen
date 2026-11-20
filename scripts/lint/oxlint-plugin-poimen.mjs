/**
 * poimen's own oxlint rules (an oxlint JS plugin, `.oxlintrc.json` `jsPlugins`).
 *
 * `poimen/no-inline-template` (#1051): a `@Component` keeps its template in a sibling `.html` file
 * (`templateUrl`), so ESLint's template and accessibility rules see every product template.
 * `.oxlintrc.json` turns it off for spec files, whose test hosts keep inline templates.
 */
const noInlineTemplate = {
  meta: {
    type: "problem",
    docs: { description: "Disallow an inline template in a @Component decorator." },
    messages: {
      inline: "Put the component template in a sibling .html file and use templateUrl (#1051).",
    },
    schema: [],
  },
  create(context) {
    return {
      Decorator(node) {
        const call = node.expression;
        if (call?.type !== "CallExpression" || call.callee?.name !== "Component") return;
        const meta = call.arguments[0];
        if (meta?.type !== "ObjectExpression") return;
        for (const prop of meta.properties) {
          const key = prop.type === "Property" ? prop.key : null;
          const name = key?.type === "Identifier" ? key.name : key?.value;
          if (name === "template") context.report({ node: prop, messageId: "inline" });
        }
      },
    };
  },
};

export default {
  meta: { name: "poimen" },
  rules: { "no-inline-template": noInlineTemplate },
};
