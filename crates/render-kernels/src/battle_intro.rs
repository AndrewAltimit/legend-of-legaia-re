//! The field-to-battle transition's per-frame emitter: the working-set owner
//! that stands between the wired transition clock and the ordering table.
//!
//! # What was missing, and what this is
//!
//! Everything either side of this module was already built. On the simulation
//! side `crate::battle_intro_transition::tick_transition` is live -
//! `legaia_engine_core::World::tick_encounter` runs it every frame the
//! encounter session sits in its `Transition` phase - and all five style
//! kernels are ported. On the render side [`crate::screen_prim`] is the
//! ordering table, the native renderer's `SceneWithScreenPrims` target
//! composites it over a drawn scene, and [`crate::vram_capture`] lands a
//! drawn frame back in the software VRAM the strips sample.
//!
//! What nothing owned was the thing in between: a per-frame, per-style object
//! that holds the working set across frames, advances it off the transition's
//! own clock, and turns the result into primitives. Retail keeps that state in
//! the transition entity's `+0x48` block and emits straight into the OT
//! cursor. [`BattleIntro`] is that owner.
//!
//! # All five styles emit
//!
//! `FUN_801CF5BC`'s first tail switch dispatches five styles
//! ([`IntroStyle`]), and each reaches a primitive here through its own retail
//! packet builder:
//!
//! | Style | Retail packet builder | Port |
//! |---|---|---|
//! | [`IntroStyle::ScatterParticles`] | `FUN_801CFDA0` | [`emit_particle_field`] |
//! | [`IntroStyle::SpinUpParticles`] | `FUN_801D0370` + tail `FUN_801D1CFC` | [`emit_particle_field`] + [`emit_spinup_ring`] |
//! | [`IntroStyle::TileShatter`] | `FUN_801D0E54` via `FUN_80043390` | [`emit_tile`] |
//! | [`IntroStyle::Curtain`] | `FUN_801CF1B0` | [`intro_quad_to_screen`] |
//! | [`IntroStyle::Swirl`] | `FUN_801D1A20` via `FUN_80043390` / `FUN_80029888` | [`emit_swirl_band`] |
//!
//! The curtain's builder produces **screen-space** corners with texture page,
//! CLUT, UVs and a top/bottom colour pair, so there is no projection step to
//! invent; its descriptor table is disc data that parses
//! ([`IntroQuadTable`]), and the texture pages its two passes name decode to
//! exactly the rects [`crate::vram_capture`] captures into.
//!
//! The other four all end in the same GTE projection this module reproduces
//! ([`project_intro_corner`] + the FT4 handler's accept chain in
//! [`push_ft4_quad`]). The tile shatter and the swirl both assemble a
//! synthetic Legaia-TMD object in the `_DAT_8007B85C + 0x5DC00` scratch block
//! and hand it to the generic per-prim dispatcher; the two particle styles
//! write `POLY_FT4` packets straight into the ordering-table cursor and
//! project through `FUN_8005BAC8` (RotTransPers4; its return is `SZ3 >> 2`,
//! which is the depth [`crate::battle_intro_styles::particle_quad_accepted`]
//! bounds).
//!
//! Two retail nuances are *not* carried, both stated rather than hidden. The
//! dispatcher runs a moving tile's opaque faces - and the whole late-phase
//! swirl - through a depth-cue bank (fade toward a per-caller far colour),
//! which the screen overlay has no channel for: receding tiles keep their
//! face grey, and the late swirl keeps its packet colours instead of hazing
//! toward the mid-grey far colour `FUN_80029888` stages. And the spin-up
//! ring's mesh comes from `FUN_80028158`, a 1395-instruction multi-shape
//! generator of which only the case-0 annulus parameters are modelled - see
//! [`emit_spinup_ring`].
//!
//! [`IntroFrame::style_drawn`] still reports per frame whether the style's own
//! geometry reached the list, as against the frame carrying only the fade
//! quad; every style's first frame is a deliberate no-draw (see the
//! stale-view-matrix note on [`BattleIntro::tick`]'s tile arm).
//!
//! # The curtain is a two-pass render-to-texture, and only one pass is on screen
//!
//! The curtain's row pass samples texture pages `0x105` / `0x108` and its
//! column pass `0x115` / `0x118`. Those decode to 15-bpp pages at VRAM
//! `(320, 0)` / `(512, 0)` and `(320, 256)` / `(512, 256)`. It is tempting to
//! read that as "two identical copies of the capture, one per pass", and this
//! module used to. `FUN_801D11D0`'s own draw-environment packets say otherwise.
//!
//! Between the two passes retail pushes `SetDrawArea` / `SetDrawOffset` pairs
//! into the ordering table, and their OT buckets order them against the strips:
//!
//! | OT bucket | what retail links there |
//! |---|---|
//! | `0x1F4` (500) | `SetDrawOffset(0, 0)` + `SetDrawArea(320, 0, 320, 240)` |
//! | `0x1EA` (490) | `FUN_801D1D9C(0x1EA, 2, 0x808080)` - the mid-pass emitter |
//! | `0x1C2` (450) | the **column** strips ([`styles::CURTAIN_COL_OT_DEPTH`]) |
//! | `0x190` (400) | `SetDrawArea(0, y, 320, h)` + `SetDrawOffset(0, y)` - the back buffer |
//! | `0x12C` (300) | the **row** strips ([`styles::CURTAIN_ROW_OT_DEPTH`]) |
//!
//! A higher OT index draws first, so the column pass runs with the draw area
//! pointed at VRAM `(320, 0)` - which is [`FIELD_CAPTURE_ROWS`], the very rect
//! the row pass then samples. The column strips are not on screen at all: they
//! render an intermediate, and the row pass slices *that* into the display.
//! `styles::CURTAIN_COL_DRAW_BIAS` (`0x1E0`) is what makes it fit - a column
//! that passes the visibility test (which re-centres on `0xA0`) lands at
//! `x` in `320..640`, exactly the installed draw area, with the offset at
//! `(0, 0)` so primitive coordinates are absolute VRAM.
//!
//! So the capture goes into [`FIELD_CAPTURE_COLS`](crate::vram_capture::FIELD_CAPTURE_COLS) **only**, and
//! [`BattleIntro::refresh_captured_page`] re-composes the intermediate every
//! frame. The port rasterises the column pass on the CPU
//! ([`compose_curtain_intermediate`]) because a screen-space quad list has no
//! render-to-VRAM target; the arithmetic is the same quad
//! `crate::battle_intro_transition::build_intro_quad` produced.
//!
//! The mid-pass emitter is established now. `FUN_801D1D9C` is dumped from the
//! `overlay_field_battle_intro` image itself
//! (`ghidra/scripts/funcs/overlay_field_battle_intro_801d1d9c.txt` - the
//! earlier note that it existed only at a VA aliasing another overlay is
//! retired), and it is `FUN_80024EE4`'s shape pointed at the intermediate: a
//! five-word `0x2B` (untextured, semi-transparent) quad over
//! `x 0x140..0x140+W, y -4..H` - the display halfwords `_DAT_1F80038C` /
//! `_DAT_1F80038E` biased one screen right - preceded by a
//! `SetDrawMode((abr << 5) | 0xE)` packet at the same OT layer. With the
//! curtain's arguments `(0x1EA, 2, 0x808080)` that is a **subtractive decay of
//! `0x80` per channel per frame over the whole intermediate**, between the
//! draw-area install at `0x1F4` and the column strips at `0x1C2` - so a column
//! the warp has culled leaves a ghost that reaches black in two frames rather
//! than vanishing. [`BattleIntro`] carries it: the intermediate persists
//! across frames and decays by [`CURTAIN_MIDPASS_DECAY_5`] instead of being
//! cleared.
//!
//! The **display** side of the same no-clear law is carried for the curtain
//! too. `FUN_801D11D0` re-arms the screen wash `FUN_8004695C(0x80808)` at the
//! top of **every** frame (`0x801D1228..0x801D1230`, unconditional), and
//! nothing clears the display buffer during the transition - so the gap a
//! departing row strip leaves shows the previous frames' pixels decaying by 8
//! per 8-bit channel per frame (~31 frames to black), not flat black.
//! [`BattleIntro`] models that display buffer on the CPU
//! ([`CURTAIN_TRAIL_RECT`] holds it), seeds it from the same field capture
//! retail's init lands in both display buffers, decays it by
//! [`CURTAIN_DISPLAY_DECAY_5`] each frame, and emits it as textured backdrop
//! quads behind the live row strips. Two disclosed approximations: the wash
//! drain (`FUN_80046978`) scales its constant by the scratchpad brightness
//! byte, taken here at full brightness; and retail's display is
//! double-buffered, so its per-buffer decay may interleave at half this rate -
//! settling that needs a retail frame capture of a curtain formation
//! (hypothesis, graded inference).

