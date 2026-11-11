# legaia-app-theme

The look the desktop programs share: one palette, in a dark and a light
variant, and the spacing values; and the words both programs that start the
game use when it fails (`game_exit`); the two programs' icons and where the
release archive puts them ([below](#icons-and-where-the-release-puts-the-programs)).
No dependencies and no toolkit types, so every consumer converts a token into
its own colour type.

| Consumer | What it takes |
|---|---|
| `legaia-explorer` (`theme.rs`) | Both palettes, as egui `Visuals` for its dark and light themes, and the button padding and item spacing; `game_exit` for its failed-game banner. |
| The game launcher (`legaia-engine` with no subcommand, `launcher_window.rs`) | Both palettes by `Role` for its software canvas (`resolve_dark` over the *Menu colours* row); `game_exit` for its help box after a failed game. |
| The play window's chrome (`window/hud/text_list.rs`) | `chrome::inks(dark)` for the key hint, the toggle notices, the quit prompt and the `F12` panel's settings and key pages, `chrome::option_value` for a settings row's value (the launcher's list draws its values by the same rule, so a row reads alike in both), and the chrome inset and line gap. |

## Icons, and where the release puts the programs

`icons/` holds the two programs' icons - the game (a green tree under a
night sky, mist across its roots) and Legaia Explorer (a disc under a
magnifying glass) - as original artwork, no game asset drawn from or traced.
`icons/make_icons.py` (Python + Pillow) is their source: it draws each icon
from a short list of flat shapes and writes the SVG, the 256 px PNG, the
`.ico` and the `.icns`, all committed, so no build runs it.

| Module | What it carries |
|---|---|
| `icons` | The PNGs as bytes (`GAME_PNG`, `EXPLORER_PNG`) for the window icons, and the `.ico` paths for build scripts. |
| `win_icon` | `embed_in_bins`, for a build script: compiles a one-icon resource with the toolchain's resource compiler (`<arch>-w64-mingw32-windres`, `windres`, `llvm-windres`, `rc.exe`, or `$LEGAIA_WINDRES`) and links it into the package's binaries, on a Windows target only. |
| `drop_backend` | `prefer_x11_for_drop_from_env`: on a Wayland session with XWayland on offer, the launcher and Explorer open as X11 clients, because winit 0.30 has no Wayland drag-and-drop; each has its own opt-out (`LEGAIA_LAUNCHER_WAYLAND`, `LEGAIA_EXPLORER_WAYLAND`). |
| `settings_file` | `set_aside` / `set_aside_notice` / `write_atomic`: a settings file that exists but does not parse is renamed to `<name>.broken` (never overwritten by the defaults) and the program says where it went; every settings write lands whole (temp file, then rename). Explorer's `settings.json` and the launcher's `launcher.toml` both go through it. |
| `placement` | `restore` / `restore_physical`: where a window reopens. The outer position a program saved when it last closed (physical pixels) is put back only when enough of the title bar to grab lands on a monitor that is still connected; otherwise the desktop places the window. Explorer, the launcher and the play window all use it. |
| `release_layout` | `find_engine`: the release archive keeps every command-line program in `command-line-tools/` below the two double-click programs, and this finds `legaia-engine` there, beside the program (a `cargo build`), or inside `Legend of Legaia.app`. The `legaia-launch` stub and Explorer's Play button both use it. |

`win_icon`, `release_layout`, `drop_backend` and `settings_file` need `std`
(processes, paths, files, the environment); the palette does not. None of them adds a
dependency.

## The palette

