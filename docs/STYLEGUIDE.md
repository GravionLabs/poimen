Extends: node_modules/@gravionlabs/helix-ui/styleguide/STYLEGUIDE.md

# UI style guide

Binding for every UI change. Read the base guide named above first (installed by `pnpm install`), then this file. This file holds only poimen's own rules and deviations; where it says nothing, the base applies. A deviation not listed here needs a reason in the PR. How the parts are wired, and which spec checks them, is in [agents/frontend.md](agents/frontend.md).

## Color

- Colors come from the tokens only. The chrome, the `--app-action*`, `--status-*` and `--origin-*` tokens follow the color scheme ([ADR 0002](adr/0002-color-schemes.md)); `src/styles.css` maps Helix's button tokens onto them.
- Every button variant and state keeps its contrast in every bundled scheme, light and dark.

## Typography and spacing

- `--h-font-size-xs` is the smallest text; only a glyph used as a mark scales it, with `calc()`.
- A component style has no rule whose subject is a `button`, `input`, `select` or `textarea` element. Size or tint a control with tokens on its container and select it by class only for the layout of a row or surface.

## Components

### Buttons, tags and fields (#844)

- The filled primary of a surface is the action the surface exists for (Save, Install).
- Low-weight actions (Cancel in a row, Reset, a link-like step) are `variant="text"`. `severity="danger"` is filled only when it is the one action of a confirmation, else outlined or text.
- A status, count or label that does nothing is an `hx-tag` or plain text: no border, no pointer cursor, no hover. If it can be clicked it is a button (`variant="text"`), with its hover and `:focus-visible` state.
- Inputs and selects in a row use `height: var(--control-height)`, so `align-items: center` lines them up with the buttons.

### Tooltips (#1039)

- An icon-only control uses `hx-tooltip`, not `title`; the `title` attributes that remain are counted per file and the count only goes down.

### Context menus (#851)

- Every context menu is an `app-context-menu` (`src/app/ui/context-menu/`, model in `menu-model.ts`): the parent renders it with `@if` at a point (`at`) with `MenuGroup`s and handles `picked` and `closed`.
- The component owns backdrop, focus, keyboard, clamping and disabled entries. A component has no menu CSS.

### Dialogs (#848)

- Every dialog is an `app-dialog-shell` (`src/app/ui/dialog-shell/`): a native modal `<dialog>` with the heading, the one close icon button and the content projected.
- Buttons go in a `.dialog-buttons` element: Cancel first, the confirming action last. Shared rules for `.hint`, `.error`, labels and fields are under `dialog.dialog` in `src/styles.css`.
- The shell owns focus, Tab, Escape and Enter. `[cancelDefault]="true"` puts the focus on Cancel for a destructive or risky confirmation.
- A dialog spec checks focus on open, Escape and focus return with `ui/dialog-shell/dialog-contract.ts`.

## UX rules

### Feedback (#1040)

- Toast (`SpaceActions.notify` and `fail`): a result the user did not watch, or why a write failed.
- Inline: a lasting state (disconnected, read-only).
- Agent events (waiting, done, failed): the notification center only, never a toast.

## Enforcement

- Specs check these rules: `button-contrast`, `button-usage`, `component-styles`, `input-usage`, `list-row-usage`, `select-usage` and `title-usage` in `src/app/ui/`. Their allowlists name each exception with a reason or a count that only goes down.

## Deviations

| Base rule | This project | Reason |
| --- | --- | --- |
| components.md: a secondary action is `filled` `secondary` | `variant="outlined" severity="secondary"` | Open: why outlined for secondary actions? |
| components.md: forms use the default size; the app sets no height on a control | Small is the default in dense rows, every select and text input is `size="small"`, and `src/styles.css` sets one `--control-height` | One height lines up the controls of a row. Open: why small in forms too? |
| components.md: `input[hx-radio]` for a radio | Radios stay native | Open: why native radios? |
| components.md: `hx-select` for `<select>` | The run picker of the bottom panel is native | It needs option groups |
| components.md: rows are `hx-list-row`s | The sidebar's space cards are hand-built | Treeitems with nested buttons, drag and a tree keyboard that `hx-list` does not have |
| components.md: no app component that redraws a Helix one (`hx-menu`) | `app-context-menu` | Open: why not `hx-menu`? |
| dialogs.md: `hx-dialog`, `hxDialogFooter` without a wrapper, `HxConfirmationService`, no own focus or Enter handling | `app-dialog-shell` with `.dialog-buttons`, which moves the focus and handles Enter; confirmations use it too | Open: why not `hx-dialog`? |
| ux-rules.md: a toast never carries an error the user has to act on | The reason of a failed write is a toast (a failed settings write is also inline) | Open: should every failed write be inline? |
| STYLEGUIDE.md: `@gravionlabs/helix-lint` blocks in CI | Not installed; the specs above check the rules | Open: when to adopt `helix-lint`? |

## When this guide is silent

1. Follow the nearest existing component or screen that solves the same problem.
2. If none exists, choose the simplest option made of Helix components and tokens only.
3. Write the choice and the reason in the PR. If it will recur, file a Helix issue.
