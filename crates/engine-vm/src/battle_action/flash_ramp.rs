//! The battle **Arts announcement banner**: the layered emit + slide clock.
//!
//! PORT: FUN_801e2524
//!
//! This was read as a full-screen flash ramp, and it is not one. Decoding the
//! texel rows its emitter ([`flash_quads`], `FUN_801E2650`) addresses -
//! `etim.dat`'s third TIM through the value-readout sub-palette - shows the
//! four `ctx[+0x28B]` values selecting the four **`<word> ARTS!!` banners**:
//! `NEW` / `HYPER` / `MIRACLE` / `SUPER`. The sheet stores one `ARTS!!`, and
//! each banner is composed from its own word plus that shared tail, the two
//! halves sliding in from opposite sides of the screen to a fixed seam.
//! Layout: [`docs/formats/effect.md`](../../../../docs/formats/effect.md).
//! The "level" is that slide clock, not a brightness.
//!
//! Runs once per frame off the battle context (`_DAT_8007BD24`). Two bytes
//! drive it:
//!
//! | byte | role |
//! |---|---|
//! | `ctx[+0x28B]` | the **banner**. `0` = idle, `1..=4` = a live banner (`NEW` / `HYPER` / `MIRACLE` / `SUPER`), `5..=8` = a cancel request, `>= 9` = ignored |
//! | `ctx[+0x28C]` | the **slide clock**, `0..=0xF0`, walked up each frame while a banner is live |
//!
//! The stage byte is not a simple counter - the three bands do three
//! different things, and only the first band draws:
//!
//! * `0` returns immediately.
//! * `1..=4` runs the emit pass below and then advances the clock.
//! * `5..=8` **clears the stage byte** and draws nothing. That band is how a
//!   caller cancels a banner in flight: it writes `stage + 4` and the next
//!   frame retires it.
//! * `>= 9` returns without even clearing, so a garbage stage byte is inert
//!   rather than self-healing.
//!
//! The emit pass is four layers of the same quad emitter ([`flash_quads`]),
//! each with its own `(offset, percent, semi_transparent)` triple and all
//! sharing `stage - 1` as the emitter's position selector. Since the per-layer
//! `offset` shifts the slide clock, the four layers are the banner drawn at
//! four points of its own travel - a **ghost trail** behind the sliding word,
//! brightening `5 / 10 / 20 / 50` percent toward the real one. The first three
//! are gated on the clock being **below** a per-layer ceiling, so the trail
//! retracts as the banner lands - `0xD0` kills the innermost, `0xE0` the
//! middle, `0xF0` the outermost. The fourth is ungated and the only opaque
//! one, so a fully-arrived banner is a single opaque pair.
//!
//! The clock then advances by `frame_delta * 8` (retail `DAT_1F800393`, the
//! same per-frame scalar the move-buffer envelope uses) and saturates at
//! `0xF0` - the value that has already gated every trail layer off.
//!
//! # The raiser
//!
//! Every write that raises or retires a banner is in **`FUN_8004AD80` in
//! `SCUS_942.54`**, the staged-animation commit
//! (`see ghidra/scripts/funcs/8004ad80.txt`; the four `sb ..,0x28b(..)` sites
//! are `0x8004ADDC`, `0x8004B774`, `0x8004B80C` and `0x8004B87C`), not in the
//! battle overlay - which is why an overlay sweep for the raiser came back
//! empty. The one overlay write is the tick's own clear, `sb zero,0x28b(v0)`
//! at `0x801E263C` in `FUN_801E2524` (the `5..=8` arm [`step_flash_ramp`]
//! ports), so the byte has five writers disc-wide, not four.
//!
//! The commit reaches the banner block only for a **party** actor
//! (`actor[+0x5A] < 3`) whose staged id `actor[+0x1DA]` is the SpecialStarter
//! `0x1A` (`0x8004B6E8` bounds out anything `< 0x10`, `0x8004B6F4` bounds out
//! the monster slots, `0x8004B720` selects the id) - the same arm that freezes
//! every actor's `+0x21D` animation rate and puts the acting one at quarter
//! speed. Inside it, three writes race in retail order and the last one wins:
//!
//! | site | condition | banner |
//! |---|---|---|
//! | `0x8004B774` | `ctx[+0x28D + slot] != 0` - the per-seat flag `FUN_801EED1C` raises at `0x801EF5A8` | `3` |
//! | `0x8004B80C` | the word at `0x801F6990 + (ctx[+0x15] - 1) * 4` is non-zero | that word's low byte |
//! | `0x8004B87C` | the byte is *still* `0` | `2` |
//!
//! So the seat flag does not decide the banner on its own: the table pick
//! overwrites it when it hits. Whatever wins, `0x8004BB44` clears the slide
//! clock `ctx[+0x28C]`, which is what makes a raise restart the slide rather
//! than resume it.
//!
//! The **cancel** band is the same routine's prologue (`0x8004ADBC`): a commit
//! that lands on the actor the context is already running
//! (`actor[+0x5A] == ctx[+0x13]`) while a banner is live writes `banner + 4`
//! and clears the clock, i.e. it asks the next frame to retire the banner -
//! exactly the `5..=8` band above.
//!
//! What this module does **not** carry is the sound: each raise is followed by
//! `jal 0x8004FCC8` on a cue id `0x101` / `0x111` / `0x121` selected by
//! `0x8007BD10 + ctx[+0x13]`, which is the per-character Arts shout and lives
//! on the `legaia_art::arts_voice` path instead.
//!
//! `0x801F6990` is the arts queue-builder's per-token **side array**, written
//! by `FUN_801EED1C`'s build loop and by the Super tail-replace
//! `FUN_801EF9E4` - the same array
//! [`crate::battle_action::BattleActor::starter_marks`] already models. That
//! is what makes the middle pick an engine read rather than a missing one, and
//! it closes the banner space exactly:
//!
//! | source | mark | banner |
//! |---|---|---|
//! | the build loop accepted an art ([`crate::battle_action::BUILD_STARTER_MARK`]) | `1` | `NEW` |
//! | nothing picked | - | `2` = `HYPER` |
//! | the per-seat Super / Miracle flag | - | `3` = `MIRACLE` |
//! | the Super tail-replace ([`crate::battle_action::SUPER_STARTER_MARK`]) | `4` | `SUPER` |
//!
//! So the two mark constants are not arbitrary tags that happen to differ:
//! each *is* its banner's position index, and the "difference is load-bearing
//! exactly once, at the Attack x2 refill" note on `starter_marks` was one
//! reader short.

