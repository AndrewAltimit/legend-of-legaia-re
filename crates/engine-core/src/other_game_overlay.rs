//! Two small simulation kernels of the PROT 0977 `other_game` overlay - the
//! mode-24 sub-id-5 **arena door/init slot** whose contest settlement is
//! [`crate::muscle_dome::settle_contest`].
//!
//! The overlay's per-frame update drives a set of counters, scales each
//! frame's step through [`step_scale`], and keys one rotating SPU voice
//! through [`arena_voice_cue`]; the visible half is the sprite/decimal HUD in
//! `legaia_engine_ui::other_game_hud`.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_0977_other_game_801d14b0.txt`
//! and `..._801d1288.txt`; ported from the disassembly.
//!
//! # NOT WIRED
//!
//! The overlay's *data* is reachable, and so are its screens: the browser
//! dome page draws PROT 0977's hub screens through
//! `legaia_engine_ui::other_game_hud`. What has no analogue is the one
//! **tick** these two kernels belong to, and it is now identified rather
//! than merely missing.
//!
//! Their caller is `FUN_801CF074` (true VA; the `801c085c` dump is mis-based
//! by `+0xE818`), the contest **score-tally screen** - four count-up lanes
//! that roll pending values into a sink, one [`step_scale`] step per lane
//! per frame with an [`arena_voice_cue`] blip per step, lane 3's sink being
//! the running score tally `_DAT_80084440` that
//! [`crate::muscle_dome::settle_contest`] settles. The screen's layout is
//! decoded (`other_game_hud::HUB_SCORE_TALLY_LABELS` /
//! `score_tally_quads`), so the geometry half is done.
//!
//! What is missing is what the six rows *hold*. Retail fills them from a
//! per-leg score breakdown the port does not compute: the session is a
//! single fight with no course id, no round progression and no per-round
//! scoring, so a tally driven today would be counting invented numbers up.
//! The blocker is the course run, the same one
//! [`crate::muscle_dome::settle_contest`] waits on - not another parser and
//! not a renderer.

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

// REF: FUN_80065034, FUN_80016b6c, FUN_8001ffa4 (the voice-attr primitive,
// the SCUS cue drainer that pins its argument order, and the cold reset that
// seeds the volume word this halves)
/// Number of voice slots the cue trigger rotates through.
pub const CUE_VOICE_SLOTS: u32 = 4;

/// Base of the rotating voice-slot range (`0x10 ..= 0x13`).
pub const CUE_VOICE_BASE: u32 = 0x10;

/// Channel mixer level the cue passes (argument 2). Literal `0` here; the
/// SCUS cue drainer sources the same argument from its cue record.
pub const CUE_LEVEL: i32 = 0;

/// VAB program the cue keys (argument 3). Literal `0` here.
pub const CUE_PROGRAM: i32 = 0;

/// Tone / ADSR region within the program (argument 4). Literal `1` here.
pub const CUE_TONE: i32 = 1;

/// Note the voice is keyed at (argument 5).
pub const CUE_NOTE: i32 = 0x3C;

/// Argument 6, `0x40` at every retail call site of the voice-attr primitive -
/// including the SCUS cue drainer `FUN_80016B6C`.
pub const CUE_ARG6: i32 = 0x40;

/// One resolved voice-attr call, as handed to `FUN_80065034`.
///
/// The retail signature the port follows is
/// `FUN_80065034(voice, level, program, tone, note, 0x40, vol_l, vol_r)`,
/// read off the SCUS cue drainer `FUN_80016B6C`, whose own call fills the
/// same eight slots from a cue descriptor. This overlay's call hard-codes
/// every slot but the voice and the volume pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceAttrCue {
    /// Voice slot, `CUE_VOICE_BASE + (counter % 4)`.
    pub voice: u32,
    /// [`CUE_LEVEL`] / [`CUE_PROGRAM`] / [`CUE_TONE`].
    pub level_program_tone: (i32, i32, i32),
    /// [`CUE_NOTE`] and [`CUE_ARG6`].
    pub note_and_arg6: (i32, i32),
    /// Left / right volume; both entries carry the same value, halved out of
    /// the voice-volume config word ([`cue_volume`]).
    pub volume: (i32, i32),
}

