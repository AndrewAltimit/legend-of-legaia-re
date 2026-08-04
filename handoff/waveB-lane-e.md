# LANE E handoff - screen-space PSX primitives on the browser host

## What changed (one sentence)

The screen-space PSX primitive model - the record, the ABR selector, the
ordering-table sort and the vertex builder - moved into `engine-ui`, and the
browser play page grew a WebGL2 pass that consumes it, so both hosts now draw
PSX `POLY_FT4` / `POLY_GT4` quads out of one model instead of one host having
the capability and the other having no type for it.

## The five items, item by item

The list is `docs/tooling/host-drift.md` § "Screen-space PSX primitives"
(the section is now retitled *across the two hosts* - it stopped being a
speculation).

| # | item | status |
|---|---|---|
| 1 | a draw record that is a PSX primitive | **landed** - lifted, not re-invented |
| 2 | an ordering-table sort, shared | **landed** - and the API removes the second place to sort |
| 3 | a WebGL2 fragment path with the same CLUT decode | **landed** |
| 4 | four ABR blend modes | **landed** - reusing the page's own table, not a copy |
| 5 | a framebuffer capture | **not landed**, and it is not next - see below |

### 1 + 2: the shared record and the shared sort

`crates/engine-ui/src/screen_prim.rs` (new) carries:

- `ScreenPrim` / `ScreenQuad` / `FlatQuad` - four corners, four `(u, v)` pairs,
  a `(cba, tsb)` pair, flat **or** per-vertex colour, a semi-transparency flag,
  an OT index;
- `abr_mode(tsb)` - TSB bits 5..=6;
- `order_primitives` - `AddPrim` + `DrawOTag`: descending OT index, LIFO ties;
- `ScreenVertex` + `SCREEN_VERTEX_STRIDE` + the five `SCREEN_VERTEX_OFF_*`
  offsets - one byte layout, read as bytes by wgpu and by WebGL2 alike;
- `build_geometry` / `OverlayGeometry` (+ `vertex_bytes()` and `run_words()`);
- `PSX_DISPLAY_W` / `_H` and the display-rect packet builders
  `display_rect_flat_quad` / `fade_prim`.

Every one of those was moved out of `engine-render::screen_overlay` verbatim,
not rewritten. `screen_overlay` is now a ~130-line shim: it re-exports the whole
module at its old path (so every native call site and test reads unchanged),
keeps `afterimage_screen_quad` (it needs `crate::afterimage`), and pins the
display-rect constants against `vram_capture::PSX_SCREEN_WIDTH/HEIGHT` with a
`const _: () = assert!(...)` rather than a comment.

**The sort has no gate and does not need one.** Neither host is ever handed a
primitive list. `build_geometry` is the only public route from `&[ScreenPrim]`
to something drawable and it runs the OT walk itself, so by the time a host sees
the data the order is baked into the index buffer. A divergence needs two places
that sort; there is one.

`battle_intro::fade_quad` / `wash_prim` / `backdrop_prim` are now thin adapters
over the shared display-rect builders. That is what lets the browser emit the
*same packet* rather than its own.

### 3 + 4: the page's pass

`ScreenPrimPass` in `site/js/play-app.js`, drawn from `_drawScreenPrims(rt)`
right after `renderAssembled` and before the 2D overlay canvas blit.

- Fragment path: the 3D VRAM-mesh decode with the texture-window remap dropped
  (screen sprites never use GP0(E2)) - 4/8/15-bpp pages out of the same
  1024x512 R16UI VRAM texture the 3D pass already uploads, CLUT through CBA,
  BGR555 -> RGBA, `0x0000` discarded. `flags` bit 0 picks textured vs flat.
- Blend: **`TmdRenderer._setSemiBlend`**, the page's existing four-ABR table.
  A second copy in `play-app.js` is exactly the drift this lane exists to stop,
  so the pass borrows the table, the GL context and the VRAM texture handle from
  the live `TmdRenderer` and owns none of the three. `blendColor` carries mode
  0's `0.5` and mode 3's `0.25`, which is why the browser needs no shader
  pre-scale where the native pipeline uses one.
