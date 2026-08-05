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
//! # What the tally screen counts
//!
//! Both kernels belong to one tick: `FUN_801CF074` (true VA; the `801c085c`
//! dump is mis-based by `+0xE818`), the contest **score-tally screen**. It
//! rolls four pending lanes into their sinks, one [`step_scale`] step per
//! lane per frame with an [`arena_voice_cue`] blip per step.
//!
//! The lanes are [`crate::muscle_dome::LegScoreRows`], and they do not all
//! mean the same thing. Three of them - `round * 2`, `min(turns, 8)` and
//! the outcome-table cell, each scaled `× max_hp / 100` - drain into the
//! **same** accumulator `DAT_801D1AC8`, which the hub's restore state then
//! adds to the fighter's HP: they are between-leg healing, not score. Only
//! the fourth, the `(course, round)` score-table cell, drains into the
//! running tally `_DAT_80084440` that [`crate::muscle_dome::settle_contest`]
//! settles into casino coins.
//!
//! So the scoring and the healing are one mechanism, which is why a dome
//! contest costs no permanent HP. The values are computed by
//! [`crate::muscle_dome::leg_score_rows`] and carried by
//! [`crate::muscle_dome::DomeContest`]; the screen's geometry is
//! `other_game_hud::HUB_SCORE_TALLY_LABELS` / `score_tally_quads`.
//!
//! # Wiring status is per item, not per module
//!
//! This module carried a blanket `# NOT WIRED` heading saying that nothing
//! called either kernel. That is no longer true of [`step_scale`], and a
//! blanket is read unconditionally by every anchor in the file, so leaving it
//! would make it assert something false about that one.
//!
//! [`step_scale`] is on the live path. `FUN_801D14B0` is not unique to this
//! overlay: the Baka Fighter overlay links **the same 24 instructions** at
//! `FUN_801D6710`, and that copy paces the end-of-match tally
//! ([`crate::baka_fighter::BakaTally::tick`]) both hosts run. The port keeps
//! one implementation of the pair, and it is this one.
//!
//! What is still absent is this overlay's *own* driver, and with it
//! [`arena_voice_cue`]. See that function's tag for the exact ramp.

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
// REF: FUN_801d6710 (the Baka Fighter overlay's copy of this same routine)
// Wired, but not through this overlay's own tick. `FUN_801D6710` is the same
// 24 instructions linked into the Baka overlay - identical opcode for opcode
// and register for register, differing only in the `lui`/`lw` pair that loads
// the bypass flag (`DAT_801D1AB4` here, `DAT_801DBF00` there) and in the
// relocated branch targets - and [`crate::baka_fighter::tally_drain_step`]
// delegates here so the port holds one implementation. The live caller is
// therefore the *Baka* tally, reached from both hosts; the dome hub's own
// caller `FUN_801CF074` is still unported, so the `boost` argument is only
// ever the Baka fast-forward latch and never `DAT_801D1AB4`.
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
// NOT WIRED: this decode is correct and tested, but its only caller is
// [`arena_voice_cue`] directly below, which is itself inert - so no host root
// reaches this function either. The blocker is that one's, not a second
// independent gap: the tally tick plus the two hosts holding its ramp state.
// Read that tag for the named function and the full shape. Stated separately
// because the
// module's blanket heading was narrowed to the sites it actually described,
// which left this anchor covered by nothing.
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
// NOT WIRED: the missing thing is one named function, `FUN_801CF074` - the
// tally tick itself, which is what turns the settled lane values into a
// per-frame count-up and blips this cue once per counted step. Its shape is
// fully in the disassembly, so wiring is mechanical rather than exploratory:
// four staged lanes, each with a fade accumulator (`DAT_801D1ABC` /
// `..1AC0` / `..1AC4` / `..1AB8`) that advances by the frame delta
// `DAT_1F800393` and clamps at `0x10`; a lane only starts accumulating once the
// previous lane's *pending* word (`DAT_801D1ACC` / `..1AD0` / `..1AD4` /
// `..1AAC`) has emptied; a lane past the clamp moves [`step_scale`] of its
// remainder per frame and fires this cue on every step. Lanes 0..2 drain into
// the HP accumulator `DAT_801D1AC8`, lane 3 into the running tally
// `_DAT_80084440`, and the return word is `1` while anything is still pending.
// Each lane's clamped accumulator doubles as its row brightness.
//
// What that costs is not the tick but the two hosts: both draw the tally at its
// settled values in one frame (`window/minigames.rs`'s `muscle_interval_timer`
// arm and the dome page's `score_tally_quads` call), so each has to hold the
// ramp state and read the counted-up rows instead - and the browser side is a
// change in the page's JavaScript, not only in the wasm surface. Until then a
// port of `FUN_801CF074` would itself be inert, which is why the row is left
// stated rather than half-built.
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
    fn the_baka_tally_drain_rate_is_this_same_kernel() {
        // `FUN_801D6710` (Baka overlay) and `FUN_801D14B0` (this one) are one
        // routine linked twice, so the port keeps one implementation. This is
        // the guard the `sin_4096` incident says was missing when two
        // reproductions of one table disagreed and nothing compared them: if
        // the delegation is ever unwound into a second copy, any drift in a
        // band edge, a divisor or the bypass fails here.
        use crate::baka_fighter::{
            TALLY_DIVISOR_FAST, TALLY_DIVISOR_MID, TALLY_FAST_THRESHOLD, TALLY_SLOW_THRESHOLD,
            tally_drain_step,
        };
        assert_eq!(TALLY_FAST_THRESHOLD + 1, STEP_FAST_MIN, "fast band edge");
        assert_eq!(TALLY_SLOW_THRESHOLD, STEP_MIN_FLOOR, "slow band edge");
        assert_eq!(TALLY_DIVISOR_FAST, 5);
        assert_eq!(TALLY_DIVISOR_MID, 2);
        for v in -40..=400 {
            assert_eq!(tally_drain_step(v, false), step_scale(v, false), "v={v}");
            assert_eq!(tally_drain_step(v, true), step_scale(v, true), "v={v}");
        }
        for v in [i32::MIN + 1, -1_000_000, 1_000_000, i32::MAX] {
            assert_eq!(tally_drain_step(v, false), step_scale(v, false), "v={v}");
        }
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
