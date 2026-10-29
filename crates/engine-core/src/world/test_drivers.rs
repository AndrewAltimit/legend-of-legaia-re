//! The battle-screen steps a test above this crate drives directly.
//!
//! `legaia-engine-battle-hud`'s tests seat a battle at one phase and step the
//! glide and command state machines a frame at a time, then assert what the
//! HUD reads back; `legaia-engine-save-state`'s tests settle a fight
//! (`finish_battle`) before they round-trip what it banked through a save.
//! Those steps are internal to the battle loop (`World::tick` runs them in
//! its own order) and stay `pub(crate)` / `pub(in crate::world)` for every
//! other caller; this trait re-exposes them under the same names, and only
//! when the `test-drivers` feature is on, which only that crate's
//! `[dev-dependencies]` enables. Rust skips an inherent method the caller
//! cannot see, so `world.live_battle_tick()` resolves here once the trait is
//! in scope. The glide steps that read and write the battle state alone are
//! public `BattleState` methods in `legaia-engine-battle-loop`, so a test
//! steps them as `world.battle.step_action_plate_glides()` without the trait.

use super::World;
use legaia_engine_battle_loop::round_flow::PendingPartyAction;
use legaia_engine_battle_menus::battle_input::BattleCommandInput;
use legaia_engine_battle_vm::battle_action::StepOutcome;

pub trait BattleTestDrivers {
    fn note_action_plate_raise(&mut self, element: u8, mode: u8);
    fn arm_battle_intro_names(&mut self);
    fn live_battle_tick(&mut self) -> Option<StepOutcome>;
    fn tick_battle_command(&mut self);
    fn commit_party_command(&mut self, actor: u8, action: PendingPartyAction);
    /// The press the round prompt's gate parked (`ctx[+0x884]`).
    fn command_pad_buffer(&self) -> Option<BattleCommandInput>;
    /// The battle exit's settle (`World::finish_battle`): capture banking,
    /// spoils and the field restore, run once at the end of a fight.
    fn finish_battle(&mut self);
    /// Arm the end-of-battle sequence off `battle.end`
    /// (`World::begin_battle_end_sequence`).
    fn begin_battle_end_sequence(&mut self);
    /// One frame of the end-of-battle sequence.
    fn tick_battle_end_sequence(&mut self);
    /// The battle Item window's session over the live world.
    fn build_battle_item_session(&self) -> legaia_engine_menus::inventory_use::InventoryUseSession;
    /// One frame of the open battle Item window.
    fn tick_battle_item_menu(&mut self);
}

impl BattleTestDrivers for World {
    fn note_action_plate_raise(&mut self, element: u8, mode: u8) {
        World::note_action_plate_raise(self, element, mode)
    }
    fn arm_battle_intro_names(&mut self) {
        World::arm_battle_intro_names(self)
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
    fn begin_battle_end_sequence(&mut self) {
        World::begin_battle_end_sequence(self)
    }
    fn tick_battle_end_sequence(&mut self) {
        World::tick_battle_end_sequence(self)
    }
    fn build_battle_item_session(&self) -> legaia_engine_menus::inventory_use::InventoryUseSession {
        World::build_battle_item_session(self)
    }
    fn tick_battle_item_menu(&mut self) {
        World::tick_battle_item_menu(self)
    }
}
