//! The frame-step floor `DAT_8007B9D8` and the retail routines that write it.
//!
//! `FUN_80016B6C` raises the adaptive cadence `DAT_1F800393` to the floor's low
//! byte whenever it measures below it (`lw v1,-0x4628(v1)` / `slt` /
//! `lbu -0x4628` at `0x80017178..0x80017198`); nothing else reads the word
//! for timing. Every writer on the disc, read off the stores (`sw rt,-0x4628`
//! after `lui 0x8008`, and `sw v0,0x6c0(gp)`):
//!
//! | Writer | Site | Value | Engine |
//! |---|---|---|---|
//! | mode init `FUN_8001DCF8` | `0x8001DD2C` (`gp+0x6C0`) | `1` | superseded by each mode's own write below |
//! | field init `FUN_801D6704` | `0x801D6990` (PROT 0897) | `2` | scene entry installs it |
//! | move-VM ext `0x2F` | `0x801D45E4` (PROT 0897) | operand | `ext_set_8007b9d8` |
//! | name entry `FUN_801F03F0` | save `0x801F0468`, hold `0x801F0488`, restore `0x801F09A0` | `1` while open | [`World::open_name_entry`] / [`World::step_name_entry`] |
//! | save screen `FUN_801DC6B4` (PROT 0899) | `0x801DC7B4` / `0x801DC9BC`; save `0x801DD8AC`, restore `0x801DFC30` | `1`, then `2` | not modelled |
//! | fishing init (PROT 0972) | `0x801CF344` | `2` | `PondSession::vsync` (`VENUE_FRAME_STEP`) |
//! | Baka Fighter (PROT 0976) | `0x801CFCC4` / `0x801D0FAC` | `1` / `4` | `BakaFight::frame` (`SECRET_TALLY_FRAME_STEP`) |
//! | hub screens `FUN_801CF870` (PROT 0977) | `0x801CF8E0` | `2` | `HubTimers::tick` (`HUB_FRAME_STEP`) |
//! | battle intro (PROT 0979) | `0x801CFE00` | `3` | not modelled |
//! | dance (PROT 0980) | `0x801CF070` | `3` | `DanceGame::advance_vsync` (`HALL_FRAME_STEP`) |
//! | monster test (PROT 0981) | `0x801CE884` / `0x801CEC1C` | variable | dev mode, not ported |
//!
//! The minigame rows are modelled by their engine sessions, which keep their
//! own clocks rather than this word; the unmodelled rows run in modes whose
//! sessions do the same. The audio ring ages by the display vsync, not by this floor, so
//! none of them moves a cue.
//!
//! REF: FUN_80016B6C, FUN_8001DCF8, FUN_801D6704, FUN_801F03F0

use super::*;

impl World {
    /// Install `floor` as the frame-step floor, and - because the engine runs
    /// the deterministic arm of `FUN_80016B6C`'s resolver, whose result is the
    /// floor itself - as the cadence too. Mirrors the move-VM ext `0x2F`
    /// write. A zero floor reads as `1` (the resolver never steps by zero).
    pub fn set_frame_step_floor(&mut self, floor: u8) {
        let floor = floor.max(1);
        self.clock.frame_step_floor = floor;
        self.clock.frame_step = floor;
    }

    /// Run the game tick at `step` vsyncs a frame, a cadence retail
    /// **measured** rather than installed: `FUN_80016B6C` steps by the longest
    /// of its last sixteen frame times, raised to the floor, so a scene that
    /// costs more than two fields a frame runs at `3` on a floor of `2`
    /// (`koin3`'s dance hall does). The engine's own resolver has no frame
    /// time to measure and keeps the floor; a host standing the engine where
    /// a retail capture stood hands it the capture's step here. Never below
    /// the installed floor, which stays what it was.
    ///
    /// REF: FUN_80016B6C
    pub fn seed_frame_step(&mut self, step: u8) {
        self.clock.frame_step = step.max(self.clock.frame_step_floor).max(1);
    }

    /// Stand the adaptive resolver on a capture's frame-time ring and run it
    /// from here on at `cost` hblanks a frame: `history` is `DAT_80084098`,
    /// `cursor` the write cursor `gp+0x440`, `step` the step the capture was
    /// running at (`0x1F800393`). The step then follows retail's own law -
    /// the longest of the last sixteen samples against the `0xF0` / `0x1FE`
    /// / `0x2D0` rungs, over the floor - so a capture saved a few frames
    /// after a slow stretch falls back to the floor when retail's does
    /// (`garmel_zeto_appears_rising`: ten more frames at `3`, then `2`).
    ///
    /// A harness seat: a play host has no ring to hand over and keeps the
    /// installed step ([`Self::seed_frame_step`]). A scene entry drops it.
    ///
    /// REF: FUN_80016B6C
    pub fn seed_frame_cost_history(
        &mut self,
        history: [i16; 16],
        cursor: usize,
        step: u8,
        cost: i32,
    ) {
        let step = step.max(self.clock.frame_step_floor).max(1);
        self.clock.frame_step = step;
        self.clock.frame_step_telemetry =
            legaia_engine_vm::actor_tick::FrameStepTelemetry::from_state(history, cursor, step);
        self.clock.frame_cost = Some(cost);
        self.clock.pass_wait = Some(step);
        self.clock.next_frame_step = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seeded_step_rides_above_the_floor_and_never_under_it() {
        let mut w = World::new();
        w.set_frame_step_floor(2);
        w.seed_frame_step(3);
        assert_eq!((w.clock.frame_step, w.clock.frame_step_floor), (3, 2));
        w.seed_frame_step(1);
        assert_eq!(w.clock.frame_step, 2);
    }

    /// A ring that holds ten slow samples behind six fast ones runs ten more
    /// game frames at `3` and then falls to the floor; the wait after a frame
    /// is that frame's own step.
    #[test]
    fn a_seeded_ring_lets_the_step_fall_when_the_slow_samples_age_out() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.set_frame_step_floor(2);
        // Newest first from the cursor: six at 450, ten at the clamp.
        let mut history = [700i16; 16];
        let cursor = 6usize;
        for h in history.iter_mut().take(cursor) {
            *h = 450;
        }
        w.seed_frame_cost_history(history, cursor, 3, 450);
        let mut gaps = Vec::new();
        let (mut last, mut steps) = (None, Vec::new());
        for t in 0..60u32 {
            let _ = w.tick();
            if w.clock.game_tick_fired {
                if let Some(l) = last {
                    gaps.push(t - l);
                }
                last = Some(t);
                steps.push(w.clock.frame_step);
            }
        }
        assert_eq!(&steps[..12], &[3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2, 2]);
        assert_eq!(&gaps[..12], &[3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2, 2]);
    }

    #[test]
    fn name_entry_holds_the_floor_at_one_and_hands_it_back() {
        let mut w = World::new();
        w.set_frame_step_floor(2);
        w.open_name_entry(0);
        assert_eq!(w.clock.frame_step_floor, 1);
        assert_eq!(w.clock.frame_step, 1);
        // Force the commit state; the next step closes the screen.
        use legaia_engine_menus::name_entry::NameEntryState;
        w.party.name_entry.as_mut().unwrap().state = NameEntryState::Done;
        w.step_name_entry(legaia_engine_menus::name_entry::NameEntryInput::default());
        assert!(!w.name_entry_active());
        assert_eq!(w.clock.frame_step_floor, 2);
        assert_eq!(w.clock.frame_step, 2);
    }
}
