//! The battle-screen steps a test above this crate drives directly.
//!
//! `legaia-engine-battle-hud`'s tests seat a battle at one phase and step the
//! glide and command state machines a frame at a time, then assert what the
//! HUD reads back; `legaia-engine-save-state`'s tests settle a fight
//! (`finish_battle`) before they round-trip what it banked through a save. Those steps are internal to the battle loop (`World::tick`
//! runs them in its own order) and stay `pub(crate)` / `pub(in crate::world)`
//! for every other caller; this trait re-exposes them under the same names,
//! and only when the `test-drivers` feature is on, which only that crate's
//! `[dev-dependencies]` enables. Rust skips an inherent method the caller
//! cannot see, so `world.step_action_plate_glides()` resolves here once the
//! trait is in scope.

use super::World;
use crate::battle_round::PendingPartyAction;
use legaia_engine_battle_menus::battle_input::BattleCommandInput;
use legaia_engine_battle_vm::battle_action::StepOutcome;

pub trait BattleTestDrivers {
    /// The flag `World::step_action_plate_glides` holds the stepper's return
    /// at while a glide is in flight.
    const GLIDE_BLOCKING_FLAG: u8;
    fn note_action_plate_raise(&mut self, element: u8, mode: u8);
    fn raise_module_roster_panels(&mut self);
    fn start_round_prompt_glide(&mut self);
    fn start_ring_open_glide(&mut self);
    fn step_action_plate_glides(&mut self);
    fn step_commit_log_launch(&mut self);
    fn arm_battle_intro_names(&mut self);
    fn step_battle_intro_names(&mut self);
    fn live_battle_tick(&mut self) -> Option<StepOutcome>;
    fn tick_battle_command(&mut self);
    fn commit_party_command(&mut self, actor: u8, action: PendingPartyAction);
    /// The press the round prompt's gate parked (`ctx[+0x884]`).
    fn command_pad_buffer(&self) -> Option<BattleCommandInput>;
    /// The battle exit's settle (`World::finish_battle`): capture banking,
    /// spoils and the field restore, run once at the end of a fight.
    fn finish_battle(&mut self);
}

impl BattleTestDrivers for World {
    const GLIDE_BLOCKING_FLAG: u8 = World::GLIDE_BLOCKING_FLAG;
    fn note_action_plate_raise(&mut self, element: u8, mode: u8) {
        World::note_action_plate_raise(self, element, mode)
    }
    fn raise_module_roster_panels(&mut self) {
        World::raise_module_roster_panels(self)
    }
    fn start_round_prompt_glide(&mut self) {
        World::start_round_prompt_glide(self)
    }
    fn start_ring_open_glide(&mut self) {
        World::start_ring_open_glide(self)
    }
    fn step_action_plate_glides(&mut self) {
        World::step_action_plate_glides(self)
    }
    fn step_commit_log_launch(&mut self) {
        World::step_commit_log_launch(self)
    }
    fn arm_battle_intro_names(&mut self) {
        World::arm_battle_intro_names(self)
    }
    fn step_battle_intro_names(&mut self) {
        World::step_battle_intro_names(self)
    }
    fn live_battle_tick(&mut self) -> Option<StepOutcome> {
        World::live_battle_tick(self)
    }
    fn tick_battle_command(&mut self) {
        World::tick_battle_command(self)
    }
    fn commit_party_command(&mut self, actor: u8, action: PendingPartyAction) {
        World::commit_party_command(self, actor, action)
    }
    fn command_pad_buffer(&self) -> Option<BattleCommandInput> {
        self.battle.command_pad_buffer
    }
    fn finish_battle(&mut self) {
        World::finish_battle(self)
    }
}
