//! The stores PROT 0932 (Meta), 0933 (Terra) and 0934 (Ozma) make on other
//! seats beside their directors: the monster row's seat, colour word and
//! render flag, and the creature's animation rate and tint.
//!
//! See `ghidra/scripts/funcs/overlay_summon_meta_0932_801f6a34.txt`,
//! `overlay_summon_terra_0933_801f6a30.txt` and
//! `overlay_summon_ozma_0934_801f6a40.txt`.

use super::*;
use legaia_engine_battle_vm::battle_anim_rate::AnimRate;

/// The neutral colour word `0x20080200`: each lane at `0x80`.
const WORD_NEUTRAL: u32 = 0x2008_0200;
/// The render flag that parks the tint machine on its word
/// (`FUN_80050120`'s default arm).
const FLAG_PARKED: u8 = 0xFF;

impl World {
    /// `(engine slot, row index)` of each living monster, in seat order.
    fn ra_seru_row(&self, monster_count: u8) -> Vec<(usize, usize)> {
        let map = self.cast_seat_map();
        (0..usize::from(monster_count))
            .filter_map(|i| map.engine(3 + i as u8).map(|e| (usize::from(e), i)))
            .filter(|&(e, _)| self.actors.get(e).is_some_and(|a| a.battle.hp != 0))
            .collect()
    }

    /// The row store `+0x04` / `+0x21C` on every living monster.
    fn ra_seru_row_word(&mut self, monster_count: u8, word: u32, flag: u8) {
        for (e, _) in self.ra_seru_row(monster_count) {
            if let Some(a) = self.actors.get_mut(e) {
                a.battle.render_color = word;
                a.battle.render_flag = flag;
            }
        }
    }

    /// Arm 0's re-seat: each living monster's trio and facing parked, then
    /// `seat` applied to its alternate-family seat `0x80077628 + (count + 3)
    /// * 0x20`, facing `0x800`, at `scale` when the arm halves it.
    fn ra_seru_row_seat(
        &mut self,
        monster_count: u8,
        seat: impl Fn(i16, i16) -> (i16, i16),
        scale: Option<u16>,
    ) {
        self.casting.module_saved_poses.clear();
        for (e, i) in self.ra_seru_row(monster_count) {
            let s = legaia_engine_battle::battle_seats::monster_seat(monster_count, i, true);
            let Some(a) = self.actors.get_mut(e) else {
                continue;
            };
            let m = &mut a.move_state;
            self.casting.module_saved_poses.push((
                e,
                [m.world_x, m.world_y, m.world_z],
                a.battle.facing_angle,
            ));
            (m.world_x, m.world_z) = seat(s.x, s.z);
            a.battle.facing_angle = 0x800;
            if scale.is_some() {
                a.battle.render_scale = scale;
            }
        }
    }

    /// The terminal arm's restore of what [`Self::ra_seru_row_seat`] parked,
    /// for the monsters still alive.
    fn ra_seru_row_restore(&mut self, monster_count: u8, rescale: bool) {
        let row = self.ra_seru_row(monster_count);
        for (e, pos, facing) in self.casting.module_saved_poses.clone() {
            if !row.iter().any(|&(s, _)| s == e) {
                continue;
            }
            if let Some(a) = self.actors.get_mut(e) {
                [
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ] = pos;
                a.battle.facing_angle = facing;
                if rescale {
                    a.battle.render_scale = None;
                }
            }
        }
    }

