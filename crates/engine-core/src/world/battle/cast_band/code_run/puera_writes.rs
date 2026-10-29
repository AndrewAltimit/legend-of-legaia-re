//! The seat and part writes PROT 0922 (Puera)'s arms make that its tick
//! body's port (`cast_module_ticks::puera_tick`) does not carry, and its
//! stager as its parts reach it. `s1` is the creature `actor_table[7]`.

use super::*;

/// The neutral tint word an arm shows a seat with (`lui 0x2008; ori 0x200`).
const SHOWN_TINT: u32 = 0x2008_0200;
/// `+0x21C = 0xFF`: the seat is hidden.
const HIDDEN: u8 = 0xFF;

impl World {
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
    /// | 19 (`0x801F81C8..0x801F8210`, while counting; pass `0x801F82B4..0x801F82C0`) | with `w = (scalar << 6) - countdown`, the creature at `y = 17 w / 2 - 0x1100`, `z = 13 w - 0x1100`; on the pass it is hidden and arm 18's ring re-seated as the crystals ([`Self::seat_puera_crystals`]) |
    /// | 20 (every pass, `0x801F85F8..0x801F8690`) | the crystals fall ([`Self::fall_puera_crystals`]) |
    /// | 24 (`0x801F9014..0x801F90A0`, pass) | each live monster back on its saved pose |
    ///
    /// plus the part halts of [`legaia_engine_cast::cast_module_camera::puera_arm_kills`]
    /// on each pass.
    ///
    /// PORT: overlay_summon_puera_0922_801f6a3c (PROT 0922; the monster, creature and part-halt writes of arms 0, 5, 11, 12, 18, 19, 22 and 24, the crystal re-seat of arm 19 and the fall of arm 20)
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
        // Arm 20's fall runs ahead of its gate, on every pass.
        if phase == 20 {
            self.fall_puera_crystals();
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
                self.seat_puera_crystals();
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

    /// PROT 0922 arm 19's pass: arm 18's ring re-seated as the crystals,
    /// each on its ring point at its dealt height and turned by its draws
    /// ([`legaia_engine_cast::cast_module_camera::puera_crystal_seats`], off
    /// the world `rand()` stream).
    fn seat_puera_crystals(&mut self) {
        let seats =
            legaia_engine_cast::cast_module_camera::puera_crystal_seats(|| self.next_rand());
        for (part, seat) in self.casting.puera_ring_parts().zip(seats.iter()) {
            let st = &mut part.state;
            [st.world_x, st.world_y, st.world_z] = seat.pos;
            st.world_y_mirror = seat.pos[1];
            [st.render_24, st.render_26, st.render_28] = seat.rot;
        }
    }

    /// PROT 0922 arm 20, every pass: each crystal drops `(step * scalar) <<
    /// 7` until it lands at `-0x100`; a landing raises the pad's rumble
    /// (`FF / FF`, rates `0x10`, `0x801F8658..0x801F8680`) behind the Battles
    /// vibration row.
    fn fall_puera_crystals(&mut self) {
        let drop = legaia_engine_cast::cast_module_camera::SPEED_SCALAR << 7;
        let mut landed = false;
        for part in self.casting.puera_ring_parts() {
            let st = &mut part.state;
            let (y, hit) =
                legaia_engine_cast::cast_module_camera::puera_crystal_fall(st.world_y, drop);
            st.world_y = y;
            st.world_y_mirror = y;
            landed |= hit;
        }
        if landed && !self.battle_vibration_off() {
            self.rumble
                .battle_write(legaia_engine_system::rumble::BattleRumbleWrite::even(
                    0xFF, 0x10,
                ));
        }
    }
}
