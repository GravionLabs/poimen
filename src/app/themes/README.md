# Bundled color schemes

One JSON file per scheme (`ColorScheme` of `core/color-scheme.ts`). `poimen-light` and `poimen-dark` are poimen's own colors (the `--term-*` tokens of `styles.css`). The other 15 are the schemes of herdr, with the canonical palette values of the upstream projects (ANSI 0 to 15 in order; the cursor and selection colors follow the upstream terminal ports):

| Scheme | Upstream | License |
| --- | --- | --- |
| `catppuccin` (Mocha) | catppuccin/catppuccin | MIT |
| `tokyo-night`, `tokyo-night-day` | folke/tokyonight.nvim | Apache-2.0 |
| `dracula` | dracula/dracula-theme | MIT |
| `gruvbox`, `gruvbox-light` | morhetz/gruvbox | MIT |
| `one-dark`, `one-light` | Atom One (atom/atom, joshdick/onedark.vim) | MIT |
| `solarized`, `solarized-light` | altercation/solarized | MIT |
| `kanagawa` (Wave), `kanagawa-lotus` | rebelot/kanagawa.nvim | MIT |
| `rose-pine`, `rose-pine-dawn` | rose-pine/rose-pine-theme | MIT |
| `vesper` | raunofreiberg/vesper | MIT |

Colors are facts about a palette; the upstream names are listed for attribution.