    /// PROT 0932 (Meta).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6B40..0x801F6C60`) | pass | row parked and re-seated at `x / 4`, `z / 4 + 0xED`, facing `0x800`, scale `0x800`; hidden |
    /// | 1 (`0x801F6CD0`) | every | row hidden |
    /// | 5 (`0x801F7028`) | pass | creature rate `8` |
    /// | `0xA` (`0x801F74F8`) | pass | creature rate `4` |
    /// | `0xB` (`0x801F75E0`, `0x801F7610`) | every; pass | creature rate `4`; `0`, row shown |
    /// | `0xC` (`0x801F76F4`) | pass | creature rate `2`, `z = -0x1C0` |
    /// | `0xD`, `0xF` (`0x801F777C`, `0x801F7910`) | every | creature rate `2` |
    /// | `0xE` (`0x801F78D4`) | pass | creature rate `2` |
    /// | `0x10`, `0x11` (`0x801F7A20`, `0x801F7A3C`) | pass; every | creature rate `4` |
    /// | `0x12` (`0x801F7C54`, `0x801F7C84`) | pass | row flag `0xFF`, rate `2` |
    /// | `0x13` (`0x801F7CC8`) | every | row rate `0` |
    /// | `0x15` (`0x801F7FE0`, `0x801F7FEC`) | pass | row flag `0`, rate `2` |
    /// | `0x16` (`0x801F8318`) | pass | row restored, scale `0x1000` |
    ///
    /// Not carried: the render node's distance-fade radius `+0x58` the
    /// arms halve and restore, and the creature's object-clip row.
    ///
    /// PORT: overlay_summon_meta_0932_801f6a34 (the row and creature rate stores)
    pub(in crate::world::battle::cast_band) fn apply_meta_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        match (phase, passed) {
            (0, true) => {
                self.ra_seru_row_seat(monster_count, |x, z| (x / 4, z / 4 + 0xED), Some(0x800));
                self.ra_seru_row_word(monster_count, 0, FLAG_PARKED);
            }
            (1, _) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            (0xB, true) => self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0),
            (0x12, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = FLAG_PARKED;
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            (0x13, _) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.anim_rate = AnimRate(0);
                    }
                }
            }
            (0x15, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            (0x16, true) => self.ra_seru_row_restore(monster_count, true),
            _ => {}
        }
        let rate = match (phase, passed) {
            (5, true) => Some(8),
            (0xA, true) | (0xB, false) | (0x10, true) | (0x11, _) => Some(4),
            (0xB, true) => Some(0),
            (0xC, true) | (0xD, _) | (0xE, true) | (0xF, _) => Some(2),
            _ => None,
        };
        if rate.is_some() {
            self.casting.module_cam.creature_rate = rate;
        }
        if (phase, passed) == (0xC, true)
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.move_state.world_z = -0x1C0;
        }
    }

    /// PROT 0933 (Terra).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6B20..0x801F6C80`) | pass | backdrop weight `0x1000`; row parked and re-seated at `x / 4`, `z / 4 + 0x500`, facing `0x800`, scale `0x800` |
    /// | 1 (`0x801F6CF4`) | every | row hidden |
    /// | 8 (`0x801F745C`, `0x801F7478`) | pass | creature rate `2`; row shown |
    /// | 9 (`0x801F7590`) | pass | orbit words zeroed |
    /// | `0xA`, `0xC` (`0x801F75E4..`) | every | the row's orbit about `(0, 0x613)` |
    /// | `0xA`, `0xB` (`0x801F7820`, `0x801F7910`) | pass | creature rate `4`, `2` |
    /// | `0xC` (`0x801F7C8C..`) | pass | row rate `0`, back on its seat, facing `0x800` |
    /// | `0xE` (`0x801F7E18`) | pass | creature rate `8` |
    /// | `0x11` | pass | row height `0` |
    /// | `0x13`, `0x14` (`0x801F84CC`) | every | creature `z += 0x20` |
    /// | `0x13` (`0x801F843C..`) | pass | row flag `0`, rate `2`, facing away from `(0, 0x613)` |
    /// | `0x14` (`0x801F8514`) | every | the row's facing `+ 0x20`, rate `2`, on a seat still drawn |
    /// | `0x14` (`0x801F85FC`) | pass | creature hidden |
    /// | `0x15` | pass | row restored, scale `0x1000` |
    ///
    /// PORT: overlay_summon_terra_0933_801f6a30 (the row and creature stores)
    pub(in crate::world::battle::cast_band) fn apply_terra_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        use legaia_engine_cast::cast_module_camera::DELTA_PER_TICK as D;
        const ORBIT_Z: i32 = 0x613;
        let seat_of = |count: u8, i: usize| {
            let s = legaia_engine_battle::battle_seats::monster_seat(count, i, true);
            (s.x / 4, s.z / 4 + 0x500)
        };
        match (phase, passed) {
            (0, true) => {
                self.battle.backdrop_cue =
                    legaia_engine_battle_vm::battle_ground_grid::BACKDROP_CUE_FULL;
                self.ra_seru_row_seat(monster_count, |x, z| (x / 4, z / 4 + 0x500), Some(0x800));
            }
            (1, _) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            (8, true) => self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0),
            (9, true) => self.casting.module_terra_orbit = (0, 0),
            (0x11, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.move_state.world_y = 0;
                    }
                }
            }
            (0x13, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                        a.battle.anim_rate = AnimRate(2);
                        let b = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                            ORBIT_Z as i16,
                            0,
                            a.move_state.world_z,
                            a.move_state.world_x,
                        );
                        a.battle.facing_angle = b.wrapping_add(0x800) & 0xFFF;
                    }
                }
            }
            (0x14, _) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e)
                        && a.battle.render_color != 0
                    {
                        a.battle.facing_angle =
                            a.battle.facing_angle.wrapping_add((0x20 * D) as u16) & 0xFFF;
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            (0x15, true) => self.ra_seru_row_restore(monster_count, true),
            _ => {}
        }
        if matches!(phase, 0xA | 0xC) {
            let (speed, angle) = self.casting.module_terra_orbit;
            let speed = speed + 3 * D;
            let angle = angle + speed;
            self.casting.module_terra_orbit = (speed, angle);
            for (e, i) in self.ra_seru_row(monster_count) {
                let (px, pz) = seat_of(monster_count, i);
                let (px, pz) = (i32::from(px), i32::from(pz) - ORBIT_Z);
                let a0 = i32::from(
                    legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                        pz as i16, px as i16, 0, 0,
                    ),
                );
                let (sin, cos) = legaia_engine_system::trig::trig12(a0 as u16);
                let r = (i32::from(sin) * px / 0x1000).abs() + (i32::from(cos) * pz / 0x1000).abs();
                let (s2, c2) =
                    legaia_engine_system::trig::trig12(((a0 + (angle & 0xFFFF)) & 0xFFF) as u16);
                if let Some(a) = self.actors.get_mut(e) {
                    a.move_state.world_x = (-i32::from(s2) * r / 0x1000) as i16;
                    a.move_state.world_z = (-i32::from(c2) * r / 0x1000 + ORBIT_Z) as i16;
                    a.battle.facing_angle =
                        a.battle.facing_angle.wrapping_add(speed as u16) & 0xFFF;
                    a.move_state.world_y = (-speed) as i16;
                }
            }
        }
        if (phase, passed) == (0xC, true) {
            for (e, i) in self.ra_seru_row(monster_count) {
                let (x, z) = seat_of(monster_count, i);
                if let Some(a) = self.actors.get_mut(e) {
                    a.battle.anim_rate = AnimRate(0);
                    a.move_state.world_x = x;
                    a.move_state.world_z = z;
                    a.battle.facing_angle = 0x800;
                }
            }
        }
        let rate = match (phase, passed) {
            (8, true) | (0xB, true) => Some(2),
            (0xA, true) => Some(4),
            (0xE, true) => Some(8),
            _ => None,
        };
        if rate.is_some() {
            self.casting.module_cam.creature_rate = rate;
        }
        if let Some(a) = self.actors.get_mut(usize::from(creature)) {
            if matches!(phase, 0x13 | 0x14) {
                a.move_state.world_z = a.move_state.world_z.wrapping_add((0x20 * D) as i16);
            }
            if (phase, passed) == (0x14, true) {
                a.battle.render_color = 0;
                a.battle.render_flag = FLAG_PARKED;
            }
        }
    }

    /// PROT 0934 (Ozma).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6B28..0x801F6BCC`) | pass | row parked and re-seated at `x / 2`, `z / 2 - 0x226`, facing `0x800` |
    /// | 1 (`0x801F6C74`) | pass | row shown |
    /// | 6 (`0x801F72B8`) | pass | row hidden |
    /// | 8 (`0x801F76A4`) | pass | creature rate `2` |
    /// | `0xC` (`0x801F79BC`) | pass | creature rate `4` |
    /// | `0xF` (`0x801F7DB8..`) | pass | the backdrop pair's weight `0x1000` |
    /// | `0x10` (`0x801F7E78`) | pass | creature rate `2` |
    /// | `0x11` (`0x801F7F2C`, `0x801F7F30`) | every | creature rate `2`, flag `4` (the blue ease) |
    /// | `0x12` (`0x801F7FC8..0x801F7FE8`) | pass | creature rate `4`, clip `3`'s window `0x22..0x2C` |
    /// | `0x15` (`0x801F85BC`) | pass | row flag `0` |
    /// | `0x19` (`0x801F9184`, `0x801F9188`) | every from word `0x80` | creature flag `2`, `+0x225 = 2`, selector `0` |
    /// | `0x19` (`0x801F91F0..`) | pass | row restored |
    ///
    /// PORT: overlay_summon_ozma_0934_801f6a40 (the row and creature stores)
    pub(in crate::world::battle::cast_band) fn apply_ozma_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        match (phase, passed) {
            (0, true) => {
                self.ra_seru_row_seat(monster_count, |x, z| (x / 2, z / 2 - 0x226), None);
            }
            (1, true) => self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0),
            (6, true) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            (0xF, true) => {
                self.battle.backdrop_cue =
                    legaia_engine_battle_vm::battle_ground_grid::BACKDROP_CUE_FULL;
            }
            (0x15, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                    }
                }
            }
            (0x19, true) => self.ra_seru_row_restore(monster_count, false),
            _ => {}
        }
        let rate = match (phase, passed) {
            (8, true) | (0x10, true) | (0x11, _) => Some(2),
            (0xC, true) | (0x12, true) => Some(4),
            _ => None,
        };
        if rate.is_some() {
            self.casting.module_cam.creature_rate = rate;
        }
        if (phase, passed) == (0x12, true)
            && let Some(p) = self
                .actors
                .get_mut(usize::from(creature))
                .and_then(|a| a.battle_animation.as_mut())
        {
            p.set_loop_frames(0x22, 0x2C);
        }
        let fading = phase == 0x19 && self.casting.module_cam.countdown.0 < 0x81;
        if (phase == 0x11 || fading)
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.battle.render_flag = if fading { 2 } else { 4 };
            if fading {
                a.battle.capture_state = 2;
                a.battle.impact_state = 0;
            }
        }
    }
}
