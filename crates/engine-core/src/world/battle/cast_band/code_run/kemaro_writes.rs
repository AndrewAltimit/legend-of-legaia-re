//! The seat writes PROT 0918 (Kemaro)'s arms make that its tick body's
//! port (`cast_module_ticks::kemaro_tick`) does not carry: the victim moved
//! onto the stage line and back, who is drawn, and the creature's spin and
//! landing. `s4` is the victim `actor_table[caster + 0x1DD]`, `s3` the
//! creature `actor_table[7]`.

use super::*;

/// The neutral tint word an arm shows a seat with (`lui 0x2008; ori 0x200`).
const SHOWN_TINT: u32 = 0x2008_0200;
/// `+0x21C = 0xFF`: the seat is hidden.
const HIDDEN: u8 = 0xFF;

impl World {
    /// Apply PROT 0918's arm `phase` writes for one pass of its director;
    /// `passed` is whether the arm's gate let it through, `countdown` the
    /// module word after the pass.
    ///
    /// | arm | writes |
    /// |---|---|
    /// | 0 (`0x801F6E48..0x801F6EA0`, pass) | the victim's pose saved, then `x = 0`, `z = -0x800`, facing `0` |
    /// | 1 (`0x801F6F20..0x801F6F30`, every pass) | the victim shown |
    /// | 6 (`0x801F7480..0x801F74A4`, every pass) | the creature's facing `= +0x72 * 6`, the spin it grows in on |
    /// | 8 (`0x801F7690..0x801F76A8`, while the countdown is at or under `scalar << 5`) | the creature at `z = 0x400`, facing `0x800`; its height back to `0` on the pass (`0x801F7748`) |
    /// | 13 (`0x801F7D40..0x801F7D78`, at or under `scalar << 5`; pass `0x801F7DC0..0x801F7DCC`) | the creature walks `scalar / 4` a tick along `z`; on the pass the creature and the victim hidden |
    /// | 14 (`0x801F7E84..0x801F7E90`, pass) | the victim shown |
    /// | 16 (`0x801F81E0`, pass) | the victim hidden |
    /// | `0x12` (`0x801F8698..0x801F86E0`, every pass) | the victim at `z = ctx[+0x6D0] / 8 - 0xA00` |
    /// | `0x13` (`0x801F893C..0x801F89B8`, pass) | the victim's pose restored, clip `0` at rate `8`; shown, or `+0x21C = 2` when it died |
    ///
    /// PORT: overlay_summon_kemaro_0918_801f6c70 (PROT 0918; the victim and creature seat writes of arms 0, 1, 6, 8, 13, 14, 16, 0x12 and 0x13)
    pub(in crate::world::battle::cast_band) fn apply_kemaro_writes(
        &mut self,
        phase: u8,
        passed: bool,
        victim: u8,
    ) {
        let s = legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
        let countdown = self.casting.module_cam.countdown.0;
        let ramp = self.casting.module_cam.ramp;
        let creature = self.casting.summon_actor_slot.map(usize::from);
        let v = usize::from(victim);
        let depth = i32::from(self.battle.camera_frame_height);
        if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
            match phase {
                6 => a.battle.facing_angle = ((ramp * 6) & 0xFFF) as u16,
                8 if countdown <= s << 5 => {
                    a.move_state.world_z = 0x400;
                    a.battle.facing_angle = 0x800;
                }
                13 if countdown <= s << 5 && !passed => {
                    a.move_state.world_z = a.move_state.world_z.wrapping_add((s / 4) as i16);
                }
                _ => {}
            }
            if passed {
                match phase {
                    8 => a.move_state.world_y = 0,
                    13 => {
                        a.battle.render_color = 0;
                        a.battle.render_flag = HIDDEN;
                    }
                    _ => {}
                }
            }
        }
        if phase == 1
            && let Some(a) = self.actors.get_mut(v)
        {
            a.battle.render_flag = 0;
            a.battle.render_color = SHOWN_TINT;
        }
        if phase == 0x12
            && let Some(a) = self.actors.get_mut(v)
        {
            a.move_state.world_z = (depth / 8 - 0xA00) as i16;
        }
        if !passed {
            return;
        }
        match phase {
            0 => {
                self.casting.module_saved_poses.clear();
                if let Some(a) = self.actors.get_mut(v) {
                    self.casting.module_saved_poses.push((
                        v,
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ],
                        a.battle.facing_angle,
                    ));
                    a.move_state.world_x = 0;
                    a.move_state.world_z = -0x800;
                    a.battle.facing_angle = 0;
                }
            }
            13 | 16 => {
                if let Some(a) = self.actors.get_mut(v) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
            }
            14 => {
                if let Some(a) = self.actors.get_mut(v) {
                    a.battle.render_color = SHOWN_TINT;
                    a.battle.render_flag = 0;
                }
            }
            0x13 => {
                let saved = self
                    .casting
                    .module_saved_poses
                    .iter()
                    .find(|p| p.0 == v)
                    .copied();
                if let Some(a) = self.actors.get_mut(v) {
                    if let Some((_, pos, facing)) = saved {
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ] = pos;
                        a.battle.facing_angle = facing;
                    }
                    if a.battle.hp != 0 {
                        a.battle.render_flag = 0;
                    } else {
                        a.battle.render_flag = 2;
                        a.battle.render_color = 0;
                    }
                }
            }
            _ => {}
        }
    }
}
