//! The seat and part writes PROT 0922 (Puera)'s arms make that its tick
//! body's port (`cast_module_ticks::puera_tick`) does not carry, and its
//! stager as its parts reach it. `s1` is the creature `actor_table[7]`.

use super::*;

/// The neutral tint word an arm shows a seat with (`lui 0x2008; ori 0x200`).
const SHOWN_TINT: u32 = 0x2008_0200;
/// `+0x21C = 0xFF`: the seat is hidden.
const HIDDEN: u8 = 0xFF;

impl World {
    /// Queue one op-`0x20` call into the resident module's stager, as a
    /// part's program makes it (`MoveVmHostImpl::ext_20`); the module's next
    /// tick runs it. A test seam for the stagers only parts reach.
    pub fn queue_cast_stager_call(&mut self, arm: u8) {
        self.casting
            .module_stager_calls
            .push((arm, Default::default()));
    }

    /// Run the op-`0x20` calls PROT 0922's parts made since its last tick
    /// through its stager `FUN_801F90E4`: arm 0 raises `ctx[+0x278] = 3`.
    ///
    /// REF: FUN_801F90E4
    pub(in crate::world::battle::cast_band) fn run_puera_stager_calls(
        &mut self,
        ctx: &mut legaia_engine_cast::cast_module_ticks::CastModuleCtx,
    ) {
        for (arm, _) in std::mem::take(&mut self.casting.module_stager_calls) {
            legaia_engine_cast::cast_module_ticks::puera_stager(ctx, arm);
        }
    }

    /// Apply PROT 0922's arm `phase` writes for one pass of its director;
    /// `passed` is whether the arm's gate let it through, `before` the
    /// countdown the pass opened on.
    ///
    /// | arm | writes |
    /// |---|---|
    /// | 0 (`0x801F6AE0..0x801F6C1C`, pass) | each live monster: pose saved, moved to half its alternate-family seat less `0x1F4` on `z`, facing `0x800` |
    /// | 11 (every pass while counting) | the creature sinks `2 * scalar` a tick (`0x801F7330..0x801F734C`) |
    /// | 12 (every pass; pass `0x801F7524..0x801F75A0`) | the creature sinks `2 * scalar` a tick (`0x801F74A0..0x801F74B0`); on the pass it is hidden and each live monster shown |
    /// | 18 (`0x801F7FE0..0x801F8020`, pass) | the creature at `(0, -0x1100, -0x1100)`, shown, its next clip staged |
    /// | 19 (`0x801F81C8..0x801F8210`, while counting; pass `0x801F82B4..0x801F82C0`) | with `w = (scalar << 6) - countdown`, the creature at `y = 17 w / 2 - 0x1100`, `z = 13 w - 0x1100`; on the pass it is hidden |
    /// | 24 (`0x801F9014..0x801F90A0`, pass) | each live monster back on its saved pose |
    ///
    /// plus the part halts of [`legaia_engine_cast::cast_module_camera::puera_arm_kills`]
    /// on each pass.
    ///
    /// PORT: overlay_summon_puera_0922_801f6a3c (PROT 0922; the monster, creature and part-halt writes of arms 0, 5, 11, 12, 18, 19, 22 and 24)
    pub(in crate::world::battle::cast_band) fn apply_puera_writes(
        &mut self,
        phase: u8,
        passed: bool,
        before: i32,
        monster_count: u8,
    ) {
        let s = legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
        let monsters = usize::from(monster_count);
        let map = self.cast_seat_map();
        let monster_slots: Vec<(usize, usize)> = (0..monsters)
            .filter_map(|i| map.engine(3 + i as u8).map(|e| (i, usize::from(e))))
            .filter(|&(_, e)| self.actors.get(e).is_some_and(|a| a.battle.hp != 0))
            .collect();
        let creature = self.casting.summon_actor_slot;
        if let Some(a) = creature.and_then(|c| self.actors.get_mut(usize::from(c))) {
            match phase {
                11 if before > 0 => {
                    a.move_state.world_y = a.move_state.world_y.wrapping_sub((2 * s) as i16);
                }
                12 => a.move_state.world_y = a.move_state.world_y.wrapping_sub((2 * s) as i16),
                19 if before > 0 => {
                    let w = (s << 6) - (before - s).max(0);
                    a.move_state.world_y = (17 * w / 2 - 0x1100) as i16;
                    a.move_state.world_z = (13 * w - 0x1100) as i16;
                }
                _ => {}
            }
        }
        if !passed {
            return;
        }
        let kills = legaia_engine_cast::cast_module_camera::puera_arm_kills(phase);
        if !kills.is_empty()
            && let Some(scene) = self.casting.active_summon.as_mut()
        {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| kills.contains(&t)) {
                    part.finished = true;
                }
            }
        }
        match phase {
            0 => {
                self.casting.module_saved_poses.clear();
                for &(i, e) in &monster_slots {
                    let seat =
                        legaia_engine_battle::battle_seats::monster_seat(monsters as u8, i, true);
                    let Some(a) = self.actors.get_mut(e) else {
                        continue;
                    };
                    self.casting.module_saved_poses.push((
                        e,
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ],
                        a.battle.facing_angle,
                    ));
                    a.move_state.world_x = seat.x / 2;
                    a.move_state.world_z = seat.z / 2 - 0x1F4;
                    a.battle.facing_angle = 0x800;
                }
            }
            12 => {
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(usize::from(c))) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
                for &(_, e) in &monster_slots {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_color = SHOWN_TINT;
                        a.battle.render_flag = 0;
                    }
                }
            }
            18 => {
                if let Some(slot) = creature {
                    let mut v = self.cast_view(slot);
                    v.render_flag = 0;
                    v.present_04 = 0;
                    let next = v.staged_anim.wrapping_add(1);
                    legaia_engine_cast::cast_module_ticks::stage_clip(&mut v, next);
                    self.write_cast_view(slot, &v);
                    if let Some(a) = self.actors.get_mut(usize::from(slot)) {
                        a.move_state.world_x = 0;
                        a.move_state.world_y = -0x1100;
                        a.move_state.world_z = -0x1100;
                    }
                }
            }
            19 => {
                if let Some(a) = creature.and_then(|c| self.actors.get_mut(usize::from(c))) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
            }
            24 => {
                for &(slot, pos, facing) in &self.casting.module_saved_poses {
                    if !monster_slots.iter().any(|&(_, e)| e == slot) {
                        continue;
                    }
                    if let Some(a) = self.actors.get_mut(slot) {
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ] = pos;
                        a.battle.facing_angle = facing;
                    }
                }
            }
            _ => {}
        }
    }
}
