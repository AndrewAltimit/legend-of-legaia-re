//! Battle-context screen-flash ramp: the layered emit + brightness walk.
//!
//! PORT: FUN_801e2524
//!
//! Runs once per frame off the battle context (`_DAT_8007BD24`). Two bytes
//! drive it:
//!
//! | byte | role |
//! |---|---|
//! | `ctx[+0x28B]` | the **stage**. `0` = idle, `1..=4` = a live flash, `5..=8` = a cancel request, `>= 9` = ignored |
//! | `ctx[+0x28C]` | the **brightness level**, `0..=0xF0`, walked up each frame while a flash is live |
//!
//! The stage byte is not a simple counter - the three bands do three
//! different things, and only the first band draws:
//!
//! * `0` returns immediately.
//! * `1..=4` runs the emit pass below and then advances the level.
//! * `5..=8` **clears the stage byte** and draws nothing. That band is how a
//!   caller cancels a flash in flight: it writes `stage + 4` and the next
//!   frame retires it.
//! * `>= 9` returns without even clearing, so a garbage stage byte is inert
//!   rather than self-healing.
//!
//! The emit pass is four layers of the same quad emitter (`FUN_801E2650`),
//! each with its own `(offset, percent, semi_transparent)` triple and all
//! sharing `stage - 1` as the emitter's position selector. The first three
//! layers are gated on the level being **below** a per-layer ceiling, so as
//! the flash brightens the layers drop out one at a time - `0xD0` kills the
//! innermost, `0xE0` the middle, `0xF0` the outermost. The fourth layer is
//! ungated and is the only opaque one, so a fully-ramped flash is a single
//! opaque quad.
//!
//! The level then advances by `frame_delta * 8` (retail `DAT_1F800393`, the
//! same per-frame scalar the move-buffer envelope uses) and saturates at
//! `0xF0` - the value that has already gated every layer off.
//!
//! What is *not* ported here is `FUN_801E2650` itself: the quad emitter that
//! turns one of these layers into two `POLY_FT4`s with a position picked by
//! `stage - 1`. This module hands the layers back as a list so the emitter
//! can be ported behind it without disturbing the ramp law.
//!
//! # NOT WIRED
//!
//! Two prerequisites are missing on the engine side. The ramp is driven
//! entirely by the two battle-context bytes `ctx[+0x28B]` (stage) and
//! `ctx[+0x28C]` (level), and `BattleActionCtx` carries neither - nothing in
//! the port can raise a flash or hold its level between frames. And the layer
//! list it returns is an argument list for `FUN_801E2650`, which is not
//! ported, so even a driven ramp would have no primitive sink: the engine's
//! full-screen effects are the `engine_core::fade` kernel, a different
//! mechanism with no per-layer quad emitter behind it.

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

    #[test]
    fn a_live_flash_never_rewrites_its_own_stage_byte() {
        // Only the cancel band touches +0x28B; the draw band walks the
        // level and leaves the stage for its caller to advance.
        for stage in 1u8..=4 {
            assert_eq!(step_flash_ramp(stage, 0x10, 1).stage_out, None);
        }
    }
}