/// The banner `ctx[+0x28D + slot]` selects (`0x8004B774`).
pub const BANNER_SEAT_FLAG: u8 = 3;

/// The banner a starter commit falls back to when nothing else picked one
/// (`0x8004B87C`).
pub const BANNER_DEFAULT: u8 = 2;

/// The staged animation id (`actor[+0x1DA]`) whose commit raises a banner -
/// the SpecialStarter, and the same id the animation-rate freeze keys on.
pub const STARTER_ANIM_ID: u8 = 0x1A;

/// Raise the banner on a **SpecialStarter commit**, in retail's own order.
///
/// REF: FUN_8004AD80 (`0x8004B754..0x8004BB44`)
///
/// `seat_flag` is `ctx[+0x28D + slot]`, `table_pick` the low byte of the
/// non-zero word at `0x801F6990 + (ctx[+0x15] - 1) * 4` (`None` when that
/// word is zero or unresolved). Returns the new `(stage, level)` pair - the
/// level is always cleared, which is what restarts the slide.
///
/// The seat flag is applied *first* and the table pick overwrites it: that
/// ordering is the whole content of the three sites, and a port that tested
/// them as an `else` chain would show the wrong banner wherever both hit.
pub fn banner_on_starter_commit(seat_flag: bool, table_pick: Option<u8>) -> (u8, u8) {
    let mut stage = 0u8;
    if seat_flag {
        stage = BANNER_SEAT_FLAG;
    }
    if let Some(pick) = table_pick.filter(|p| *p != 0) {
        stage = pick;
    }
    if stage == 0 {
        stage = BANNER_DEFAULT;
    }
    (stage, 0)
}

