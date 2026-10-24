# legaia-app-theme

The look the desktop programs share: one palette, in a dark and a light
variant, and the spacing values; and the words both programs that start the
game use when it fails (`game_exit`). No dependencies and no toolkit types,
so every consumer converts a token into its own colour type.

| Consumer | What it takes |
|---|---|
| `legaia-studio` (`theme.rs`) | Both palettes, as egui `Visuals` for its dark and light themes, and the button padding and item spacing; `game_exit` for its failed-game banner. |
| The game launcher (`legaia-engine` with no subcommand, `launcher_window.rs`) | The dark palette, as `[f32; 4]` for its software canvas; `game_exit` for its help box after a failed game. |
| The play window's chrome (`window/hud/text_list.rs`) | `chrome::HEADING` / `KEY` / `TEXT` for the key hint, the toggle notices, the quit prompt and the `F12` panel, and the chrome inset and line gap. |

## The palette

Colours are 8-bit sRGB (`Rgb`); `Rgb::to_f32` gives the normalised,
still-sRGB-encoded `[r, g, b, 1.0]` a texture-modulating draw expects. Each
`Palette` names roles, not hues: `ground` / `band` / `card` / `stripe` for
surfaces from deepest up, `raised` / `hover` / `pressed` / `open` for a
button's states, `rule` for separators and `outline` for a control's edge,
`ink` / `ink_dim` / `ink_faint` for text, `accent` (the gold of the game's
title lettering) with `accent_fill` / `on_accent` for the one primary button a
screen has, and `good` / `warn` / `error` / `info` for status lines.

`DARK` is the launcher's palette, each of its original float colours rounded
to the nearest 8-bit step. `LIGHT` carries the same hues on white, with the
accent darkened until it reads there. Studio's light theme applies only the
surface, accent, selection, error and outline roles over egui's own light
widgets; in both themes a control at rest carries the `outline` edge, without
which an unticked check box all but vanishes into the panel.

The play window's chrome is always dark: it sits on a scrim over whatever the
game draws.

## How a failed game is reported

`game_exit` is the wording the launcher and Studio share when the game they
started stops with an error: `exit_headline` says how the process ended
(the exit code, the signal it died of on Unix, or the exception code a
Windows crash leaves), and `failure_detail` gives the reason the game's log
holds - a panic's message, the root cause of the `Error:` chain the engine
exits with, or its last `ERROR` record - with paths cut to their file name
and the line capped at `DETAIL_MAX`. A log with no failure in it gives no
reason: what a crash leaves is the chatter before it. The module needs `std`
(an exit status, owned strings); the palette does not.

## What the tests hold

- Text contrast on every surface it is drawn over, by the WCAG ratio:
  `ink` at least 7:1; `ink_dim`, `accent`, `good`, `warn`, `error` and
  `info` at least 4.5:1; `ink_faint` and `outline` at least 3:1 (the WCAG
  floor for a control's boundary); button and selected-row text, and
  the primary button's label, likewise.
- The contrast function against the WCAG reference points.
- The dark tokens stay within half an 8-bit step of the launcher's original
  float values.
- `game_exit`: the error-chain, panic and crash-log readings, and the
  signal / exit-code headlines.
