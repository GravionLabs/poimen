# 0001: Frontend stack Angular 22 and helix-ui

- Status: Accepted (decided by the owner, Jerome Kaufmann, on 2026-11-01)
- Context: PBI #17, migration PBI #18

## Context

poimen started as a spike: the frontend in `src/` is vanilla TypeScript with Vite and xterm.js (`model.ts` is the pure, tested model; `terminal-view.ts` renders one collapsible terminal card). The spike proved the Rust side (herdr socket client, event watcher, terminal streams through `herdr terminal session observe|control`), but the UI has no component model, no shared state handling and no design system. The next step is an IDE-like layout in the style of VS Code or Zed: an activity bar, a spaces sidebar with agents and terminals, editor tabs, split groups and a status bar.

The owner maintains two references for this:

- **agentlens** (`GravionLabs/agentlens`), a Tauri desktop application with an Angular 22 frontend (standalone components, signals, pnpm), a thin `src-tauri` shell and a bridge to the commands in `src/app/core`. It adopted helix in its ADR 0007.
- **helix** (`GravionLabs/helix`), the in-house Angular library: `@gravionlabs/helix-ui` (vanilla, signal-based components on `--h-*` design tokens, theming with `provideHxTheme`), `@gravionlabs/helix-shell` (application shell with topbar, nav rail and footer), `@gravionlabs/helix-zod` and `@gravionlabs/helix-ag-grid`.

helix-shell does not cover the IDE layout: it offers a topbar, a nav rail and a footer, not an activity bar with a sidebar, editor tabs or split groups.

## Options

1. **Keep vanilla TypeScript.** No framework, but components, state, change detection and accessibility stay our work, and the code shares nothing with agentlens.
2. **Another framework (React, Svelte, Solid).** Viable for a Tauri webview, but it shares neither components nor conventions with the owner's other applications.
3. **Angular 22 and helix-ui, like agentlens.** One stack across the owner's desktop applications, the helix components and tokens, and an existing pattern for the Tauri bridge.

## Decision

- **Framework: Angular 22** with standalone components, signals and pnpm, structured like agentlens: `src-tauri` stays a thin shell, and views call Tauri commands and events through a bridge in `src/app/core`.
- **Components: helix-ui** (`@gravionlabs/helix-ui`) with its tokens (`--h-*`) and `provideHxTheme` for dark mode. helix-shell is used only where it fits; the IDE layout (activity bar, spaces sidebar, editor tabs, split groups, status bar) is built in poimen.
- **Terminals: xterm.js stays**, wrapped in an Angular component.
- **No embedded browser** (decided by the owner on the same day): poimen shows agents and terminals, not web content.
- **Contribution rule.** A component goes to helix-ui when it is generic (it has no herdr knowledge: no panes, agents, spaces or herdr types in its API) and is plausibly useful in another application. Everything else stays in poimen. Named candidates: **tab bar**, **split pane**, **tree view**.

## Consequences

- **Migration.** PBI #18 moves the spike UI to Angular: the model, the terminal cards and the event handling. `model.ts` and its tests move with it.
- **Rust side unchanged.** `herdr.rs`, `watch.rs` and `stream.rs` and their commands keep their contracts; only the frontend that calls them changes.
- **Contributing a component.** A generic component is contributed by a PR in `GravionLabs/helix`; poimen consumes the published helix-ui version, pinned, from the package registry (no `link:` or `workspace:` references). Until it is published, it lives in poimen under a clear folder (`src/app/ui/helix-candidates/`) with no herdr imports, so the move is a copy and an import change.
- **In-house dependency.** poimen follows helix's release cadence and Angular version; a breaking change in helix is ours to absorb, and a fix poimen needs waits for a helix release (or the component stays in the candidates folder longer).
- **Bundle size** is not a concern: poimen is a desktop application loaded from disk, so the Angular and helix bundle has no network cost.
- **Tooling.** `pnpm check` and `pnpm test` change with the migration to the Angular equivalents (build, lint, unit tests); AGENTS.md is updated in #18.

## Follow-up

- #18 migrates the spike UI and builds the IDE layout.
- The tab bar, split pane and tree view start in poimen's candidates folder and move to helix-ui once they are stable.