/// Halve the **voice-volume config word** `_DAT_80084580` into the per-channel
/// volume the voice-attr primitive's last two arguments take.
///
/// `_DAT_80084580` is the voice/SFX volume setting, cold-reset to `200` by
/// `FUN_8001FFA4` (see [`crate::new_game::GAME_STATE_COLD_RESET`]) - **not** a
/// party-block coordinate. The SCUS cue drainer `FUN_80016B6C` passes the very
/// same `(_DAT_80084580 << 0xf) >> 0x10` expression into arguments 7 and 8 of
/// the same primitive, which is what pins these two slots as `vol_l` / `vol_r`.
///
/// Retail computes it with an *arithmetic* right shift, so it extracts bits
/// `1..=16` and sign-extends from bit 16 - a halving of the low 17 bits, not a
/// plain `>> 1`.
///
/// PORT: FUN_801d1288 (volume decode)
#[inline]
pub fn cue_volume(word: u32) -> i32 {
    ((word << 15) as i32) >> 16
}

/// Resolve this frame's voice-attr call and advance the rotating counter.
///
/// `counter` is `DAT_801D1AE4`, which retail increments on every call and
/// masks with `3` only when picking the voice, so it is a free-running u32.
///
/// Named `arena_voice_cue` rather than `sfx_cue` on purpose:
/// `MenuInput::sfx_cue` in `crate::menu_input` already holds that name, and a
/// free function sharing a name with anything else is never receiver-gated by
/// the reachability pass - the collision would eventually manufacture a false
/// live edge onto this inert port. See
/// `docs/tooling/stale-not-wired-triage.md`.
///
/// PORT: FUN_801d1288
pub fn arena_voice_cue(counter: &mut u32, volume_word: u32) -> VoiceAttrCue {
    let voice = CUE_VOICE_BASE | (*counter & (CUE_VOICE_SLOTS - 1));
    let v = cue_volume(volume_word);
    *counter = counter.wrapping_add(1);
    VoiceAttrCue {
        voice,
        level_program_tone: (CUE_LEVEL, CUE_PROGRAM, CUE_TONE),
        note_and_arg6: (CUE_NOTE, CUE_ARG6),
        volume: (v, v),
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
        let got: Vec<u32> = (0..6).map(|_| arena_voice_cue(&mut c, 0).voice).collect();
        assert_eq!(got, vec![0x10, 0x11, 0x12, 0x13, 0x10, 0x11]);
        assert_eq!(c, 6, "the counter itself keeps counting past the mask");
    }

    #[test]
    fn the_volume_pair_is_the_halved_low_word() {
        assert_eq!(cue_volume(0), 0);
        assert_eq!(cue_volume(4), 2);
        // Bit 16 is the sign of the extracted field.
        assert_eq!(cue_volume(0x1_0000), -0x8000);
        // Bits above 16 are discarded by the left shift - but bit 16 is
        // not: it lands on the sign, which is what makes the field signed.
        assert_eq!(cue_volume(0xFFFE_0004), 2);
        assert_eq!(cue_volume(0xFFFF_0004), -32766);
    }

    #[test]
    fn the_boot_voice_volume_halves_to_one_hundred() {
        // The word this reads is the voice-volume config `_DAT_80084580`,
        // which the cold reset seeds at 200 - so a freshly booted game keys
        // the arena cue at 100 per channel. This is what settles the slot as
        // a volume rather than a coordinate.
        let boot = crate::new_game::GAME_STATE_COLD_RESET.voice_volume;
        assert_eq!(boot, 200);
        assert_eq!(cue_volume(boot as u32), 100);
    }

    #[test]
    fn the_cue_carries_the_hard_coded_argument_slots() {
        let mut c = 7;
        let cue = arena_voice_cue(&mut c, 8);
        assert_eq!(cue.voice, 0x13);
        assert_eq!(cue.level_program_tone, (CUE_LEVEL, CUE_PROGRAM, CUE_TONE));
        assert_eq!(cue.note_and_arg6, (CUE_NOTE, CUE_ARG6));
        assert_eq!(cue.volume, (4, 4));
    }
}
