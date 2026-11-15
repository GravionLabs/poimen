# ADR 0011: Named layouts referenced by templates

- Status: Proposed
- Date: 2026-11-15
- Issues: #608
- Builds on: [ADR 0006](0006-poimen-console-with-herdr-scope.md)

## Context

A new space gets its layout from one of four places (`layout::resolve`): the project file, a space template, `spaces.layout` of the user config, or the built-in default. A template carries its own inline `layout`. Today every layout is a list of `panes[]` (`name`, `command`, and from the second pane on `split.from`, `split.direction`, `split.ratio`): one tab, split into panes.

This has limits:

- A layout cannot be reused. Two templates that want the same layout copy it, and `spaces.layout` is the only layout that has a name of sorts ("the default").
- A layout is one tab. The server has had several tabs per space, each with its own split tree, since [ADR 0006](0006-poimen-console-with-herdr-scope.md), but no config can ask for them.
- The spaces tree can show tabs below their parent (#655, #656), and the app can open editor groups side by side (#657), but a layout cannot describe either.

The project file's `presets[]` use the same `panes[]` shape and appear as palette entries of that project.

## Decision

Layouts get names and the shape of the server model. Templates refer to them by name.

### `layouts[]` in the user config

`layouts` is an array of `{name, tabs}`. `name` is required and unique; an invalid entry warns and is skipped, as for `spaceTemplates`.

A template refers to a layout by name: `{"name": "work", "match": {...}, "agent": "claude", "layout": "review"}`. An unknown name warns and the template falls through to the next layout of the precedence. An inline layout object in a template stays allowed. A layout is user config and needs no trust, as today.

### Shape of a layout

`{name, tabs: [Tab]}`. A tab is one of two forms:

- Flat, for a tab with one terminal: `{name?, command?, cwd?, children?, place?}`.
- With splits: `{name?, root: Node, children?, place?}`, where `Node` is as in the server protocol (`docs/protocol.md`, tab tree):
  - leaf `{"kind": "terminal", "name"?, "command"?, "cwd"?}`;
  - `{"kind": "split", "direction": "right" | "down", "ratio"?, "children": [Node, Node]}`. `right` puts the second child right of the first, `down` below it; `ratio` is the share of the first child, above 0 and below 1, default 0.5.

A tab has either the flat fields or `root`, never both (a parse error that names the tab). `split_id` is never in a layout: the server assigns it. `command` is typed into the shell followed by Enter and `cwd` is relative to the space directory (default the space directory), as for `PaneSpec` today. The first terminal of the first tab is the root terminal of the space, the one a task's command replaces.

### Nested tabs

A tab may have `children[]`: tabs again, any depth. The spaces tree shows them below their parent (#655, #656). Nesting is about the tree, not the screen:

```json
{ "tabs": [{ "name": "claude", "command": "claude", "children": [{ "name": "Terminal" }, { "name": "Logs" }] }] }
```

gives the tab `claude` with the tabs `Terminal` and `Logs` below it. A child tab is a tab of the space like any other; the server stores the parent link as a field of the tab. Several top-level tabs are allowed: each is a tab of the space and a direct child of the space in the spaces tree, in config order.

### App-only placement

`place: {direction, of?, ratio?}` is per tab and read by the Tauri app only:

- `direction` is `right` or `down`.
- `of` names a tab of the same layout. Default: the parent tab; for a top-level tab, the first tab.
- `ratio` is the share `of` keeps, default 0.5.
- The app opens the tab in an editor group next to (`right`) or below (`down`) the group of `of`. Tabs with the same `place` share that group. Without `place` a tab opens in the group of its parent; a top-level tab in the main group.

Side by side in the app is editor groups, not splits inside the agent tab (#657). The TUI ignores `place` and shows the tabs as the server holds them. A `place` that names an unknown tab, or a cycle, warns and is ignored.

```json
{
  "tabs": [
    {
      "name": "claude",
      "command": "claude",
      "children": [
        { "name": "Terminal", "place": { "direction": "right", "ratio": 0.6 } },
        { "name": "Logs", "place": { "direction": "down", "of": "Terminal" } }
      ]
    },
    { "name": "review", "command": "copilot" }
  ]
}
```

In the app `claude` is left (60 %), `Terminal` right top and `Logs` right bottom, and `review` is a second top-level tab in the main group. In the spaces tree: `claude › Terminal, Logs` and `review` below the space.

### How today's `panes[]` map onto it

`panes[]` stays valid everywhere it is accepted. It reads as one tab whose terminals are the panes:

- The first pane is the root leaf. Each later pane replaces the leaf named by `split.from` with a `split` whose first child is that leaf and whose second child is the new pane; `split.direction` becomes `direction`.
- `split.ratio` is the share the `from` pane keeps, which is the share of the first child, so it becomes `ratio` unchanged. This is what `tab.split` does today.
- `name` and `command` carry over. A layout of one pane becomes a flat tab.

The built-in default layout (`agent` left 60 % running `claude`, `tests` right top, `shell` right bottom) as a named layout:

```json
{
  "name": "default",
  "tabs": [
    {
      "name": "agent",
      "root": {
        "kind": "split",
        "direction": "right",
        "ratio": 0.6,
        "children": [
          { "kind": "terminal", "name": "agent", "command": "claude" },
          {
            "kind": "split",
            "direction": "down",
            "ratio": 0.5,
            "children": [
              { "kind": "terminal", "name": "tests" },
              { "kind": "terminal", "name": "shell" }
            ]
          }
        ]
      }
    }
  ]
}
```

### Old keys keep working

- `spaces.layout` is read as the layout named `default`. If `layouts[]` also has `default`, `layouts[]` wins and `spaces.layout` is ignored with a warning.
- The default is the template without `match` (unchanged); it refers to a layout by name, or has none and uses `default`.
- Inline layouts in templates stay allowed.
- Nothing is migrated at start and no file is rewritten. The settings write the new shape (`layouts[]`, `layout: "<name>"`) only when the user edits a layout or a template.
- The project file's `presets[]` stay what they are: palette entries of that project (`Preset: <name>`), behind the trust gate. They are not named layouts and are not in `layouts[]`.

### Precedence

`layout::resolve` is unchanged: a task's command, then the project file, then the template's layout, then `spaces.layout` (the layout `default`), then the built-in default. Only what "the template's layout" is changes: a name looked up in `layouts[]`, or an inline layout.

### Server

The server builds a layout with `build_tabs` (`space.create`, `space.apply_layout`). It gains the parent link of a tab and the nested tabs. `place` is not sent to the server: the app reads it from the layout it resolved and applies it to editor groups. The wire changes are additive, get a protocol bump, a parsing test in `poimen-protocol` and a section in `docs/protocol.md`; the follow-up issues of #582 do that.

## Consequences

- One layout can serve several templates, and a layout can have several tabs, nested tabs and an app arrangement.
- Existing configs, project files and presets behave as before. A version that does not know `layouts[]` ignores the key and uses the template's inline layout or the default.
- Two shapes (`panes[]` and `tabs[]`) are read for as long as the old keys exist. The mapping above is a pure function with tests.
- `place` lives only in the config and the app. The server model stays free of editor groups, and the TUI shows the plain tab order. A layout looks different in the app and the TUI by design.
- Follow-up work (#582): parser and resolver in `poimen-core`, nested tabs in the server, the spaces tree, editor groups from `place`, and the settings editor.

## Alternatives

- **Keep `panes[]` and add only names.** Smallest change, but one tab per layout, so no nested tabs or several top-level tabs.
- **Store layouts in the server, not the config.** The server owns pane state, but a layout is a preference like templates and `spaces.layout`, and both clients already read the user config.
- **Make `presets[]` named layouts.** They live in a project file behind the trust gate and belong to one project; merging them into the user's list would mix trust levels.
- **Splits inside the agent tab for side by side in the app.** The agent TUIs need the width; editor groups (#657) give side by side without that cost.
- **Migrate old keys on start.** It rewrites the user's file unasked; the settings write the new shape only on edit.