/// The commit prologue's **cancel**: a live banner whose own actor commits
/// again moves into the `5..=8` retire band and restarts its clock.
///
/// REF: FUN_8004AD80 (`0x8004ADBC..0x8004ADE8`)
///
/// `None` leaves both bytes alone - the banner is idle, or this commit is not
/// the context's active actor.
pub fn banner_cancel_on_commit(
    stage: u8,
    committing_slot: u8,
    active_actor: u8,
) -> Option<(u8, u8)> {
    if stage == 0 || committing_slot != active_actor {
        return None;
    }
    Some((stage.wrapping_add(4), 0))
}

/// Stage values `1..=STAGE_DRAW_MAX` run the emit pass.
pub const STAGE_DRAW_MAX: u8 = 4;

/// Stage values `STAGE_DRAW_MAX+1..=STAGE_CANCEL_MAX` clear the stage byte
/// and draw nothing.
pub const STAGE_CANCEL_MAX: u8 = 8;

/// Ceiling the brightness level saturates at - also the value that has
/// already gated every layer off.
pub const LEVEL_MAX: u8 = 0xF0;

/// Level advance per frame is `frame_delta << LEVEL_STEP_SHIFT`.
pub const LEVEL_STEP_SHIFT: u32 = 3;

/// One layer the ramp asks the quad emitter for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashLayer {
    /// The emitter's first argument - a per-layer level offset subtracted
    /// from `ctx[+0x28C]` before the quad's vertical extent is derived.
    pub offset: u8,
    /// The emitter's second argument - a percentage the emitter scales to
    /// `0..=0xFF` (`v * 256 / 100`, clamped) and replicates into RGB.
    pub percent: u8,
    /// The emitter's third argument, which picks the GP0 code:
    /// `false` = `0x2C` (opaque textured quad), `true` = `0x2E`
    /// (semi-transparent).
    pub semi_transparent: bool,
    /// The emitter's fourth argument - `stage - 1`, its position selector.
    pub position: u8,
}

/// The four layers in retail emit order, as `(offset, percent, semi, gate)`.
/// `gate` is the exclusive level ceiling below which the layer is emitted;
/// `None` means the layer is always emitted.
const LAYERS: [(u8, u8, bool, Option<u8>); 4] = [
    (0x30, 5, true, Some(0xF0)),
    (0x20, 10, true, Some(0xE0)),
    (0x10, 20, true, Some(0xD0)),
    (0x00, 50, false, None),
];

/// What one frame of the ramp does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlashFrame {
    /// Layers to hand the quad emitter, in retail emit order.
    pub layers: Vec<FlashLayer>,
    /// New value for `ctx[+0x28B]`, or `None` to leave the byte alone.
    pub stage_out: Option<u8>,
    /// New value for `ctx[+0x28C]`, or `None` to leave the byte alone.
    pub level_out: Option<u8>,
}

/// Step the flash ramp one frame.
///
/// `frame_delta` is retail `DAT_1F800393` (idle = `1`). Returns the layers
/// to draw plus the write-backs for the two context bytes; an idle or
/// out-of-range stage yields an empty frame with no write-backs at all,
/// which is the difference between "inert" and "retired".
///
/// Driven once per battle frame by
/// `legaia_engine_core::world::World::tick_arts_banner`, so both hosts step
/// it through `World::tick`.
pub fn step_flash_ramp(stage: u8, level: u8, frame_delta: u8) -> FlashFrame {
    if stage == 0 || stage > STAGE_CANCEL_MAX {
        return FlashFrame::default();
    }
    if stage > STAGE_DRAW_MAX {
        // Cancel band: retire the flash, draw nothing, leave the level.
        return FlashFrame {
            stage_out: Some(0),
            ..FlashFrame::default()
        };
    }

    let position = stage - 1;
    let layers = LAYERS
        .iter()
        .filter(|(_, _, _, gate)| gate.is_none_or(|ceiling| level < ceiling))
        .map(|&(offset, percent, semi_transparent, _)| FlashLayer {
            offset,
            percent,
            semi_transparent,
            position,
        })
        .collect();

    let stepped = u32::from(level) + (u32::from(frame_delta) << LEVEL_STEP_SHIFT);
    let level_out = if stepped > u32::from(LEVEL_MAX) {
        LEVEL_MAX
    } else {
        stepped as u8
    };

    FlashFrame {
        layers,
        stage_out: None,
        level_out: Some(level_out),
    }
}

