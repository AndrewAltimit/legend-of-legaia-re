//! The scripted countdown timer: scheduling its flags and the per-frame drain.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Arm the scripted countdown the field VM installs with `0x4C 0xD3`
    /// (`SCHEDULE_TIMED_FLAGS`).
    ///
    /// The three operands are the ones the installer writes into its four
    /// globals (`0x801E2BDC..0x801E2C30`): `ab` is the packed flag word
    /// `_DAT_800845C0` (high half = expiry flag, low half = below-threshold
    /// flag), `cd` is the duration - stored into **both** `_DAT_800845B8`
    /// (the armed word) and `_DAT_800845A0` (the live counter) - and `ef` is
    /// the below-threshold trigger point `_DAT_800845BC`. A zero duration
    /// leaves the timer disarmed, which is what retail's `_DAT_800845B8 != 0`
    /// arm test resolves to.
    ///
    /// Retail also snapshots the play clock into `_DAT_80073ED4` here so the
    /// first drain sees a zero delta; the engine's drain takes its delta from
    /// the retail-frame sub-clock instead, so there is no latch to seed.
    ///
    /// REF: FUN_801DE840 case 0xD sub 3 (the installer)
    pub fn schedule_timed_flags(&mut self, ab: u32, cd: u32, ef: u32) {
        self.battle.escape_timer_flag_word = ab;
        self.battle.escape_timer = legaia_engine_field::escape_timer::EscapeTimer {
            remaining: cd as i32,
            warn_threshold: ef as i32,
            armed: cd != 0,
        };
        self.battle.escape_timer_hud = None;
        // The op ends on the actor's install / remove `FUN_801D596C`: a zero
        // duration retires a live HUD actor, a non-zero one spawns it when
        // none is live, and a live one is otherwise kept with its phase.
        use legaia_engine_field::escape_timer::{CountdownSpawn, countdown_actor_spawn};
        match countdown_actor_spawn(cd as i32, self.battle.escape_timer_actor.is_some()) {
            CountdownSpawn::Retire => self.battle.escape_timer_actor = None,
            CountdownSpawn::Spawn(actor) => self.battle.escape_timer_actor = Some(actor),
            CountdownSpawn::Leave => {}
        }
    }

    /// Whether this frame is one of the ones retail's timed-flag scheduler
    /// sits out. Retail short-circuits on three conditions at
    /// `0x801D2EBC..0x801D2F30`, and a busy frame refreshes the clock latch
    /// without touching the counter:
    ///
    /// 1. `*(_DAT_8007C364 + 0x10) & 0x80000` - the **player actor's engaged
    ///    bit** (`0x801D2EE0..0x801D2EEC`), the one `FUN_801D5B5C` sets on a
    ///    touch/talk and the script runner `FUN_80039B7C` raises on every frame
    ///    it steps a context. The engine reads it as
    ///    [`crate::world::World::dialogue_owns_input`] and the bit itself on
    ///    the player's `move_state`. The wider
    ///    [`crate::world::World::script_context_engages_player`] is not a term:
    ///    it counts every stepped helper record, and with it the `chitei2`
    ///    countdown never ran out on the full-game ladder - whether retail's
    ///    bit stands under those helpers is not captured.
    /// 2. `_DAT_8007B6B4 != 0` (`0x801D2EF4..0x801D2F00`) - the pad hold a map
    ///    install (`0x28`), a kind-0 warp's landing (`0x28`) and a
    ///    conversation's release (`0x10`) leave, which the player tick drains
    ///    by the frame step (`0x801D161C..0x801D1630`).
    /// 3. `_DAT_8007B6B0 > 0` (`0x801D2F08..0x801D2F14`) - the **kind-0 warp
    ///    timer** of the walk-on dispatcher `FUN_801D1EC4`, which counts the
    ///    `0x26` frames between a door-tile crossing and the landing
    ///    ([`crate::world::World::field_warp_in_flight`]). The mode test
    ///    covers the frames where the field is not what is being driven at
    ///    all (a menu, a battle, a minigame).
    fn escape_timer_busy(&self) -> bool {
        let player_bit = self
            .player_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .is_some_and(|a| a.move_state.flags & 0x0008_0000 != 0);
        self.dialogue_owns_input()
            || player_bit
            || self.locomotion.warp.hold != 0
            || self.field_warp_in_flight()
            || !matches!(self.mode, SceneMode::Field | SceneMode::Cutscene)
    }

    /// Drain the scripted countdown one retail frame and fire whichever
    /// system flags the tick reaches, then refresh the HUD readout.
    ///
    /// Retail's `FUN_801D2EBC` is one function that does all three: it
    /// subtracts the play-clock delta from `_DAT_800845A0`, calls
    /// `func_0x8003CE08(flag & 0xFFF)` for the expiry flag at zero (disarming
    /// the timer) and for the below-threshold flag under `_DAT_800845BC`,
    /// then decomposes the remaining count into MM:SS.ff and picks the
    /// readout ink from it. The decomposition and ink are therefore products
    /// of the tick, not of a renderer - [`legaia_engine_battle_loop::battle_state::BattleState::escape_timer_hud`] caches
    /// this frame's.
    ///
    /// The delta is one retail frame per call (the caller gates on
    /// [`legaia_engine_vm::frame_clock::FrameClock::display_frame_step`]); retail reads it as
    /// `_DAT_80084570 - _DAT_80073ED4`, a clock that also advances one step
    /// per display frame.
    ///
    /// REF: FUN_801D2EBC (scheduler + HUD decomposition; the ports are
    /// `legaia_engine_field::escape_timer::EscapeTimer` and `timer_ink`)
    pub(super) fn tick_escape_timer(&mut self) {
        // The countdown runs for exactly as long as its HUD actor lives: the
        // arming op spawns it, and after expiry it holds a zeroed readout
        // for `EXPIRED_HOLD` frames before killing itself.
        if self.battle.escape_timer_actor.is_none() {
            if !self.battle.escape_timer.armed {
                self.battle.escape_timer_hud = None;
                return;
            }
            self.battle.escape_timer_actor =
                Some(legaia_engine_field::escape_timer::EscapeTimerHud::default());
        }
        let busy = self.escape_timer_busy();
        let flag_word = self.battle.escape_timer_flag_word;
        let events = self.battle.escape_timer.tick(1, flag_word, busy);
        if let Some(flag) = events.expiry_flag {
            self.system_flag_set(flag);
        }
        if let Some(flag) = events.warning_flag {
            self.system_flag_set(flag);
        }
        if busy {
            // Retail's busy frame returns before the phase machine and the
            // draw; the port keeps the last readout up.
            return;
        }
        let Some(mut actor) = self.battle.escape_timer_actor else {
            return;
        };
        let frame = actor.step(&mut self.battle.escape_timer, 1);
        if frame.teardown {
            self.battle.escape_timer_actor = None;
            self.battle.escape_timer_hud = None;
            return;
        }
        self.battle.escape_timer_actor = Some(actor);
        let (minutes, seconds, hundredths) = frame.digits;
        self.battle.escape_timer_hud = Some((minutes, seconds, hundredths, frame.ink));
    }
}
