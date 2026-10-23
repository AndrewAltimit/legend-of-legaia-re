# legaia-engine-title

The boot front end's `World`-free kernels: the publisher-logos phase that
runs off `init.pak`, the title screen state machine, the title overlay's
per-frame tick and the title-art atlas. Free of wgpu, winit and cpal, so it
builds for native and `wasm32` alike. Hosts and tests name its modules
directly (`legaia_engine_title::title::TitleSession`).

## What lives here

| Module | Covers |
|---|---|
| `publisher_logos` | The publisher-logos boot phase off the four TIMs in PROT 0895 (`FUN_801CE9C0` + sequencer `FUN_801CEFD4`): atlas build and the per-frame logo quads. |
| `title` | `TitleSession`, the title state machine both hosts run: `FadeIn -> PressStart -> MainMenu -> Done`, with a no-save fallback. |
| `title_overlay` | The title-screen tick `FUN_801DD35C` (the 25-entry sub-mode dispatcher, its menu and transitions) and `title_overlay::title_draw_list`, the sub-mode -> screen selector both hosts ask. |
| `title_prim` | The SCUS-side GPU primitive helpers the title tick calls (`FUN_80058298`, `FUN_80058490`, `FUN_800198E0`) behind the `PrimHost` trait. |
| `title_screen_atlas` | RGBA decode of the title TIM and its per-frame draw helper. |

Layout, sub-mode table and provenance: [`docs/subsystems/boot.md`](../../docs/subsystems/boot.md).

## The split line

These modules name each other, `legaia-engine-system` (the mode-entry
words and the cutscene-trigger constants), `legaia-tim` and `legaia-ui-formats` -
no other engine crate. No menu module names them either: the title tick and
the pause / shop / save screens share no state. The save-slot screen the
title's Continue opens is `legaia-engine-menus`' `save_select`, which the
hosts reach on their own.

The `FUN_801DD35C` image sits in the menu overlay (PROT 0899), but the
routine's own sub-mode and master-mode stores make it the title's
dispatcher, not the menu's (`title_overlay`'s module comment carries the
operands).

What lives elsewhere: the game-mode seat that hands the title its row
counter is `legaia-engine-mode`; the hosts' title draws are `legaia-engine-ui` and
`legaia-engine-screens`.

## See also

- [`crates/engine-menus`](../engine-menus/README.md) - the menu and
  memory-card front end beside this crate.
- [`docs/subsystems/boot.md`](../../docs/subsystems/boot.md) - boot
  sequence, publisher logos and the title overlay.