/// One `POLY_FT4` the layer emitter builds. Both of a pair share the colour
/// word, the GP0 code, the CLUT / texture page and the vertical extent; they
/// differ in horizontal extent and in which texels they sample.
///
/// The quad is axis-aligned - retail writes `x0 == x2`, `x1 == x3`,
/// `y0 == y1`, `y2 == y3` - so it is carried here as a rect rather than as
/// four independent corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashQuad {
    /// GP0 primitive code: `0x2C` opaque / `0x2E` semi-transparent
    /// (`0x2C | semi << 1`).
    pub code: u8,
    /// The packet colour word, `r = g = b` (`percent * 256 / 100`, capped at
    /// `0xFF`).
    pub gray: u8,
    /// Left / right screen X (retail `x0`/`x2` and `x1`/`x3`).
    pub x: (i16, i16),
    /// Top / bottom screen Y (retail `y0`/`y1` and `y2`/`y3`).
    pub y: (i16, i16),
    /// Left / right texel U inside the page.
    pub u: (u8, u8),
    /// Top / bottom texel V inside the page.
    pub v: (u8, u8),
    /// CBA - always [`crate::battle_value_readout::GLYPH_CLUT`].
    pub clut: u16,
    /// Texture page - always [`crate::battle_value_readout::GLYPH_TPAGE`].
    pub tpage: u16,
}

/// Per-position geometry: `(left_travel, seam_x, right_travel, u_left,
/// u_right, v_top, v_bottom)` for the **first** quad of the pair.
///
/// The left quad runs from `level_extent - left_travel` to `seam_x`; the right
/// quad runs from `seam_x` to `right_travel - level_extent`. So both halves
/// march toward the seam as `level_extent` grows.
const POSITIONS: [(i16, i16, i16, u8, u8, u8, u8); 4] = [
    (0x198, 0x90, 0x2D8, 0x80, 0xC7, 0xC8, 0xDF),
    (0x1AC, 0xA4, 0x2EC, 0x00, 0x6F, 0xB0, 0xC7),
    (0x1B4, 0xAC, 0x2F4, 0x00, 0x7F, 0xC8, 0xDF),
    (0x1AC, 0xA4, 0x2EC, 0x00, 0x6F, 0x98, 0xAF),
];

/// The second quad's texel rect is position-independent (retail writes it
/// before the position switch): `u 0x70..0xD7`, `v 0xB0..0xC7`.
const SECOND_QUAD_UV: (u8, u8, u8, u8) = (0x70, 0xD7, 0xB0, 0xC7);

/// The horizontal travel term: `min(level - offset + 0x30, 0xF0) * 2`.
///
/// Retail reads `level` from `ctx[+0x28C]` inside the emitter; the port takes
/// it as an argument so the routine stays a function of its inputs.
fn level_extent(level: u8, offset: u8) -> i16 {
    let raw = i32::from(level) - i32::from(offset) + 0x30;
    (raw.min(0xF0) * 2) as i16
}

/// Half the quad pair's vertical opening: `(0x1E0 - extent) * 7 / 20`.
///
/// The band is `0x90 - half` to `0xB2 + half`, so it covers the frame at
/// `level = 0` and shrinks to a 34-line strip once the level tops out.
fn vertical_half(extent: i16) -> i16 {
    let v = 0x1E0 - i32::from(extent);
    ((v * 224) / 640) as i16
}

