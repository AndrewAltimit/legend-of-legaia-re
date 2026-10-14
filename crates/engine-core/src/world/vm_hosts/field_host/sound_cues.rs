//! Field-host bodies for the BGM op and the op-`0x36` sound-cue op.

use super::*;

impl FieldHostImpl<'_> {
    pub(super) fn bgm(&mut self, text_id: u16, sub_op: u8) {
        // Sub-ops 1 (start) and 9 (start behind a load barrier) are the only
        // writers of the track id `_DAT_8007BAC8` (`0x801E012C`,
        // `0x801E0254`). The other sub-ops are control words - pause (2, 3),
        // re-attach (4, `0x801E0180`), volume, commit - that leave the id
        // alone, so none of them clears `current_bgm` either.
        // Pause bit 1 of `_DAT_8007B750`: raised by 2 / 3, ended by every
        // start, the re-attach and the commit.
        match sub_op {
            2 | 3 => self.world.audio.bgm_script_paused = true,
            1 | 9 | 4 | 0xA => self.world.audio.bgm_script_paused = false,
            _ => {}
        }
        if sub_op == 1 || sub_op == 9 {
            self.world.audio.current_bgm = Some(text_id);
            // Sub-op 9 also raises the script-owned start bit 0 of
            // `_DAT_8007B750` (`0x801E0260`): the poller holds the outgoing
            // track in the slot until the sub-op 0xA commit.
            if sub_op == 9 {
                self.world.audio.start_pending_commit = true;
            }
        } else if sub_op == 0xA {
            // The commit (`0x801E0264`) sets the release-ack bit 4, on which
            // the poller installs the incoming track and clears bit 0
            // (`0x8002472C`).
            self.world.audio.start_pending_commit = false;
        } else if sub_op == 7 {
            // Sub-7 stores the next fight's battle sound set `_DAT_8007B880`
            // and makes no sequencer call: `lbu v1,0x1(s6)` tests the
            // operand's low byte against `0xFF` (store `-1`), else the u16
            // operand is stored (`0x801E01DC..0x801E0208`).
            self.world.audio.battle_sound_set = if text_id & 0xFF == 0xFF {
                -1
            } else {
                i32::from(text_id)
            };
        } else if sub_op == 5 {
            // Sub-5 is the timed release: retail's handler is
            // `FUN_800267A8(0, s16_operand)` at `0x801E01B4` (the operand is
            // the same `FUN_8003CE9C` read every other sub-op takes), which
            // arms the sound-source auto-release for `operand` vsyncs. The
            // frame-begin tick (`FUN_800267FC`) counts it down and raises
            // `pending_sound_release` on expiry.
            // PORT: FUN_800267A8 (live wiring; the arm record itself is
            // `legaia_engine_system::scus_leaf_kernels::TimedSoundArm`)
            self.world.arm_sound_release(i32::from(text_id));
        }
        // Free-roam picker staging: drop an ENTRY-WINDOW pause. A scene's
        // entry script may park the just-started BGM for a story moment the
        // choreography repairs (town01 `P1[0]` `+0x61`: silent dawn while
        // flag 0x225 is clear, restarted by the opening records' sub-9
        // starts). A picker visit runs no choreography, so the pause would
        // hold forever - the "music dies a second into the scene" report.
        // Pauses issued after the window (dialog / cutscene beats the
        // player triggers) route normally.
        if sub_op == 2
            && self.world.field_vm.free_roam_staging
            && self
                .world
                .clock
                .display_frames
                .saturating_sub(self.world.field_vm.free_roam_entry_frame)
                < crate::world::FREE_ROAM_ENTRY_PAUSE_WINDOW
        {
            return;
        }
        self.world
            .pending_field_events
            .push(FieldEvent::Bgm { text_id, sub_op });
    }

    pub(super) fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        // Op `0x36` is the sound-cue op, not a fade (the `SceneFade` event
        // name predates the handler being read): retail decodes two s16
        // operands and, when bit 15 of the first is set, runs a five-way
        // sub-switch on its low bits. Two of those sub-ops are the pending-cue
        // ring's producer pair, and the engine models both.
        //
        // * sub-`0` - `FUN_80035B50(arg)` enqueues the cue, parks its slot at
        //   `gp+0x15A` and zeroes that slot's delay word.
        // * sub-`4` - `FUN_80035BAC(arg)` stores `arg` as the parked slot's
        //   delay, i.e. schedules the cue instead of firing it.
        //
        // Two gates ride on top of the sub-switch, both read off
        // `0x801E030C..0x801E0444` and both keyed on the side-band
        // request/acknowledge pair `_DAT_8007BABC` / `_DAT_8007BAA0`
        // (`World::audio.sound_stream`) plus the dev/dual-mode word
        // `_DAT_8007B868` (`World::audio.dual_mode_gate`, `0` in retail):
        //
        // * bit-15 **set**: `_DAT_8007B868 != 0` skips the whole sub-switch
        //   (`bnez v0,0x801DF898` at `0x801E031C`). Subs `0`, `1` and `2`
        //   then consult the pair; subs `3` and `4` do not.
        // * bit-15 **clear**: the pair is a *precondition* on the XA arm
        //   (`bne a0,v0,0x801DEE4C` at `0x801E040C`) which
        //   `_DAT_8007B868 != 0` bypasses (`bnez v0,0x801E0414`).
        //
        // The doc this port was written from said the gate covered subs
        // `0`/`2`/`3` and stopped at the bit-15-set arm; the bytes say subs
        // `0`/`1`/`2` and the bit-15-clear arm, with sub `3` ungated.
        //
        // Both producer calls also cross to the host's audio ring as
        // `SfxRingOp`s (`World::take_sfx_ring_ops`), which is what sounds the
        // cue; the pair above is the engine-core mirror the gates read.
        //
        // PORT: FUN_80035BAC (live wiring; the table itself is
        // `legaia_engine_system::scus_leaf_kernels::SfxCueDelays`)
        // REF: FUN_80035B50
        // REF: FUN_800243F0 (the driver that settles the pair)
        let dev_gate = self.world.audio.dual_mode_gate != 0;
        if op0_word & 0x8000 != 0 {
            if dev_gate {
                // Retail never reaches this: the arm is skipped whole and
                // the op advances.
                self.world
                    .pending_field_events
                    .push(FieldEvent::SceneFade { op0_word, op1_word });
                return SceneFadeResult::Done;
            }
            match op0_word & 0x7FFF {
                0 => {
                    if !self.world.audio.sound_stream.is_settled() {
                        return SceneFadeResult::Busy;
                    }
                    // The enqueue writes the slot the cursor names, parks it,
                    // then advances the cursor - so the parked slot stays the
                    // written one until the next enqueue.
                    let slot = self.world.audio.sfx_cue_cursor;
                    self.world.audio.sfx_cue_cursor = self.world.audio.sfx_cue_delays.park(slot);
                    self.world.audio.sfx_parked_slot = slot;
                    // `jal 0x80035B50` with `a0 = (s16)op1_word`
                    // (`0x801E0344..0x801E034C`): the cue id goes to the
                    // host ring.
                    self.world.audio.sfx_ring_ops.push(
                        legaia_engine_system::sound_state::SfxRingOp::Push(op1_word as i16),
                    );
                }
                1 => {
                    if !self
                        .world
                        .audio
                        .sound_stream
                        .request(i32::from(op1_word as i16))
                    {
                        return SceneFadeResult::Busy;
                    }
                    // Synchronous host: the driver's latch lands in the same
                    // call, so a following sub-`2` barrier is satisfied on
                    // arrival.
                    self.world.audio.sound_stream.settle();
                }
                2 => {
                    if !self.world.audio.sound_stream.is_settled() {
                        return SceneFadeResult::Busy;
                    }
                }
                4 => {
                    let parked = self.world.audio.sfx_parked_slot;
                    self.world
                        .audio
                        .sfx_cue_delays
                        .set_delay(parked, op1_word as i16);
                    self.world.audio.sfx_ring_ops.push(
                        legaia_engine_system::sound_state::SfxRingOp::SetLastDelay(op1_word as i16),
                    );
                }
                // Sub `3` (`FUN_801D8450`) is ungated: it stops the top two
                // voices, closes VAB slot 6 and clears the field-bank latch,
                // so the next field init reloads PROT 0876.
                3 => self.world.release_field_audio(),
                // Every sub `>= 5` advances unconditionally.
                _ => {}
            }
        } else if !dev_gate && !self.world.audio.sound_stream.is_settled() {
            return SceneFadeResult::Busy;
        } else if op0_word & 0x7FFF != 0 {
            // The CD-XA arm (`0x801E0420`): `FUN_8003D53C(arg >> 3, arg & 7,
            // sel)`, `arg` the second s16 operand. It lands on the field XA
            // queue both hosts drain (`World::push_field_xa_cue`).
            //
            // `sel & 0x7FFF == 0` is `FUN_80019794(arg >> 3)` instead - a
            // seek-ahead that issues no read (`crate::world::field_xa`), which
            // an engine with no drive completes on arrival.
            // REF: FUN_8003D53C, FUN_80019794
            let arg = op1_word as i16;
            self.world
                .push_field_xa_cue((arg >> 3) as u8, (arg & 7) as u8, op0_word);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SceneFade { op0_word, op1_word });
        SceneFadeResult::Done
    }
}
