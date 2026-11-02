# 0002: Color schemes

- Status: Proposed
- Context: PBI #111

## Context

poimen has one hard-coded dark palette and a `theme` setting (light, dark, system) that the toolbar theme button cycles. Users want the colors of their terminal (Ghostty, WezTerm) and of herdr's themes, and the chrome (sidebar, tabs, status bar) should follow the terminal colors instead of clashing with them. Settings come from `~/.config/poimen/config.json` and an optional Ghostty config, where explicit config wins over Ghostty (see `settings.rs`).

## Options

1. **Keep one palette and only recolor xterm.js.** Simple, but the chrome and the terminal drift apart.
2. **Full schemes with a derived chrome.** One scheme drives both terminal and chrome; the model is small and portable.
3. **Bundle every known scheme** (for example all of iTerm2-Color-Schemes). Large, mostly unused, and hard to keep accessible.

## Decision

### Scheme model

A scheme has `name` (identifier), `label`, `background`, `foreground`, `cursor`, `selectionBackground`, `selectionForeground`, `ansi` (16 colors) and optional `chrome` overrides (`--h-*` token values). Appearance (light or dark) is not stored: it follows from the luminance of `background`.

### Sources and precedence

- Bundled schemes: herdr's 15 (catppuccin, tokyo-night, tokyo-night-day, dracula, gruvbox, gruvbox-light, one-dark, one-light, solarized, solarized-light, kanagawa, kanagawa-lotus, rose-pine, rose-pine-dawn, vesper) plus `poimen` (today's colors). They live in the frontend.
- User schemes: TOML files in the WezTerm color scheme format, imported by the user. More schemes arrive this way.
- Precedence: bundled < user TOML (the same name wins), then the Ghostty palette over the scheme, then explicit config over Ghostty (as today).
- Name resolution happens in the frontend. The backend validates the shape of `colorSchema` and sends the user schemes. An unknown name logs a warning and falls back to `poimen`.

### Chrome derivation

- `--h-surface-0…950`: an OKLCH ramp from `background` to `foreground`.
- `--h-primary-*`: derived from ANSI magenta (palette 5).
- Status colors: info from ANSI blue, warning from yellow, success from green.
- `--term-*`: taken directly from the palette.
- `chrome` overrides replace any derived token.
- Every bundled scheme is checked against WCAG AA contrast (text on surfaces, accent on surfaces) in a test.

### Config shape

`colorSchema` sits next to `theme` and is one of: a scheme name, `{ "light": "<name>", "dark": "<name>" }`, or `"herdr"` (use the herdr theme).

- A single scheme fixes light or dark from its background; `theme` is then ignored.
- With a pair, `theme` (light, dark, system) picks the side, and the toolbar theme button works as today.
- With a single scheme or `"herdr"`, the toolbar theme button is disabled and its tooltip explains why.
- The Settings view is an editor tab opened by a gear at the bottom of the activity bar.

## Rejected alternatives

- **`theme` as an object.** Breaks existing configs and mixes the appearance choice with the scheme choice.
- **Reading WezTerm's built-in schemes from its binary.** Fragile, version-dependent and not a public contract; TOML import covers it.
- **Bundling all of iTerm2-Color-Schemes.** Size and maintenance, and no AA guarantee; import covers the long tail.

## Consequences

- One source of color for terminal and chrome, with a tested contrast floor.
- The bundled set and the derivation need maintenance when helix tokens change.
- Appearance from luminance means a scheme can never be mislabeled.

## Follow-up

- Implement the model, derivation and config validation in separate PBIs, then the Settings view.
