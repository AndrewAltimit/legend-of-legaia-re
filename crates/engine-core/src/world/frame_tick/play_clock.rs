//! The play-time clock: retail's vsync counter, advanced one sim tick at a time.

use super::*;

/// Vsyncs per second of retail's play counter (`0x80084570`); the save
/// screen divides by `3600` for minutes and `216000` for hours.
pub const PLAY_COUNTER_HZ: u8 = 60;

impl World {
    /// Advance the play-time counter by `delta_seconds` whole seconds. The
    /// sub-second remainder ([`FrameClock::play_tick_phase`]) is untouched.
    ///
    /// [`FrameClock::play_tick_phase`]: legaia_engine_vm::frame_clock::FrameClock::play_tick_phase
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.clock.play_time_seconds = self.clock.play_time_seconds.saturating_add(delta_seconds);
    }

    /// Count one sim tick on the play clock.
    ///
    /// Retail's counter is the vsyncs the game ran: the frame pacer adds the
    /// frame step `DAT_1F800393` (vsyncs this game frame took) once per game
    /// frame, gated on the mode's flag bit (`0x80016ED0..0x80016EFC` in
    /// `FUN_80016B6C`; the gate is
    /// `legaia_engine_mode::mode::GameMode::play_counter_runs`). One
    /// `World::tick` is one vsync, so a counted tick is one count, and the
    /// session counts each tick it hands out (`BootSession::claim_tick`),
    /// whichever screen owns it - the pause menu and a movie run under
    /// counting modes too.
    ///
    /// A host that does not run ticks - a backgrounded tab, a minimised
    /// window, a suspended machine - counts nothing, as retail's counter
    /// stands still when the game does not run a frame. Earlier the hosts
    /// read their wall clock and needed a stall guard to say the same.
    ///
    /// REF: FUN_80016B6C
    pub fn count_play_tick(&mut self) {
        let phase = self.clock.play_tick_phase + 1;
        if phase >= PLAY_COUNTER_HZ {
            self.clock.play_tick_phase = 0;
            self.advance_play_time(1);
        } else {
            self.clock.play_tick_phase = phase;
        }
    }
}
