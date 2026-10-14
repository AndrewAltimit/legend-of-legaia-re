//! The slot-machine session: enter, exit, tick and its sound routing.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Enter the casino slot-machine minigame on `machine`, suspending the
    /// current scene mode (restored by [`World::exit_slot_machine`]). Like
    /// the dance / fishing / pause-menu suspend contract, the interrupted
    /// field state stays intact underneath.
    pub fn enter_slot_machine(
        &mut self,
        machine: legaia_engine_minigames::slot_machine::SlotMachine,
    ) {
        if self.mode != SceneMode::SlotMachine {
            self.minigames.slot_return_mode = self.mode;
        }
        self.minigames.slot_machine = Some(machine);
        self.mode = SceneMode::SlotMachine;
    }

    /// Leave the slot machine and restore the interrupted mode, committing
    /// the session's final balance into the casino coin bank
    /// ([`legaia_engine_minigame_scenes::minigame_state::MinigameState::casino_coins`] - the retail state-100 assignment
    /// `_DAT_800845A4 = DAT_801d4114`). Returns the session so the host can
    /// read the final state. No-op when the machine isn't active.
    pub fn exit_slot_machine(
        &mut self,
    ) -> Option<legaia_engine_minigames::slot_machine::SlotMachine> {
        if self.mode == SceneMode::SlotMachine {
            self.mode = self.minigames.slot_return_mode;
        }
        let mut machine = self.minigames.slot_machine.take();
        if let Some(m) = machine.as_mut() {
            self.minigames.casino_coins = m.cash_out().max(0) as u32;
            // Leaving mid-spin must not strand the reel motor.
            self.audio
                .sfx_voice_stops
                .push(legaia_engine_minigames::slot_machine::SPIN_VOICE);
        }
        machine
    }

    /// Advance the slot machine one frame, reading this frame's pad:
    ///
    /// - **Idle**: a [`Cross`](input::PadButton::Cross) press charges the
    ///   flat bet (3 coins, 1 in feature modes) and spins - all five
    ///   paylines play on every spin.
    /// - **Spinning**: the spin-up timer runs down on its own.
    /// - **Stopping**: the three reels have a stop button each, read off the
    ///   edge word `_DAT_8007B874` the way the reel SM does it
    ///   (`0x801CF70C..0x801CF7E0`): Square (`0x80`) stops reel 0, Cross
    ///   (`0x40`) reel 1, Circle (`0x20`) reel 2. The three tests are
    ///   independent - each gated on its own reel still running - so one
    ///   frame can stop several reels.
    /// - **Payout**: a [`Cross`] press collects the win into the balance.
    ///
    /// [`Cross`]: input::PadButton::Cross
    ///
    /// PORT: the slot overlay's per-frame driver (`FUN_801cf0d8` reel SM;
    /// the confirmed kernels live in [`legaia_engine_minigames::slot_machine`]).
    pub(in crate::world::frame_tick) fn tick_slot_machine(&mut self) {
        use legaia_engine_minigames::slot_machine::SlotFrameOutcome;
        let packed = legaia_engine_minigames::slot_machine::packed_edges(
            self.input.pad(),
            self.input.pad_prev(),
        );
        let Some(m) = self.minigames.slot_machine.as_mut() else {
            // Mode is SlotMachine but no session installed - drop back.
            self.mode = self.minigames.slot_return_mode;
            return;
        };
        // The cabinet's whole frame (`SlotMachine::frame`), shared with the
        // standalone minigames page.
        match m.frame(packed) {
            SlotFrameOutcome::CashedOut => {
                // State 100's tail: the bank commit and the return warp
                // (`FUN_80026018`), the same pair the Start escape runs.
                self.route_slot_sounds();
                self.exit_slot_machine();
                self.close_minigame_round_trip();
                return;
            }
            // Committed: restore the interrupted mode (the host reads the
            // session out via [`World::exit_slot_machine`]).
            SlotFrameOutcome::Committed => self.mode = self.minigames.slot_return_mode,
            SlotFrameOutcome::Stepped => {}
        }
        self.route_slot_sounds();
    }

    /// Hand the machine's sound writes to the audio side: its ring stores
    /// as [`SfxRingOp::WriteSlot`] (resolved against
    /// [`World::runtime_sfx_bundle`]), its motor voice through the direct
    /// key / release queues both hosts drain.
    ///
    /// [`SfxRingOp::WriteSlot`]: legaia_engine_system::sound_state::SfxRingOp::WriteSlot
    pub(in crate::world::frame_tick) fn route_slot_sounds(&mut self) {
        let Some(m) = self.minigames.slot_machine.as_mut() else {
            return;
        };
        let sounds = m.take_sounds();
        for (slot, id) in sounds.ring {
            self.audio
                .sfx_ring_ops
                .push(legaia_engine_system::sound_state::SfxRingOp::WriteSlot(
                    slot, id,
                ));
        }
        self.audio.sfx_voice_keys.extend(sounds.voice_on);
        self.audio.sfx_voice_stops.extend(sounds.voice_off);
    }
}