- Failure is contained: a driver that cannot link the pass latches
  `_screenPrimBroken` and the session keeps playing without the effect.

Transport: `play_screen_prim_count` (the early-out), then
`play_screen_prim_vertex_bytes` / `_indices` / `_runs`. The run table is
`[class_code, index_start, index_count]` triples with `class_code` from
`BlendClass::code` - `0` opaque, `1 + abr` semi - so an ABR-0 run
(`0.5B + 0.5F`) can never be read as "no blending". That encoding lives in
`engine-ui` next to the enum, not at either call site.

### 5: why the capture is not the next item

The brief's ordering has one link missing, and this is the lane's main finding.

Item 5 assumes the browser has something to texture with the captured frame.
It does not, and would not even with the FBO: **the emitters live in
`engine-render`**, which links wgpu, and `web-viewer` does not depend on it.
`engine-vm`'s `battle_intro_styles` / `_swirl` / `_tiles` / `_transition` are
wgpu-free and the browser already links them, but the thing that turns them into
primitives - `engine_render::battle_intro` - reaches `crate::gte`,
`crate::billboard`, `crate::screen_overlay` and `crate::vram_capture`. Only
`update_field_capture` genuinely needs a `&Renderer`; `BattleIntro::tick` is
pure. So the real order after item 4 is:

1. **an emitter the browser can link** (a wgpu-free home for `battle_intro` +
   `gte` + `billboard` + the `vram_capture` rect/blit arithmetic), then
2. the framebuffer capture.

Closing either alone changes nothing on screen. I did not do (1): it moves four
modules across a crate boundary, and `engine-render/src/{gte,billboard,
vram_capture}.rs` are outside this lane's scope.

## What the page draws today, exactly

One primitive per transition frame: the **full-screen fade quad**, resolved by
the shared `battle_intro_styles::intro_fade` ramp off the live transition
entity's clock and built by the shared `fade_prim`. The style selection
(`select_intro_style`) reads the same three inputs the native window's
`arm_battle_intro` reads - the formation's first monster id, the row's
per-battle flags byte, the scene's PROT base - so the two hosts fade on the same
ramp with the same ABR mode.

What it does **not** draw:

- any style body: confetti, tile shatter, curtain strips, swirl fan. Blocked on
  the two items above, both of them.
- the native emitter's `backdrop_prim`. **Deliberate, and a divergence.** That
  opaque black display-rect quad stands in for "retail's field renderer is not
  in the ordering table"; it is only correct underneath a style body that
  reconstructs the frame out of the capture. Emitted alone it would black the
  field out for the whole 132-frame window - strictly worse than the cut this
  lane replaced. Drawing the fade over the still-rendering field is the honest
  subset. Recorded in `host-drift.md`, not left in a comment.
- the afterimage streak and any `screen_fx` widget. The *pass* would draw them;
  nothing on the browser emits them yet.

## Gate + doc changes

- **Tier 7 rule added**, `screen-space fade quad`: trigger
  `\bintro_fade\s*\(`, requires `\bfade_prim\b`. Two surfaces run it
  (`engine-render/src/battle_intro.rs`, `web-viewer/src/play_battle.rs`), zero
  blocked, zero exempt. It keys on the **ramp call**, not on `IntroFade`: a
  file that has just resolved this frame's fade is the file about to build - or
  hand-roll - the packet, whereas a type trigger fires on every file that passes
  one along. The specific defect it guards is real and has been made once: the
  fade's ABR mode travels beside an OT layer that looks exactly like it, and
  reading one as the other puts every style on `0.5B + 0.5F`.
- `docs/tooling/host-drift.md`: the two old "Known gaps" rows collapse into one
  (`field-to-battle style bodies on web`) plus a new
  `framebuffer readback on web` row, since that is now a blocker in its own
  right and not a sub-item; the section is retitled and rewritten around what is
  shared vs. what is still missing.
