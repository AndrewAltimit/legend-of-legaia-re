//! The retail 4-slot SFX cue ring, byte-faithful.
//!
//! PORT: FUN_8001698c - the per-frame ring **aging** half.
//! PORT: FUN_80016b6c - the per-frame ring **drain** half.
//!
//! The producers that arm a slot, the mid-frame driver and the per-frame mode
//! handler that sequences the pair.
//! REF: FUN_80035B50, FUN_8004FE5C, FUN_80016444, FUN_80025EEC
//! The libsnd calls a drained cue makes, factored out of [`CueVoicePlan`].
//! REF: FUN_80065034, FUN_800653C8
//!
//! Retail's SFX cue ring is two parallel fixed arrays, not a queue:
//!
//! | Retail | Contents |
//! |---|---|
//! | `DAT_8007B6D8[4]` (`i16`) | the cue id, or `-1` for an empty slot |
//! | `DAT_8007C338[4]` (`i32`) | that slot's countdown, in **vsyncs** |
//!
//! Both are walked once per frame, by two different functions, in a fixed
//! order that the mode handlers pin (`FUN_8001698C` -> `FUN_80016444(1)` ->
//! `FUN_80016B6C`, per the `FUN_80025EEC` default per-frame handler):
//!
//! * **`FUN_8001698C` ages the ring first** (`0x80016AF4 .. 0x80016B54`).
//!   For each slot: a **zero** timer clears the id to `-1`; a non-zero timer
//!   is decremented by the adaptive frame step `DAT_1F800393` and floored at
//!   zero. Note the order - retail stores the possibly-negative difference and
//!   *then* overwrites it with zero, two stores, which is why a slot cannot
//!   skip past zero however large the frame step is.
//! * **`FUN_80016B6C` drains it second** (`0x80016BF8 .. 0x80016E9C`). A slot
//!   is played only when its timer is **exactly zero** *and* its id is still
//!   `>= 0` (`bne v0,zero -> skip`; `bltz s0 -> skip`).
//!
//! The producers sit **between** the two: `FUN_80035B50` (the ring enqueue) is
//! reached from the game-logic phase `FUN_80016444` runs, and it writes the id
//! plus `timer = 0`. So a cue armed on frame `N` is drained by that same
//! frame's `FUN_80016B6C`, and cleared by frame `N+1`'s aging pass.
//!
//! Those three facts compose into the ring's actual contract, which is a
//! **one-shot scheduled delay**: a cue armed with timer `N` is aged for `N`
//! vsyncs' worth of frame steps, plays on exactly the frame its timer first
//! reads zero, and is cleared to `-1` by the *next* frame's aging pass before
//! the drain can see it again. It never repeats, and it is not a FIFO - the
//! producer picks the slot (`FUN_80035B50` round-robins `gp+0x158` over the
//! four).
//!
//! A host that arms cues *before* its per-frame call rather than in the middle
//! of one gets the identical schedule by running the pair rotated - drain,
//! then age. That is what [`crate::SfxScheduler::tick_frame`] does, and its
//! doc comment carries the derivation.
//!
//! Two consequences worth stating because an approximate "queue with a
//! per-cue frame counter" gets both wrong:
//!
//! 1. The decrement is by the **frame step**, not by `1`. At the field
//!    cadence floor of 2 (`docs/subsystems/actor-vm.md`) a cue queued at
//!    `timer = 4` plays after two ticks, not four.
//! 2. The ring has exactly **four** slots and a producer that writes a slot
//!    directly, so a fifth pending cue *replaces* one - it does not extend a
//!    queue.
//!
//! Provenance: read off the disassembly
//! (`see ghidra/scripts/funcs/8001698c.txt`, `80016b6c.txt`) and cross-checked
//! block-for-block against the static-recomp renderings of `func_80016998`
//! (the recomp splits `FUN_8001698C` into a 12-byte head plus this body) and
//! `func_80016B6C`. The two agree instruction for instruction; the split is a
//! function-boundary difference only.
//!
//! The descriptor lookup and the SPU programming that follow a drain hit are
//! [`CueVoicePlan`] below - the data side is `legaia_asset::sfx_table`, and
//! `FUN_80065034` itself is libsnd, out of clean-room scope.

