//! Two small simulation kernels of the PROT 0977 `other_game` overlay - the
//! mode-24 sub-id-5 **arena door/init slot** whose contest settlement is
//! [`crate::muscle_dome::settle_contest`].
//!
//! The overlay's per-frame update drives a set of counters, scales each
//! frame's step through [`step_scale`], and fires a positional SFX cue
//! through [`sfx_cue`]; the visible half is the sprite/decimal HUD ported in
//! `legaia_engine_ui::other_game_hud`.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_0977_other_game_801d14b0.txt`
//! and `..._801d1288.txt`; ported from the disassembly.
//!
//! # NOT WIRED
//!
//! The engine has no PROT 0977 host. Its `muscle_dome` session models the
//! arena's *match* rules (which live in the battle overlay, not here) and
//! never loads the 0977 door/init module, so nothing owns the counters
//! [`step_scale`] pace or the rotating voice slot [`sfx_cue`] walks. Wiring
//! needs the arena scene host that loads 0977 in the first place.

/// Threshold above which the unslowed step is divided by five.
pub const STEP_FAST_MIN: i32 = 6;

/// Threshold below which the step collapses to one.
pub const STEP_MIN_FLOOR: i32 = 3;

/// Scale one frame's step.
///
/// `boost` is the overlay flag `DAT_801D1AB4`: while it is set the step is
/// passed through untouched. Otherwise the step is *slowed*, in three bands
/// read straight off the branch order in the disassembly:
///
/// | input | result |
/// |---|---|
/// | `> 5` | `input / 5` |
/// | `3 ..= 5` | `input / 2` |
/// | `< 3` | `1` |
///
/// Both divisions truncate toward zero (the retail code uses the
/// `0x66666667` reciprocal for `/5` and an arithmetic shift for `/2`), so a
/// negative input in the middle band rounds toward zero as well - and any
/// input below `3`, negative ones included, returns `1`.
///
/// PORT: FUN_801d14b0
#[inline]
pub fn step_scale(step: i32, boost: bool) -> i32 {
    if boost {
        return step;
    }
    if step >= STEP_FAST_MIN {
        step / 5
    } else if step < STEP_MIN_FLOOR {
        1
    } else {
        step / 2
    }
}

/// Number of voice slots the cue trigger rotates through.
pub const CUE_VOICE_SLOTS: u32 = 4;

/// Base of the rotating voice-slot range (`0x10 ..= 0x13`).
pub const CUE_VOICE_BASE: u32 = 0x10;

/// The two fixed arguments the cue passes to the SFX voice-attr primitive
/// `FUN_80065034` - the same pair the slot-machine reel cue uses.
pub const CUE_FIXED_ARGS: (i32, i32) = (0x3C, 0x40);

/// One resolved cue, as handed to the SFX voice-attr primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxCue {
    /// Voice slot, `CUE_VOICE_BASE + (counter % 4)`.
    pub voice: u32,
    /// The three literal leading arguments (`0, 0, 1` in this overlay).
    pub leading: (i32, i32, i32),
    /// [`CUE_FIXED_ARGS`].
    pub fixed: (i32, i32),
    /// Positional pair; both entries carry the same value, derived from the
    /// party-block word at `0x80084580`.
    pub position: (i32, i32),
}

/// Decode the positional argument out of the party-block word.
///
/// Retail computes `(word << 15) >> 16` with an *arithmetic* right shift,
/// which extracts bits `1..=16` and sign-extends from bit 16 - a halving of
/// the low 17 bits, not a plain `>> 1`.
///
/// PORT: FUN_801d1288 (position decode)
#[inline]
pub fn cue_position(word: u32) -> i32 {
    ((word << 15) as i32) >> 16
}

/// Resolve this frame's cue and advance the rotating counter.
///
/// `counter` is `DAT_801D1AE4`, which retail increments on every call and
/// masks with `3` only when picking the voice, so it is a free-running u32.
///
/// PORT: FUN_801d1288
pub fn sfx_cue(counter: &mut u32, position_word: u32) -> SfxCue {
    let voice = CUE_VOICE_BASE | (*counter & (CUE_VOICE_SLOTS - 1));
    let p = cue_position(position_word);
    *counter = counter.wrapping_add(1);
    SfxCue {
        voice,
        leading: (0, 0, 1),
        fixed: CUE_FIXED_ARGS,
        position: (p, p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boost_flag_bypasses_every_band() {
        assert_eq!(step_scale(100, true), 100);
        assert_eq!(step_scale(1, true), 1);
        assert_eq!(step_scale(-7, true), -7);
    }

    #[test]
    fn the_fast_band_divides_by_five() {
        assert_eq!(step_scale(6, false), 1);
        assert_eq!(step_scale(50, false), 10);
        assert_eq!(step_scale(52, false), 10);
    }

    #[test]
    fn the_middle_band_halves() {
        assert_eq!(step_scale(3, false), 1);
        assert_eq!(step_scale(4, false), 2);
        assert_eq!(step_scale(5, false), 2);
    }

    #[test]
    fn anything_below_three_floors_to_one() {
        assert_eq!(step_scale(2, false), 1);
        assert_eq!(step_scale(0, false), 1);
        assert_eq!(step_scale(-9, false), 1);
    }

    #[test]
    fn the_voice_slot_rotates_over_four() {
        let mut c = 0;
        let got: Vec<u32> = (0..6).map(|_| sfx_cue(&mut c, 0).voice).collect();
        assert_eq!(got, vec![0x10, 0x11, 0x12, 0x13, 0x10, 0x11]);
        assert_eq!(c, 6, "the counter itself keeps counting past the mask");
    }

    #[test]
    fn the_position_pair_is_the_halved_low_word() {
        assert_eq!(cue_position(0), 0);
        assert_eq!(cue_position(4), 2);
        // Bit 16 is the sign of the extracted field.
        assert_eq!(cue_position(0x1_0000), -0x8000);
        // Bits above 16 are discarded by the left shift - but bit 16 is
        // not: it lands on the sign, which is what makes the field signed.
        assert_eq!(cue_position(0xFFFE_0004), 2);
        assert_eq!(cue_position(0xFFFF_0004), -32766);
    }

    #[test]
    fn the_cue_carries_the_fixed_argument_pair() {
        let mut c = 7;
        let cue = sfx_cue(&mut c, 8);
        assert_eq!(cue.voice, 0x13);
        assert_eq!(cue.leading, (0, 0, 1));
        assert_eq!(cue.fixed, CUE_FIXED_ARGS);
        assert_eq!(cue.position, (4, 4));
    }
}