use crate::battle_intro_styles::IntroStyle;
use crate::vram_capture::{FIELD_CAPTURE_LAND, VramRect};

mod curtain;
mod particles;
mod quad_table;
mod screen_quads;
mod session;
mod spinup_ring;
mod swirl_band;
mod tick;
mod tile_projection;

pub use curtain::*;
pub use particles::*;
pub use quad_table::*;
pub use screen_quads::*;
pub use session::*;
pub use spinup_ring::*;
pub use swirl_band::*;
pub use tile_projection::*;

/// The 4bpp "edge shade" texture page the tile shatter stretches over each
/// box side face: `tpage 0x0027` = VRAM `(448, 0)`, 64x64 texels, so 16
/// halfwords wide and 64 rows tall.
///
/// It matters here because it lies **inside** [`FIELD_CAPTURE_ROWS`]
/// (`320..640` x `0..240`). Capturing the field frame into that rect
/// overwrites the page, so a style that needs the shade texture must not have
/// the rows rect written under it - see [`capture_rects_for`].
pub const TILE_SHADE_PAGE: VramRect = VramRect::new(448, 0, 16, 64);

/// Which capture rects a style's own primitives actually sample.
///
/// The capture is not free of consequences: it writes over whatever else
/// lives in the destination rect, and the two rects are not equally safe.
///
/// | Style | Samples | Rects written |
/// |---|---|---|
/// | [`IntroStyle::Curtain`] | column pass `0x115`/`0x118` at `(320,256)`/`(512,256)`; the row pass' `0x105`/`0x108` at `(320,0)`/`(512,0)` is the column pass' **output** | columns only |
/// | [`IntroStyle::TileShatter`] | `0x135`/`0x137` at `(320,256)`/`(448,256)`, plus the 4bpp [`TILE_SHADE_PAGE`] at `(448,0)` | columns only |
/// | the particle styles | `0x135..=0x139` at `(320,256)..(576,256)` (u < 64, so the five pages tile the 320 capture columns) | columns only |
/// | [`IntroStyle::Swirl`] | `0x115`/`0x117` at `(320,256)`/`(448,256)` | columns only |
///
/// **No style writes the rows rect**, and for two different reasons. For the
/// curtain the rows rect is the intermediate its column pass renders into (see
/// the module docs) - blitting the capture there would be overwritten by the
/// first column strip anyway, and until it was it would show the transition an
/// un-warped frame. For the tile shatter it is load-bearing the other way: its
/// own pages are wholly inside the column rect and the 4bpp
/// [`TILE_SHADE_PAGE`] it also needs is inside the *row* rect, so writing the
/// rows would destroy an input it depends on while gaining it nothing. The
/// particle and swirl pages are likewise wholly inside the column rect (every
/// page they name carries the `y = 256` bit).
///
/// Inside the column page the frame lands at [`FIELD_CAPTURE_LAND`] - four
/// rows down, retail's drawn-row-to-`v` offset
/// ([`crate::vram_capture::FIELD_CAPTURE_V_OFFSET`]) - so a style's texel
/// coordinates name the drawn pixel they name on the console.
pub fn capture_rects_for(_style: IntroStyle) -> &'static [VramRect] {
    const COLS_ONLY: [VramRect; 1] = [FIELD_CAPTURE_LAND];
    &COLS_ONLY
}
