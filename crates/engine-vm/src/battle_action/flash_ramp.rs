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
//! # NOT WIRED
//!
//! The retail per-frame caller is the battle draw tick `FUN_800480D8`
//! (`jal 0x801e2524` at `0x80048140`, `ghidra/scripts/funcs/800480d8.txt`),
//! whose own port - `engine-render::battle_actor_tick` - is a schedule with no
//! host yet, so the wire's location is known but does not exist. The ramp is
//! driven entirely by the two battle-context bytes `ctx[+0x28B]` (banner) and
//! `ctx[+0x28C]` (clock), and `BattleActionCtx` carries neither - nothing in
//! the port can raise a banner or hold its clock between frames. Retail calls
//! the ramp unconditionally every frame and lets the stage byte gate it, so
//! the missing piece is not the per-frame call but the **raiser**.
//!
//! Now that the four positions are identified, the raiser's engine-side home
//! is too: the Super / Miracle chain match in `engine-core`'s battle command
//! flow (`resolve_arts_input_entry`, which already calls `miracle_for_chain` /
//! `super_for_chain`) is where a recognized chain would set `1..=4`. Retail's
//! own writer is still unfound in the battle overlay, so a port that raises it
//! there is choosing the trigger rather than reproducing one.

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
/// NOT WIRED: same reason as [`step_flash_ramp`] - the ramp that supplies
/// these layers is never stepped, because nothing in the port raises the
/// stage byte `ctx[+0x28B]`.
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