/// Emit one layer's quad pair.
///
/// PORT: FUN_801e2650
///
/// `layer` is a row of [`step_flash_ramp`]'s output and `level` is
/// `ctx[+0x28C]` - retail re-reads the context byte per layer, so a layer
/// emitted later in the same frame still sees the pre-walk value.
///
/// Returns `None` for `position >= 4`: retail's position switch has no default
/// arm, so those calls fall straight through to the CLUT / tpage writes and
/// `AddPrim` **without setting any X**, leaving whatever the recycled packet
/// memory held. That is not a shape a port can reproduce meaningfully, and
/// [`step_flash_ramp`] only ever produces `0..=3`.
///
/// Both hosts emit these through
/// `legaia_engine_ui::battle_numerals::arts_banner_prims`, fed from
/// `World::battle_arts_banner_quads`.
pub fn flash_quads(layer: &FlashLayer, level: u8) -> Option<[FlashQuad; 2]> {
    let (left_travel, seam, right_travel, u_left, u_right, v_top, v_bottom) =
        *POSITIONS.get(usize::from(layer.position))?;
    let extent = level_extent(level, layer.offset);
    let half = vertical_half(extent);
    let y = (0x90 - half, 0xB2 + half);
    // `percent * 256 / 100`, saturated at 0xFF - retail's `0x51EB851F`
    // reciprocal multiply followed by `slti 0x100`.
    let gray = ((i32::from(layer.percent) << 8) / 100).min(0xFF) as u8;
    let code = 0x2C | u8::from(layer.semi_transparent) << 1;
    let quad = |x: (i16, i16), u: (u8, u8), v: (u8, u8)| FlashQuad {
        code,
        gray,
        x,
        y,
        u,
        v,
        clut: crate::battle_value_readout::GLYPH_CLUT,
        tpage: crate::battle_value_readout::GLYPH_TPAGE,
    };
    let (su0, su1, sv0, sv1) = SECOND_QUAD_UV;
    Some([
        quad(
            (extent - left_travel, seam),
            (u_left, u_right),
            (v_top, v_bottom),
        ),
        quad((seam, right_travel - extent), (su0, su1), (sv0, sv1)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail applies the three picks in order and lets the later one win.
    /// An `else` chain would read identically on every input but the one that
    /// matters - a seat flag raised *and* a table word present - so the order
    /// is asserted rather than the outcomes.
    #[test]
    fn the_table_pick_overwrites_the_seat_flags_banner() {
        assert_eq!(banner_on_starter_commit(false, None), (BANNER_DEFAULT, 0));
        assert_eq!(banner_on_starter_commit(true, None), (BANNER_SEAT_FLAG, 0));
        assert_eq!(banner_on_starter_commit(true, Some(4)), (4, 0));
        assert_eq!(banner_on_starter_commit(false, Some(1)), (1, 0));
        // A zero word is "no pick", not "banner 0" - retail tests the whole
        // word before reading its low byte.
        assert_eq!(
            banner_on_starter_commit(false, Some(0)),
            (BANNER_DEFAULT, 0)
        );
        assert_eq!(
            banner_on_starter_commit(true, Some(0)),
            (BANNER_SEAT_FLAG, 0)
        );
    }

    /// Every raise restarts the slide: the clock write is unconditional at
    /// `0x8004BB44`, which is why a second starter re-runs the whole slide
    /// instead of resuming a banner that had already landed.
    #[test]
    fn every_raise_clears_the_slide_clock() {
        for seat in [false, true] {
            for pick in [None, Some(1u8), Some(4)] {
                assert_eq!(banner_on_starter_commit(seat, pick).1, 0);
            }
        }
    }

    /// The prologue's cancel lands in the `5..=8` band the step arm retires,
    /// and only for the context's own active actor.
    #[test]
    fn the_cancel_moves_a_live_banner_into_the_retire_band() {
        for stage in 1u8..=4 {
            let (out, level) = banner_cancel_on_commit(stage, 2, 2).expect("own actor cancels");
            assert_eq!(out, stage + 4);
            assert_eq!(level, 0);
            assert_eq!(step_flash_ramp(out, 0x40, 1).stage_out, Some(0));
        }
        assert_eq!(
            banner_cancel_on_commit(3, 1, 2),
            None,
            "another actor does not cancel"
        );
        assert_eq!(
            banner_cancel_on_commit(0, 2, 2),
            None,
            "an idle banner has nothing to cancel"
        );
    }

    #[test]
    fn stage_zero_and_stage_nine_up_are_inert() {
        for stage in [0u8, 9, 10, 0xFF] {
            let f = step_flash_ramp(stage, 0, 1);
            assert!(f.layers.is_empty());
            assert_eq!(f.stage_out, None, "stage {stage} must not self-heal");
            assert_eq!(f.level_out, None);
        }
    }

    #[test]
    fn the_five_to_eight_band_retires_the_flash_without_drawing() {
        for stage in 5u8..=8 {
            let f = step_flash_ramp(stage, 0x40, 1);
            assert!(f.layers.is_empty());
            assert_eq!(f.stage_out, Some(0));
            // The cancel arm returns before the level walk.
            assert_eq!(f.level_out, None);
        }
    }

    #[test]
    fn layers_drop_out_one_ceiling_at_a_time() {
        let n = |level: u8| step_flash_ramp(1, level, 1).layers.len();
        assert_eq!(n(0x00), 4);
        assert_eq!(n(0xCF), 4);
        assert_eq!(n(0xD0), 3); // innermost (0x10 / 20%) gated off
        assert_eq!(n(0xDF), 3);
        assert_eq!(n(0xE0), 2);
        assert_eq!(n(0xEF), 2);
        // At the ceiling only the ungated opaque layer is left.
        let last = step_flash_ramp(1, 0xF0, 1).layers;
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].offset, 0);
        assert!(!last[0].semi_transparent);
    }

    #[test]
    fn every_layer_carries_stage_minus_one_as_its_position() {
        for stage in 1u8..=4 {
            let f = step_flash_ramp(stage, 0, 1);
            assert!(f.layers.iter().all(|l| l.position == stage - 1));
        }
    }

    #[test]
    fn level_walks_by_eight_frame_deltas_and_saturates() {
        assert_eq!(step_flash_ramp(1, 0, 1).level_out, Some(8));
        assert_eq!(step_flash_ramp(1, 0, 3).level_out, Some(24));
        // Saturates at 0xF0 rather than wrapping the byte.
        assert_eq!(step_flash_ramp(1, 0xEF, 4).level_out, Some(LEVEL_MAX));
        assert_eq!(step_flash_ramp(1, LEVEL_MAX, 1).level_out, Some(LEVEL_MAX));
    }

    /// The pair meets at the seam and marches toward it: at level `0` with the
    /// outermost layer's `0x30` offset the extent is `0`, so the left quad
    /// starts a full `left_travel` off-screen and the right quad ends a full
    /// `right_travel` off the other side.
    #[test]
    fn the_pair_slides_in_from_both_sides_and_meets_at_the_seam() {
        let layer = FlashLayer {
            offset: 0x30,
            percent: 50,
            semi_transparent: false,
            position: 0,
        };
        let [l, r] = flash_quads(&layer, 0).expect("position 0 emits");
        assert_eq!(l.x, (-0x198, 0x90));
        assert_eq!(r.x, (0x90, 0x2D8));
        // Fully ramped: extent saturates at 0xF0 * 2, closing both halves onto
        // the seam from opposite sides.
        let [l, r] = flash_quads(&layer, 0xF0).expect("position 0 emits");
        assert_eq!(l.x, (0x1E0 - 0x198, 0x90));
        assert_eq!(r.x, (0x90, 0x2D8 - 0x1E0));
    }

    /// The vertical band is widest when the extent is `0` - level `0` under
    /// the outermost layer's `0x30` offset, which cancels the emitter's own
    /// `+0x30` bias - and closes to `0x90..0xB2` once the extent tops out.
    /// A layer with a smaller offset starts part-way closed, which is what
    /// makes the four layers a trail rather than four copies.
    #[test]
    fn the_band_closes_as_the_level_ramps() {
        let at = |offset: u8, level: u8| {
            flash_quads(
                &FlashLayer {
                    offset,
                    percent: 50,
                    semi_transparent: false,
                    position: 0,
                },
                level,
            )
            .expect("emits")[0]
                .y
        };
        let open = at(0x30, 0);
        assert_eq!(open, (0x90 - 168, 0xB2 + 168));
        assert!(open.0 < 0 && open.1 > 240, "band starts off both edges");
        // The innermost layer (offset 0) is already 96 extent units in at the
        // same level, so its band is narrower.
        assert_eq!(at(0x00, 0), (10, 312));
        assert_eq!(at(0x30, 0xF0), (0x90, 0xB2));
    }

    /// Both quads of a pair share the colour word, the code and the atlas
    /// coordinates; only the semi flag moves the code.
    #[test]
    fn code_and_colour_follow_the_layer() {
        use crate::battle_value_readout::{GLYPH_CLUT, GLYPH_TPAGE};
        for (percent, gray) in [(5u8, 12u8), (10, 25), (20, 51), (50, 128), (100, 0xFF)] {
            for semi in [false, true] {
                let layer = FlashLayer {
                    offset: 0,
                    percent,
                    semi_transparent: semi,
                    position: 1,
                };
                for q in flash_quads(&layer, 0x40).expect("emits") {
                    assert_eq!(q.gray, gray, "percent {percent}");
                    assert_eq!(q.code, if semi { 0x2E } else { 0x2C });
                    assert_eq!(q.clut, GLYPH_CLUT);
                    assert_eq!(q.tpage, GLYPH_TPAGE);
                }
            }
        }
    }

    /// Each position selects its own seam + texel row; the second quad's rect
    /// is position-independent because retail writes it before the switch.
    #[test]
    fn each_position_picks_its_own_row_and_seam() {
        let seams = [0x90i16, 0xA4, 0xAC, 0xA4];
        let rows = [(0xC8u8, 0xDFu8), (0xB0, 0xC7), (0xC8, 0xDF), (0x98, 0xAF)];
        for position in 0u8..4 {
            let layer = FlashLayer {
                offset: 0,
                percent: 50,
                semi_transparent: true,
                position,
            };
            let [l, r] = flash_quads(&layer, 0).expect("emits");
            assert_eq!(l.x.1, seams[usize::from(position)]);
            assert_eq!(r.x.0, seams[usize::from(position)]);
            assert_eq!(l.v, rows[usize::from(position)]);
            assert_eq!(r.v, (0xB0, 0xC7));
            assert_eq!(r.u, (0x70, 0xD7));
        }
    }

    /// Retail's switch has no default arm, so a position past the table draws
    /// with stale X. The port refuses instead of inventing one.
    #[test]
    fn positions_past_the_table_emit_nothing() {
        for position in [4u8, 5, 0xFF] {
            let layer = FlashLayer {
                offset: 0,
                percent: 50,
                semi_transparent: true,
                position,
            };
            assert!(flash_quads(&layer, 0).is_none());
        }
    }

    /// Every layer the ramp hands out is emittable - the two routines agree on
    /// the position space.
    #[test]
    fn every_layer_the_ramp_emits_is_a_valid_position() {
        for stage in 1u8..=STAGE_DRAW_MAX {
            for level in [0u8, 0x40, 0xCF, 0xF0] {
                for layer in step_flash_ramp(stage, level, 1).layers {
                    assert!(flash_quads(&layer, level).is_some(), "stage {stage}");
                }
            }
        }
    }

    #[test]
    fn a_live_flash_never_rewrites_its_own_stage_byte() {
        // Only the cancel band touches +0x28B; the draw band walks the
        // level and leaves the stage for its caller to advance.
        for stage in 1u8..=4 {
            assert_eq!(step_flash_ramp(stage, 0x10, 1).stage_out, None);
        }
    }
}
