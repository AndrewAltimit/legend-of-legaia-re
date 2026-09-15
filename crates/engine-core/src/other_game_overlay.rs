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
//! Every kernel here is now on a live path, which is why this module carries
//! no blanket: a blanket is read unconditionally by every anchor in the file,
//! and there is nothing left for one to assert.
//!
//! [`step_scale`] is reached twice over. `FUN_801D14B0` is not unique to this
//! overlay - the Baka Fighter overlay links **the same 24 instructions** at
//! `FUN_801D6710`, and that copy paces the end-of-match tally
//! ([`crate::baka_fighter::BakaTally::tick`]) both hosts run - and this
//! overlay's own driver [`ScoreTallyRamp::tick`] now calls it as well. The
//! port keeps one implementation of the pair, and it is this one.
//!
//! What the cue does **not** have is a device. [`arena_voice_cue`] resolves the
//! whole voice-attr call retail makes, and the ramp keys one per counted step,
//! but `legaia-engine-audio` exposes no "key this voice with these attributes"
//! entry point - it plays cue ids, pre-decoded clips and sequences - so the
//! resolved cue is carried on [`ScoreTallyStep::cues`] and no host sounds it.
//! That is a missing audio API, not a missing caller.

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
// Wired from both of its retail callers' ports. `FUN_801D6710` is the same
// 24 instructions linked into the Baka overlay - identical opcode for opcode
// and register for register, differing only in the `lui`/`lw` pair that loads
// the bypass flag (`DAT_801D1AB4` here, `DAT_801DBF00` there) and in the
// relocated branch targets - and [`crate::baka_fighter::tally_drain_step`]
// delegates here so the port holds one implementation. The dome side is
// [`ScoreTallyRamp::tick`], which passes each lane's remainder through this
// on the frame it drains. Both hosts reach both.
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
// Reached through [`arena_voice_cue`], which [`ScoreTallyRamp::tick`] calls on
// every counted step. The word it halves is the game state's voice-volume
// setting; with no live mirror of that global the hosts pass its cold reset,
// so the value is retail's boot value rather than a player-set one.
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
// Reached from [`ScoreTallyRamp::tick`], the port of the tally tick
// `FUN_801CF074` that keys this cue once per counted step, on both hosts: the
// native window steps the ramp a frame at a time while the INTERVAL screen is
// up, and the dome page replays it to the screen's own tick.
//
// What the cue reaches is a struct, not a voice. `legaia-engine-audio` has no
// entry point that keys an SPU voice from an explicit
// `(program, tone, note, volume)` set - it plays cue ids through the BGM
// director, pre-decoded XA clips and sequences - so the resolved call is
// carried out on [`ScoreTallyStep::cues`] and both hosts currently drop it.
// The tally screen's audible per-lane "ka-ching" is a different mechanism
// anyway: the hub's INTERVAL arm pre-schedules four cue ids on the staggered
// vsync countdown ([`crate::muscle_dome::HUB_TALLY_CUE_STAGGER`]).
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

// ---------------------------------------------------------------------------
// The score-tally roll-up, `FUN_801CF074`
// ---------------------------------------------------------------------------

/// Lanes the tally roll drains, in the order the routine chains them.
pub const TALLY_LANES: usize = 4;

/// Rows the tally screen draws.
pub const TALLY_ROWS: usize = 6;

/// A lane's fade counter must *exceed* this before the lane starts draining,
/// and is reseeded to it on every draining frame
/// ([`crate::muscle_dome::HUB_TALLY_ROLL_LEAD_TICKS`] is the `0x11` the
/// `slti` compares against; this is the `0x10` the delay slot stores).
pub const LANE_FADE_FULL: i32 = 0x10;

/// Which lane's fade counter each of the six rows takes its brightness from.
///
/// Not one lane per row: the HP total shares lane `0`'s counter and both
/// money rows share lane `3`'s, which is why the screen lights in four steps
/// and not six.
pub const ROW_FADE_LANE: [usize; TALLY_ROWS] = [0, 1, 2, 0, 3, 3];

/// One frame of the tally roll.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScoreTallyStep {
    /// The routine's return word: something is still counting.
    pub rolling: bool,
    /// What the score lane moved into the coin tally this frame.
    pub tally_gain: i32,
    /// The voice-attr cues the drained lanes keyed this frame (retail's
    /// chaining lets at most one lane drain per frame, but the flow does not
    /// forbid more, so this is a list).
    pub cues: Vec<VoiceAttrCue>,
}

/// The between-leg score tally's roll-up state.
///
/// Four lanes drain one after another into two sinks, and the screen's six
/// rows are windows onto them. Retail keeps the whole thing in overlay
/// globals; the port keeps it here so both hosts read one model.
///
/// | field | retail |
/// |---|---|
/// | `fade[0..4]` | `DAT_801D1ABC` / `..1AC0` / `..1AC4` / `..1AB8` |
/// | `pending[0..4]` | `DAT_801D1ACC` / `..1AD0` / `..1AD4` / `..1AAC` |
/// | `hp_accum` | `DAT_801D1AC8` |
/// | `cue_counter` | `DAT_801D1AE4` |
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScoreTallyRamp {
    /// Per-lane fade counter, `0 ..=` [`LANE_FADE_FULL`] once clamped.
    pub fade: [i32; TALLY_LANES],
    /// Per-lane remaining amount, counting down to zero.
    pub pending: [i32; TALLY_LANES],
    /// The HP the first three lanes have drained so far.
    pub hp_accum: i32,
    /// Free-running voice-slot counter.
    pub cue_counter: u32,
}