/// Slots in the retail ring (`slti v0,a2,0x4`).
pub const RING_SLOTS: usize = 4;

/// Empty-slot sentinel written by the aging pass (`li t0,-1; sh t0,0(a1)`).
pub const EMPTY: i16 = -1;

/// One ring slot: the cue id plus its countdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueSlot {
    /// `DAT_8007B6D8[i]` - cue id, or [`EMPTY`].
    pub id: i16,
    /// `DAT_8007C338[i]` - vsyncs until the cue plays.
    pub timer: i32,
}

impl CueSlot {
    /// A cleared slot.
    pub const fn empty() -> Self {
        Self {
            id: EMPTY,
            timer: 0,
        }
    }
}

impl Default for CueSlot {
    fn default() -> Self {
        Self::empty()
    }
}

/// The retail cue ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxCueRing {
    slots: [CueSlot; RING_SLOTS],
}

impl SfxCueRing {
    /// A ring with every slot cleared.
    pub const fn new() -> Self {
        Self {
            slots: [CueSlot::empty(); RING_SLOTS],
        }
    }

    /// Read-only view of the slots.
    pub fn slots(&self) -> &[CueSlot; RING_SLOTS] {
        &self.slots
    }

    /// Write a cue into a slot, with a delay in vsyncs.
    ///
    /// Retail producers (`FUN_8004FE5C`, `FUN_80035B50`, the move-power sound
    /// cues) pick the slot themselves and write both cells; this is that
    /// write. A delay of `0` means "play on the next drain".
    pub fn arm(&mut self, slot: usize, id: i16, delay_vsyncs: i32) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.id = id;
            s.timer = delay_vsyncs.max(0);
        }
    }

    /// Clear one slot outright.
    pub fn clear_slot(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            *s = CueSlot::empty();
        }
    }

    /// Clear the whole ring (scene / battle teardown).
    pub fn clear(&mut self) {
        self.slots = [CueSlot::empty(); RING_SLOTS];
    }

    /// The aging pass - `FUN_8001698C`'s ring loop, verbatim.
    ///
    /// `frame_step` is `DAT_1F800393`, the adaptive cadence
    /// (`legaia_engine_vm::actor_tick::FrameCadence`). Call this **before**
    /// [`Self::drain`] each frame; that is the order the mode handlers fix.
    pub fn age(&mut self, frame_step: u8) {
        let dt = i32::from(frame_step);
        for s in &mut self.slots {
            if s.timer == 0 {
                // 0x80016B24: `sh t0,0(a1)` in the branch delay slot.
                s.id = EMPTY;
            } else {
                // 0x80016B34..0x80016B40: subtract, store, then floor at 0.
                let next = s.timer - dt;
                s.timer = if next >= 0 { next } else { 0 };
            }
        }
    }

    /// The drain pass - the slot gate at the head of `FUN_80016B6C`'s loop.
    ///
    /// Returns the slots that fire this frame, in ascending slot order (the
    /// retail walk direction). A slot fires when its timer is exactly zero
    /// and its id is non-negative.
    pub fn drain(&self) -> impl Iterator<Item = (usize, i16)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.timer == 0 && s.id >= 0)
            .map(|(i, s)| (i, s.id))
    }
}

impl Default for SfxCueRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Ids at or above this resolve through the runtime bank rather than the
/// static `DAT_8006F198` table (`slti v0,s0,0x200` at `0x80016C24`).
pub const RUNTIME_BANK_ID_BASE: i16 = 0x200;

/// Per-channel mixer record base (`0x80091508`) and stride.
///
/// `FUN_80016B6C` computes the record address as `0x80091508 + channel * 12`
/// via the `(ch*2 + ch) << 2` chain at `0x80016CD0`. The two VAs the SFX-table
/// doc names - `DAT_80091510` and `DAT_80091513` - are the `+8` and `+0xB`
/// **fields of record 0**, not two byte arrays: the record is 12 bytes wide.
pub const CHANNEL_MIXER_BASE_VA: u32 = 0x8009_1508;
/// Bytes per channel mixer record.
pub const CHANNEL_MIXER_STRIDE: u32 = 12;
/// Offset of the channel level byte inside a mixer record (`lb a1,0x8(s0)`).
pub const CHANNEL_MIXER_LEVEL: u32 = 8;
/// Offset of the channel enable byte (`lb v0,0xb(v1)`); zero skips the cue.
pub const CHANNEL_MIXER_ENABLE: u32 = 0xB;