Colours are 8-bit sRGB (`Rgb`); `Rgb::to_f32` gives the normalised,
still-sRGB-encoded `[r, g, b, 1.0]` a texture-modulating draw expects. Each
`Palette` names roles, not hues: `ground` / `band` / `card` / `stripe` for
surfaces from deepest up, `stage` for the mid-tone backdrop behind a preview
(Explorer's 3D view: a near-black mesh and a bright one both stand off it), `raised` / `hover` / `pressed` / `open` for a
button's states, `rule` for separators and `outline` for a control's edge,
`ink` / `ink_dim` / `ink_faint` for text, `accent` (the gold of the game's
title lettering) with `accent_fill` / `on_accent` for the one primary button a
screen has, and `good` / `warn` / `error` / `info` for status lines.

`DARK` is the launcher's palette, each of its original float colours rounded
to the nearest 8-bit step. `LIGHT` carries the same hues on white, with the
accent darkened until it reads there. Explorer's light theme applies only the
surface, accent, selection, error and outline roles over egui's own light
widgets; in both themes a control at rest carries the `outline` edge, without
which an unticked check box all but vanishes into the panel, and weak text
(hints such as the start page's drop line) is `ink_faint`, where egui's own
alpha-faded grey fell under 3:1. A hint is text a player is meant to read
(the launcher's key reminder, a folder line, a drop hint), so `ink_faint`
holds WCAG AA's 4.5:1 like the other text inks; it stays a visible step below
`ink_dim`, which is what marks it as a hint.

Key names are `info` (`chrome::KEY`) in all three programs: the launcher's
Controls panel, Explorer's shortcut list and the play window's `F12` panel.

The launcher and the play window's chrome draw in either palette by role (`Palette::get`, `chrome::inks`), resolved by `resolve_dark`: the player's choice, else the window system, else the desktop's preference (`desktop_prefers_dark`), else dark. Light chrome letters the disc font without its drop shadow (`Font::atlas_rgba_without_shadow`), which would smear into dark ink.

A notice line or the key hint sits over the game itself, on a plate of the palette's `ground` whose opacity is `chrome::notice_scrim_alpha`: a light plate is the more opaque of the two, because over a black screen (the title, a fade) it greys until the light inks no longer read.

## How a failed game is reported

`game_exit` is the wording the launcher and Explorer share when the game they
started stops with an error: `exit_headline` says how the process ended
(the exit code, the signal it died of on Unix, or the exception code a
Windows crash leaves), and `failure_detail` gives the reason the game's log
holds - a panic's message, the root cause of the `Error:` chain the engine
exits with, or its last `ERROR` record - with paths cut to their file name
and the line capped at `DETAIL_MAX`. A log with no failure in it gives no
reason: what a crash leaves is the chatter before it. `report_text` is what the
launcher's Ctrl+C and Explorer's *Copy report* put on the clipboard: program,
version and system, how the game ended, the reason, and the end of the log with
the home folder cut out. The play page's *Copy report* button is its twin
(`site/js/play-report.js`). The module needs `std` (an exit status, owned
strings); the palette does not.

## What the tests hold

- Text contrast on every surface it is drawn over, by the WCAG ratio:
  `ink` at least 7:1; `ink_dim`, `ink_faint`, `accent`, `good`, `warn`,
  `error` and `info` at least 4.5:1 (WCAG AA for body text), on the
  striped grid's alternate row too; `ink_faint` at least a fifth dimmer
  than `ink_dim`; `outline` at least 3:1 (the WCAG floor for a control's
  boundary); button and selected-row text, the accent on a button, and
  the primary button's label, likewise.
- The notice plate over the worst picture behind it (white behind the dark
  plate, black behind the light one): the heading and key inks at least
  4.5:1.
- The contrast function against the WCAG reference points.
- The dark tokens stay within half an 8-bit step of the launcher's original
  float values.
- `game_exit`: the error-chain, panic and crash-log readings, an audio
  underrun not read as the reason, and the signal / exit-code headlines (a
  `SIGTERM` or `SIGKILL` is the game being closed from outside, not a crash);
  a report's log tail and its removal of the home folder.
- The window-icon PNGs are 256 px RGBA and each `.ico` holds a 256 px image;
  the resource script and the windres pick; `find_engine` across the release
  folder, a build folder and a macOS bundle.