- `docs/subsystems/renderer.md`: the ordering-table pass section now says where
  the model lives and why the sort is unreachable except through
  `build_geometry`.

## What I ran, and what I saw

Headless Chromium (playwright-core, SwiftShader) against `site/` on
`localhost:8749`, with the real disc through the page's own file input:
`__playEnter('cave01')` -> `debug_force_battle(-1)` -> free-running rAF loop,
with an in-page recorder wrapped around `PlayView._drawScreenPrims` so the
sample rate is one row per *drawn frame* rather than one per node-side poll.

Measured over one transition:

| observation | value |
|---|---|
| drawn frames sampled | 49 |
| frames carrying a screen primitive | **7**, all in `SceneMode::Field` |
| primitives per such frame | 1 |
| run table | `[3, 0, 6]` - class 3 = `Semi(2)`, i.e. **ABR 2 (`B - F`)**, one quad |
| vertex bytes | 176 = 4 x the shared 44-byte `ScreenVertex` stride |
| after the window | `SceneMode::Battle`, "3D render built (2 actor meshes...)" |

The page runs ~6 fps under SwiftShader and the tick loop caps at 4 sim ticks
per drawn frame, so 7 drawn frames is **28 sim ticks** - which is exactly
`INTRO_FADE_RAMPS[TileShatter].lead = 0x1C`, and the ABR the run table carries
is exactly that row's `abr: 2`. The disc-derived ramp and the drawn output
agree without either having been fitted to the other.

On screen, in order: the lit cave01 floor with Vahn standing on it; the same
floor **visibly darkened** (frozen on the first fade frame); a fully black
frame later in the ramp; the Theeder battle with its stage, monster mesh, HUD
and the Begin/Run chips. The fade is the only thing on the page that emits a
screen primitive, and the recorder confirms it emitted on exactly those frames.

Two notes on capturing it, both cost a run before they were understood:

- **A SwiftShader element screenshot lags the draw by ~1 s**, which at ~6 fps
  is several frames - a shot taken on the first fade frame came back showing
  the battle. The fix is to freeze the view *inside* the draw hook so the
  canvas holds the frame the recorder reported, not to shoot faster.
- **`PlayView._frame` gates the tick on `advance` but not the draw**, so while
  paused the scene is re-drawn from frozen state every frame and the screen-prim
  pass composites over a fresh frame each time. That is why a frozen fade frame
  is a faithful single-frame composite rather than an accumulating one - and
  also why a frame counter kept inside the draw hook runs away while paused.

**The native window was not screenshotted.** Nothing about its behaviour
changed: `fade_quad` / `wash_prim` / `backdrop_prim` became adapters over the
shared display-rect builders, and the emitter oracle
(`engine-render/src/tests/battle_intro_emitter.rs`) already pins each one's
rect, colour, `semi_transparent`, ABR mode and OT bucket. Those tests pass.

## Left for someone else

- Hoist `battle_intro` + `gte` + `billboard` + the `vram_capture` blit
  arithmetic into a wgpu-free crate. That is the whole remaining transition gap
  and it is a mechanical move, not a re-derivation - `BattleIntro::tick` is
  already pure.
- The FBO capture, after that.
- `cargo fmt --all -- --check` fails on `crates/engine-core/src/
  man_field_scripts/npc_motion.rs` on this branch. **Pre-existing, not this
  lane** (`git diff --name-only` never lists it); `engine-core` was off limits
  here, so it is left alone.

## A gate blind spot found on the way

`check-wasm-freshness.py` builds its source closure from `git ls-files`, so a
**new, not-yet-staged** source file is invisible to it. With
`crates/engine-ui/src/screen_prim.rs` untracked the gate reported
`OK - site/wasm/ matches 875 sources` against a bundle built before the file
existed; `git add` alone flipped it to `STALE ... added
crates/engine-ui/src/screen_prim.rs`. Nothing to fix in the checker (untracked
files are not a build input the repo can reason about), but the working order
matters and is worth stating: **stage first, then check freshness, then
rebuild**. Checking before staging answers about a closure that does not yet
include the new file.