impl ScoreTallyRamp {
    /// Arm the roll from a finished leg's four lane values.
    ///
    /// The lane order is the drain order, which is
    /// [`crate::muscle_dome::LegScoreRows`]'s own field order: `DAT_801D1ACC`
    /// takes the round lane and `DAT_801D1AD0` the turn lane. The arming
    /// routine computes them in the other order and stores the turn lane
    /// first, which reads as a swap until the two `lui` reloads in between
    /// (`0x801D11E0` and `0x801D11F0`, both re-forming the `0x801D` base into
    /// the register the store then uses) are followed - the operand register
    /// is not the one that held the product.
    ///
    /// PORT: FUN_801d1184 (the store pairing; the lane values are
    /// `muscle_dome::leg_score_rows`)
    pub fn arm(rows: crate::muscle_dome::LegScoreRows) -> Self {
        Self {
            fade: [0; TALLY_LANES],
            pending: [
                rows.round_lane,
                rows.turns_lane,
                rows.outcome_lane,
                rows.score_cell,
            ],
            hp_accum: 0,
            cue_counter: 0,
        }
    }

    /// Advance the roll one frame.
    ///
    /// `frame_delta` is the adaptive frame-skip byte `DAT_1F800393` every
    /// counter step scales by; `boost` is the overlay's bypass flag
    /// `DAT_801D1AB4`, which hands [`step_scale`] its own input unslowed;
    /// `volume_word` is the voice-volume setting `_DAT_80084580` each cue
    /// halves ([`cue_volume`]).
    ///
    /// The chain is what makes the screen roll one row at a time. A lane's
    /// fade counter only advances in a frame where the *previous* lane has
    /// nothing left to drain, so lane `n` cannot start until lane `n - 1` is
    /// empty; and a lane past the fade clamp moves [`step_scale`] of its
    /// remainder per frame and keys a cue on every step. Lanes `0..3` drain
    /// into [`Self::hp_accum`], lane `3` into the caller's coin tally.
    ///
    /// The return word is the one retail's hub arm branches on: it is `1`
    /// unless the frame reached lane `3` with nothing pending, which is the
    /// only assignment of `0` in the routine.
    ///
    /// PORT: FUN_801cf074 (`0x801CF074..0x801CF294`, the simulation half; the
    /// two emitter loops below it are `legaia_engine_ui::other_game_hud`)
    pub fn tick(&mut self, frame_delta: u8, boost: bool, volume_word: u32) -> ScoreTallyStep {
        let dt = i32::from(frame_delta);
        let mut out = ScoreTallyStep {
            rolling: true,
            ..Default::default()
        };
        // Lane 0's counter is the only one that advances unconditionally.
        self.fade[0] += dt;
        for lane in 0..TALLY_LANES {
            if self.fade[lane] <= LANE_FADE_FULL {
                break;
            }
            self.fade[lane] = LANE_FADE_FULL;
            if self.pending[lane] == 0 {
                match self.fade.get_mut(lane + 1) {
                    Some(next) => *next += dt,
                    // Lane 3 empty is the routine's only `0` return.
                    None => out.rolling = false,
                }
                continue;
            }
            let step = step_scale(self.pending[lane], boost);
            self.pending[lane] -= step;
            if lane + 1 == TALLY_LANES {
                out.tally_gain += step;
            } else {
                self.hp_accum += step;
            }
            out.cues
                .push(arena_voice_cue(&mut self.cue_counter, volume_word));
            // A drained lane hands the frame to the next lane's own test.
        }
        out
    }

    /// Nothing is left to count.
    pub fn settled(&self) -> bool {
        self.pending.iter().all(|&p| p == 0)
    }

    /// The six values the screen draws, given the coin tally as it stands.
    ///
    /// Rows `0..3` are the three recovery lanes counting **down**, row `3` is
    /// the HP they have counted **up** into, row `4` is the score lane
    /// counting down and row `5` is the coin tally counting up. Retail reads
    /// them at `0x801CF558` / `0x801CF59C` / `0x801CF5E0` / `0x801CF624` /
    /// `0x801CF668` / `0x801CF6AC`.
    ///
    /// PORT: FUN_801cf074 (`0x801CF510..0x801CF6B8`, the value reads)
    pub fn row_values(&self, tally: i32) -> [i32; TALLY_ROWS] {
        [
            self.pending[0],
            self.pending[1],
            self.pending[2],
            self.hp_accum,
            self.pending[3],
            tally,
        ]
    }

    /// Per-row brightness for a screen fade level of `fade` (`0 ..= 0x80`).
    ///
    /// Retail forms `lane_fade * (fade << 4)` and divides by `128` rounding
    /// toward zero, so a lane at the clamp draws at twice the fade level -
    /// `0x100` at a full screen fade, which the emitter then caps at `0xFF`
    /// (`slti a3,0x100` at `0x801D08FC`). A settled tally is therefore drawn
    /// at full white, not at the half-intensity the other hub screens pass.
    ///
    /// PORT: FUN_801cf074 (`0x801CF298..0x801CF2B8` and its fifteen repeats)
    pub fn row_brightness(&self, fade: i32) -> [i32; TALLY_ROWS] {
        let mul = fade << 4;
        let mut out = [0; TALLY_ROWS];
        for (row, slot) in out.iter_mut().enumerate() {
            let lane = ROW_FADE_LANE[row];
            let n = self.fade[lane] * mul;
            // `bgez v0, +8; addiu v0,v0,0x7f; sra v0,v0,7` - a signed divide
            // by 128 that truncates toward zero.
            *slot = if n >= 0 { n >> 7 } else { (n + 0x7F) >> 7 };
        }
        out
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