/// The channel every cue is forced onto while `_DAT_8007BA88` is non-zero
/// (`li s3,0x6` at `0x80016CC8`).
pub const FORCED_CHANNEL: u8 = 6;

/// Base SPU voice for a **sustained** cue run (`lui s0,0x7`, then `sra a0,s0,0x10`
/// -> `7`, stepping by `+1` per voice).
pub const SUSTAINED_VOICE_BASE: u8 = 7;

/// Top SPU voice for a **one-shot** cue run (`li s8,0x17` -> `23`). One-shots
/// allocate *downward* from here: `voice = 23 - cursor`.
pub const ONESHOT_VOICE_TOP: u8 = 23;

/// One voice a drained cue would program through `FUN_80065034`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueVoice {
    /// SPU voice index.
    pub voice: u8,
    /// Channel mixer level (`record[+8]`).
    pub level: i8,
    /// VAB program (`descriptor[+0]`).
    pub program: u8,
    /// Tone / ADSR region (`descriptor[+1] + voice_ordinal`).
    pub tone: i16,
    /// Note level (`descriptor[+2]`).
    pub note: u8,
}

/// What one drained cue asks the SPU for.
///
/// This is the *shape* of `FUN_80016B6C`'s two key-on loops with the libsnd
/// calls factored out. It deliberately stops at "which voices, with which
/// attributes" - `FUN_80065034` / `FUN_800653C8` themselves are SsAPI and
/// replaced wholesale by `engine-audio`'s software SPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueVoicePlan {
    /// `true` for the `descriptor[+3] & 0x20` sustained branch.
    pub sustained: bool,
    /// Voices to key **off** first. One-shots stop each voice immediately
    /// before reprogramming it; the sustained branch releases the whole
    /// previously-held run.
    pub key_off: Vec<u8>,
    /// Voices to program, in retail order.
    pub voices: Vec<CueVoice>,
}

/// Everything the drainer needs that is not in the ring itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueDrainState {
    /// Rolling one-shot voice cursor (`gp+0x4BC`).
    pub oneshot_cursor: u32,
    /// Cursor wrap limit: `3` normally, `1` in game modes `3` and `0x17`
    /// (`s7`, set at `0x80016BAC` from `gp+0x524`).
    pub cursor_limit: u32,
    /// Held sustained-voice count (`gp+0x5D0`).
    pub sustained_held: u32,
    /// `_DAT_8007BA88` - non-zero forces every cue onto [`FORCED_CHANNEL`].
    pub force_channel: bool,
}

impl Default for CueDrainState {
    fn default() -> Self {
        Self {
            oneshot_cursor: 0,
            cursor_limit: 3,
            sustained_held: 0,
            force_channel: false,
        }
    }
}

impl CueDrainState {
    /// The cursor wrap limit for a game mode (`beq v1,s7 / bne v1,v0` pair at
    /// `0x80016BAC`: modes `3` and `0x17` use `1`, everything else `3`).
    pub const fn cursor_limit_for_mode(mode: i16) -> u32 {
        if mode == 3 || mode == 0x17 { 1 } else { 3 }
    }
}

