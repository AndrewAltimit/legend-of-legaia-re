//! Battle arms the retail kernel differential (`psx-retail-exec`) holds to
//! the bytes one arm at a time.
//!
//! Each method is one `World` step a retail routine's arm ports, kept
//! `pub(in crate::world)` for every other caller and re-exposed here under the
//! `test-drivers` feature only, as [`super::test_drivers`] does for the
//! battle-screen steps. The caller seats the world the arm reads and asserts
//! what it wrote; the gate the arm's own caller applies stays with that caller.

use super::World;
pub use super::battle::MeleeApply;

pub trait BattleArmDrivers {
    /// The anim commit's strike-loop reach snap
    /// (`World::commit_reach_snap`, `FUN_8004AD80` `0x8004BF7C..0x8004C118`).
    fn reach_snap(&mut self, slot: usize);
    /// The presentation tick's defeat-fade bookkeeping
    /// (`World::tick_battle_defeat_sink`, `FUN_80050120`
    /// `0x80050384..0x800504E8`): the Jette count, the sink, the lone latch.
    fn defeat_sink(&mut self);
    /// The formation's second cell as the battle reads it (`gp+0x9F5`).
    fn formation_second_cell(&self) -> u8;
    /// The actor pass's Mystic Shield arm (`World::tick_mystic_shield`,
    /// `FUN_8004CE2C` `0x8004D534..0x8004D674`).
    fn mystic_shield_tick(&mut self);
    /// The shield's sphere as the arm left it: draw kind (`+0x56`), scale
    /// (`+0x72`), position, retired - `None` with no shield seated.
    fn mystic_shield_sphere(&self) -> Option<(i16, u16, [i16; 3], bool)>;
    /// Effect scenes the action has seated (the break's burst among them).
    fn action_fx_count(&self) -> usize;
    /// The frame driver's slot-1 backdrop spin
    /// (`World::tick_battle_backdrop_spin`, `FUN_80046A20`
    /// `0x80046D34..0x80046D5C`).
    fn backdrop_spin(&mut self);
    /// The melee resolver's plan for one admitted hit
    /// (`World::melee_hit_plan`, `FUN_801EC3E4` `0x801EC488..0x801EC554`
    /// and the apply gates): `None` on a limb miss, else whether the total
    /// lands.
    fn melee_hit_plan(
        &self,
        attacker: u8,
        target: u8,
        hit: &legaia_engine_battle_vm::battle_action::HitEvent,
        power_run: &[u8; 4],
        event_frames: &[u8; 4],
    ) -> Option<MeleeApply>;
    /// The melee resolver's body with that plan
    /// (`World::land_melee_hit_folded` behind the plain round's fold): the
    /// damage rolled and whether the total landed.
    fn land_melee_hit_planned(
        &mut self,
        attacker: u8,
        target: u8,
        power_byte: u8,
        committed: u8,
        apply: MeleeApply,
    ) -> (u16, bool);
    /// The action SM's applier call for the acting seat
    /// (`World::fold_battle_item_action`, `FUN_801E295C`
    /// `0x801E4108..0x801E4138` into `FUN_800402F4`).
    fn fold_battle_item_action(&mut self, class: u8, tier: u8, target: u8);
}

impl BattleArmDrivers for World {
    fn reach_snap(&mut self, slot: usize) {
        World::commit_reach_snap(self, slot)
    }
    fn defeat_sink(&mut self) {
        World::tick_battle_defeat_sink(self)
    }
    fn formation_second_cell(&self) -> u8 {
        World::battle_formation_cell(self, 1)
    }
    fn mystic_shield_tick(&mut self) {
        World::tick_mystic_shield(self)
    }
    fn mystic_shield_sphere(&self) -> Option<(i16, u16, [i16; 3], bool)> {
        let p = self.casting.mystic_shield_fx.as_ref()?.parts.first()?;
        Some((
            p.state.move_substate,
            p.state.field_72,
            [p.state.world_x, p.state.world_y, p.state.world_z],
            p.finished,
        ))
    }
    fn action_fx_count(&self) -> usize {
        self.casting.active_action_fx.len()
    }
    fn backdrop_spin(&mut self) {
        World::tick_battle_backdrop_spin(self)
    }
    fn melee_hit_plan(
        &self,
        attacker: u8,
        target: u8,
        hit: &legaia_engine_battle_vm::battle_action::HitEvent,
        power_run: &[u8; 4],
        event_frames: &[u8; 4],
    ) -> Option<MeleeApply> {
        World::melee_hit_plan(self, attacker, target, hit, power_run, event_frames)
    }
    fn land_melee_hit_planned(
        &mut self,
        attacker: u8,
        target: u8,
        power_byte: u8,
        committed: u8,
        apply: MeleeApply,
    ) -> (u16, bool) {
        let fold = self.melee_equipment_fold(attacker, committed);
        self.land_melee_hit_folded(attacker, target, power_byte, committed, apply, fold)
    }
    fn fold_battle_item_action(&mut self, class: u8, tier: u8, target: u8) {
        World::fold_battle_item_action(self, class, tier, target)
    }
}
