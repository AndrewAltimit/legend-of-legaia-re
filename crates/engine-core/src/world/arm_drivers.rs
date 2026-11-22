//! Battle arms the retail kernel differential (`psx-retail-exec`) holds to
//! the bytes one arm at a time.
//!
//! Each method is one `World` step a retail routine's arm ports, kept
//! `pub(in crate::world)` for every other caller and re-exposed here under the
//! `test-drivers` feature only, as [`super::test_drivers`] does for the
//! battle-screen steps. The caller seats the world the arm reads and asserts
//! what it wrote; the gate the arm's own caller applies stays with that caller.
//!
//! Closed to new methods: a new driver is a `#[cfg(feature = "test-drivers")]`
//! `impl World` block in the module of the arm it drives, so tests for different
//! arms never edit this file.

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
    /// The anim tick's Rage arm on one seat (`World::stamp_rage_delegation`,
    /// `FUN_80047430` `0x80047F10..0x80047F98`).
    fn rage_delegation(&mut self, slot: usize);
    /// The anim tick's frozen-idle gate (`World::battle_seat_frozen_idle`,
    /// `FUN_80047430` `0x80047638..0x80047658`).
    fn frozen_idle(&self, slot: usize) -> bool;
    /// The anim tick's Slowed-idle gate (`World::battle_seat_slowed_idle`,
    /// `FUN_80047430` `0x800476D8..0x800476F4`).
    fn slowed_idle(&self, slot: usize) -> bool;
    /// The melee's angle term state `0x14` leaves at `ctx[+0x6D2]`
    /// (`World::track_block_approach_terms` after an `AttackFace` pass,
    /// `FUN_801E295C` `0x801E308C..0x801E30C8`), off the acting seat's and
    /// its target's facings as the pass left them.
    fn face_angle_term(&mut self) -> i16;
    /// Water Hazard's arm-3 settle walk over the seats the caster's target
    /// code names (`World::capture_target_settled`, PROT 0956
    /// `0x801F7C2C..0x801F7DA4`).
    fn capture_target_settled(&self, caster: u8) -> bool;
    /// PROT 0922 (Puera)'s monster, creature and part writes for one pass of
    /// its director (`World::apply_puera_writes`).
    fn puera_writes(&mut self, phase: u8, passed: bool, before: i32, monster_count: u8);
    /// The commit confirm's Reselect (`World::reselect_battle_commands`,
    /// `FUN_801D0748` `0x801D3054..0x801D30E0`): `actor` raised the screen.
    fn reselect_battle_commands(&mut self, actor: u8);
    /// The element spawner's raise / unload of the message-banner elements
    /// (`World::message_banner_ui_element`, `FUN_801D8DE8` element `0x59`'s
    /// compose under `mode == 0`).
    fn message_banner_ui_element(&mut self, element: u8, mode: u8);
    /// The anim tick's in-range back-off with the range poll given
    /// (`World::drive_in_range_backoff_with`, `FUN_80047430`
    /// `0x80047C3C..0x80047D1C`); the frame's carry starts fresh.
    fn in_range_backoff(&mut self, slot: usize, dt: u8, range: u16);
    /// The signed root-motion term with the range poll given
    /// (`World::drive_playing_root_motion_with`, `0x80047D20..0x80047E18`);
    /// the frame's carry starts fresh.
    fn playing_root_motion(&mut self, slot: usize, dt: u8, range: u16);
    /// The walk gates (`World::battle_node_walk_gates`): `(hits and effect
    /// script, cues)`.
    fn node_walk_gates(&self) -> (bool, bool);
    /// The Seru-absorb puff behind a natural end
    /// (`World::absorb_puff_plan`, `0x80047B5C..0x80047BC0`): its position
    /// and whether its cue sounds.
    fn absorb_puff(&self, slot: usize) -> Option<([i16; 3], bool)>;
    /// The zero-colour skip (`World::battle_node_tick_skipped`,
    /// `0x80047484..0x800474E0`).
    fn node_tick_skipped(&self, slot: usize) -> bool;
    /// The loop re-zero gate (`World::loop_rewind_rezeroes`,
    /// `0x80047840..0x80047878`).
    fn loop_rewind_rezeroes(&self, slot: usize) -> bool;
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
    fn rage_delegation(&mut self, slot: usize) {
        World::stamp_rage_delegation(self, slot)
    }
    fn frozen_idle(&self, slot: usize) -> bool {
        World::battle_seat_frozen_idle(self, slot)
    }
    fn slowed_idle(&self, slot: usize) -> bool {
        World::battle_seat_slowed_idle(self, slot)
    }
    fn face_angle_term(&mut self) -> i16 {
        use legaia_engine_battle_vm::battle_action::{ActionState, StepOutcome};
        World::track_block_approach_terms(
            self,
            ActionState::AttackFace.as_byte(),
            &StepOutcome::Stay,
        );
        self.battle.attack_ramp
    }
    fn capture_target_settled(&self, caster: u8) -> bool {
        World::capture_target_settled(self, caster)
    }
    fn puera_writes(&mut self, phase: u8, passed: bool, before: i32, monster_count: u8) {
        World::apply_puera_writes(self, phase, passed, before, monster_count)
    }
    fn reselect_battle_commands(&mut self, actor: u8) {
        World::reselect_battle_commands(self, actor)
    }
    fn message_banner_ui_element(&mut self, element: u8, mode: u8) {
        World::message_banner_ui_element(self, element, mode)
    }
    fn in_range_backoff(&mut self, slot: usize, dt: u8, range: u16) {
        if let Some(a) = self.actors.get_mut(slot) {
            a.battle.backoff_carry = Default::default();
        }
        World::drive_in_range_backoff_with(self, slot, dt, Some(range))
    }
    fn playing_root_motion(&mut self, slot: usize, dt: u8, range: u16) {
        if let Some(a) = self.actors.get_mut(slot) {
            a.battle.root_carry = Default::default();
        }
        World::drive_playing_root_motion_with(self, slot, dt, Some(range))
    }
    fn node_walk_gates(&self) -> (bool, bool) {
        let g = World::battle_node_walk_gates(self);
        (g.hits_and_effects, g.cues)
    }
    fn absorb_puff(&self, slot: usize) -> Option<([i16; 3], bool)> {
        World::absorb_puff_plan(self, slot)
    }
    fn node_tick_skipped(&self, slot: usize) -> bool {
        World::battle_node_tick_skipped(self, slot)
    }
    fn loop_rewind_rezeroes(&self, slot: usize) -> bool {
        World::loop_rewind_rezeroes(self, slot)
    }
}