/// Build the voice plan for one drained cue.
///
/// `descriptor` is the 8-byte `legaia_asset::sfx_table` record already
/// resolved for the cue id (static table or runtime bank - the *selection* is
/// `RUNTIME_BANK_ID_BASE`, but both paths land on the same 8-byte shape).
/// `channel_level` / `channel_enabled` come from the mixer record at
/// [`CHANNEL_MIXER_BASE_VA`]`+ channel *` [`CHANNEL_MIXER_STRIDE`].
///
/// Returns `None` when retail would skip the slot: a disabled channel
/// (`record[+0xB] == 0`) or a zero voice count (`descriptor[+3] & 0x1F == 0`).
/// `state` is advanced exactly as retail advances it.
pub fn plan_cue_voices(
    descriptor: &[u8; 8],
    channel_enabled: bool,
    channel_level: i8,
    state: &mut CueDrainState,
) -> Option<CueVoicePlan> {
    // 0x80016CEC: a disabled channel skips the slot before anything else.
    if !channel_enabled {
        return None;
    }
    let flags = descriptor[3];
    let count = u32::from(flags & 0x1F);
    let sustained = flags & 0x20 != 0;

    if sustained {
        // 0x80016DC4: release the previously-held run first, whatever the new
        // count is. Voices 7 .. 7 + held - 1.
        let key_off: Vec<u8> = (0..state.sustained_held)
            .map(|i| SUSTAINED_VOICE_BASE.wrapping_add(i as u8))
            .collect();
        // 0x80016DFC: `andi s4,s4,0x1f; beq s4,zero,0x80016E94` - a zero count
        // releases and stops without keying on. Note what it does *not* do:
        // the held-count write `sw s4,0x5d0(gp)` lives at 0x80016E28, inside
        // the key-on loop, so a zero-count sustained cue leaves the held count
        // **unchanged**. The next sustained cue therefore re-releases the same
        // (already stopped) run. Faithful, and deliberately kept.
        if count == 0 {
            return Some(CueVoicePlan {
                sustained: true,
                key_off,
                voices: Vec::new(),
            });
        }
        state.sustained_held = count;
        let voices = (0..count)
            .map(|i| CueVoice {
                voice: SUSTAINED_VOICE_BASE.wrapping_add(i as u8),
                level: channel_level,
                program: descriptor[0],
                tone: i16::from(descriptor[1]).wrapping_add(i as i16),
                note: descriptor[2],
            })
            .collect();
        return Some(CueVoicePlan {
            sustained: true,
            key_off,
            voices,
        });
    }

    // 0x80016D04: a one-shot with no voices is a complete no-op.
    if count == 0 {
        return None;
    }
    let mut key_off = Vec::with_capacity(count as usize);
    let mut voices = Vec::with_capacity(count as usize);
    for i in 0..count {
        // 0x80016D10: the cursor wraps *before* use, and the comparison is
        // `limit < cursor` - so the cursor legitimately reaches `limit`.
        if state.cursor_limit < state.oneshot_cursor {
            state.oneshot_cursor = 0;
        }
        let voice = ONESHOT_VOICE_TOP.wrapping_sub(state.oneshot_cursor as u8);
        key_off.push(voice);
        voices.push(CueVoice {
            voice,
            level: channel_level,
            program: descriptor[0],
            tone: i16::from(descriptor[1]).wrapping_add(i as i16),
            note: descriptor[2],
        });
        state.oneshot_cursor += 1;
    }
    Some(CueVoicePlan {
        sustained: false,
        key_off,
        voices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_timer_clears_the_slot_on_the_next_aging_pass() {
        let mut r = SfxCueRing::new();
        r.arm(0, 0x11, 0);
        // The drain sees it while the timer is zero.
        assert_eq!(r.drain().collect::<Vec<_>>(), vec![(0, 0x11)]);
        // The following frame's aging pass clears it, so it never repeats.
        r.age(1);
        assert_eq!(r.slots()[0].id, EMPTY);
        assert_eq!(r.drain().count(), 0);
    }

    #[test]
    fn timer_counts_down_by_the_frame_step_not_by_one() {
        let mut r = SfxCueRing::new();
        r.arm(1, 0x20, 4);
        r.age(2);
        assert_eq!(r.slots()[1].timer, 2);
        assert_eq!(r.drain().count(), 0, "not due yet");
        r.age(2);
        assert_eq!(r.slots()[1].timer, 0);
        assert_eq!(r.drain().collect::<Vec<_>>(), vec![(1, 0x20)]);
    }

    #[test]
    fn a_large_frame_step_floors_at_zero_it_never_overshoots() {
        let mut r = SfxCueRing::new();
        r.arm(2, 7, 3);
        r.age(4);
        assert_eq!(r.slots()[2].timer, 0, "0x80016B40 clamps the difference");
        assert_eq!(
            r.drain().collect::<Vec<_>>(),
            vec![(2, 7)],
            "the cue still plays - it cannot be skipped past"
        );
    }

    #[test]
    fn drain_walks_slots_in_ascending_order() {
        let mut r = SfxCueRing::new();
        r.arm(3, 30, 0);
        r.arm(0, 10, 0);
        r.arm(2, 20, 0);
        assert_eq!(
            r.drain().collect::<Vec<_>>(),
            vec![(0, 10), (2, 20), (3, 30)]
        );
    }

    #[test]
    fn negative_ids_are_skipped_by_the_drain() {
        let mut r = SfxCueRing::new();
        r.arm(0, EMPTY, 0);
        assert_eq!(r.drain().count(), 0);
    }

    #[test]
    fn oneshot_voices_walk_down_from_23_and_wrap_at_the_limit() {
        // Voice count 3, no sustained bit, channel 0.
        let d = [5u8, 9, 60, 0x03, 0, 0, 0, 0];
        let mut st = CueDrainState::default();
        let plan = plan_cue_voices(&d, true, 0x40, &mut st).unwrap();
        assert!(!plan.sustained);
        assert_eq!(
            plan.voices.iter().map(|v| v.voice).collect::<Vec<_>>(),
            vec![23, 22, 21]
        );
        // Tone steps with the voice ordinal.
        assert_eq!(
            plan.voices.iter().map(|v| v.tone).collect::<Vec<_>>(),
            vec![9, 10, 11]
        );
        assert_eq!(st.oneshot_cursor, 3);

        // Next cue: cursor 3 is still <= limit 3, so voice 20 comes out
        // before the wrap.
        let d1 = [5u8, 9, 60, 0x02, 0, 0, 0, 0];
        let plan = plan_cue_voices(&d1, true, 0x40, &mut st).unwrap();
        assert_eq!(
            plan.voices.iter().map(|v| v.voice).collect::<Vec<_>>(),
            vec![20, 23],
            "cursor 3 is not > limit 3, so it is used; then it wraps to 0"
        );
    }

    #[test]
    fn battle_and_menu_modes_narrow_the_cursor_limit() {
        assert_eq!(CueDrainState::cursor_limit_for_mode(3), 1);
        assert_eq!(CueDrainState::cursor_limit_for_mode(0x17), 1);
        assert_eq!(CueDrainState::cursor_limit_for_mode(1), 3);
    }

    #[test]
    fn sustained_cues_key_on_from_voice_seven_and_release_the_old_run() {
        let d = [2u8, 40, 60, 0x20 | 0x02, 0, 0, 0, 0];
        let mut st = CueDrainState {
            sustained_held: 3,
            ..Default::default()
        };
        let plan = plan_cue_voices(&d, true, 0x30, &mut st).unwrap();
        assert!(plan.sustained);
        assert_eq!(plan.key_off, vec![7, 8, 9], "the previously-held run");
        assert_eq!(
            plan.voices.iter().map(|v| v.voice).collect::<Vec<_>>(),
            vec![7, 8]
        );
        assert_eq!(st.sustained_held, 2);
        assert_eq!(
            st.oneshot_cursor, 0,
            "the sustained branch never touches the one-shot cursor"
        );
    }

    #[test]
    fn a_disabled_channel_skips_the_slot_entirely() {
        let d = [5u8, 9, 60, 0x03, 0, 0, 0, 0];
        let mut st = CueDrainState::default();
        assert!(plan_cue_voices(&d, false, 0x40, &mut st).is_none());
        assert_eq!(st.oneshot_cursor, 0, "no state advanced");
    }

    #[test]
    fn a_zero_voice_count_is_a_no_op_for_one_shots() {
        let d = [5u8, 9, 60, 0x00, 0, 0, 0, 0];
        let mut st = CueDrainState::default();
        assert!(plan_cue_voices(&d, true, 0x40, &mut st).is_none());
    }

    #[test]
    fn a_zero_count_sustained_cue_still_releases_the_held_run() {
        let d = [5u8, 9, 60, 0x20, 0, 0, 0, 0];
        let mut st = CueDrainState {
            sustained_held: 2,
            ..Default::default()
        };
        let plan = plan_cue_voices(&d, true, 0x40, &mut st).unwrap();
        assert_eq!(plan.key_off, vec![7, 8]);
        assert!(plan.voices.is_empty());
        assert_eq!(
            st.sustained_held, 2,
            "the held count is written inside the key-on loop, which a zero \
             count skips - so it survives the release"
        );
    }
}
