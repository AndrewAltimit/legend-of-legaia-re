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
    /// | 8 (`0x801F7640..0x801F7674`, while the word is at or under `scalar << 6`) | the creature's height [`kemaro_arm8_height`] |
    /// | 8 (`0x801F7690..0x801F76A8`, while the word is at or under `scalar << 5`) | the creature at `z = 0x400`, facing `0x800`, pitch `+0x44 = (word << 4) & 0xFFF`; height and pitch back to `0` on the pass (`0x801F7748`, `0x801F7750`) |
    /// | 13 (`0x801F7D40..0x801F7D78`, at or under `scalar << 5`; pass `0x801F7DC0..0x801F7DCC`) | the creature walks `scalar / 4` a tick along `z`; on the pass the creature and the victim hidden |
    /// | 14 (`0x801F7E84..0x801F7E90`, pass) | the victim shown |
    /// | 15 (`0x801F8088..0x801F80BC`, pass) | the victim's flinch staged |
    /// | 16 (`0x801F81E0..0x801F825C`, `0x801F8370..0x801F8388`, pass) | the victim on `+0x04 = 0x401004`, `+0x21C = 0xC8`; on the neutral word for ids `0xA8`, `0x53..0x55`; frozen on its flinch (cursor `0x30`, rate `0`, pitch `1`) |
    /// | `0x12` (`0x801F8698..0x801F86E0`, every pass) | the victim at `z = ctx[+0x6D0] / 8 - 0xA00` |
    /// | `0x13` (`0x801F893C..0x801F89B8`, pass) | the victim's pose restored, clip `0` at rate `8`, pitch `0`; shown, or `+0x21C = 2` when it died |
    ///
    /// PORT: overlay_summon_kemaro_0918_801f6c70 (PROT 0918; the victim and creature seat writes of arms 0, 1, 6, 8 (height, pitch), 13, 14, 16, 0x12 and 0x13)
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
        self.kemaro_spear_writes(phase, passed, victim, countdown, depth);
        if let Some(a) = creature.and_then(|c| self.actors.get_mut(c)) {
            match phase {
                6 => a.battle.facing_angle = ((ramp * 6) & 0xFFF) as u16,
                8 if !passed => {
                    // The arm's stores read the word ahead of the pass's
                    // drain (`lw a0,-0x6D1C(v1)` at `0x801F762C`, the drain at
                    // `0x801F7730`).
                    let w = countdown + s;
                    if w <= s << 6 {
                        a.move_state.world_y = kemaro_arm8_height(w);
                    }
                    if w <= s << 5 {
                        a.move_state.world_z = 0x400;
                        a.battle.facing_angle = 0x800;
                        a.battle.render_pitch = ((w << 4) & 0xFFF) as u16;
                    }
                }
                13 if countdown <= s << 5 && !passed => {
                    a.move_state.world_z = a.move_state.world_z.wrapping_add((s / 4) as i16);
                }
                _ => {}
            }
            if passed {
                match phase {
                    8 => {
                        a.move_state.world_y = 0;
                        a.battle.render_pitch = 0;
                    }
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
            // Arm 15's pass stages the victim's flinch (`0x801F8088..0x801F80BC`),
            // the one reaction the cast gives it: arm `0x12`'s hit stages none.
            0xF => {
                let mut view = self.cast_view(victim);
                view.staged_anim = if view.reaction_alt != 0 {
                    view.reaction_alt
                } else {
                    view.reaction_alt2
                };
                view.restage |= 5;
                self.write_cast_view(victim, &view);
            }
            0 => {
                // The hit stages no clip (`0x801F87B0..0x801F8864`).
                self.casting.module_fold_quiet = true;
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
            13 => {
                if let Some(a) = self.actors.get_mut(v) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = HIDDEN;
                }
            }
            // Not a hide: a dark tint on its way back (`+0x21C = 0xC8`), or the
            // neutral word under the raised byte for the four monsters it spares.
            16 => {
                if let Some(a) = self.actors.get_mut(v) {
                    let spared = a
                        .battle_monster_id
                        .is_some_and(|id| matches!(id, 0xA8 | 0x53 | 0x54 | 0x55));
                    if spared {
                        a.battle.render_color = SHOWN_TINT;
                        a.battle.render_flag = HIDDEN;
                    } else {
                        a.battle.render_color = 0x0040_1004;
                        a.battle.render_flag = 0xC8;
                    }
                    // ... and the pass freezes it on its flinch: the node's
                    // cursor `+0x68 = 0x30`, rate `+0x21D = 0`, pitch
                    // `+0x44 = 1` (`0x801F8370..0x801F8388`), until arm
                    // `0x13` puts it back (`0x801F8988`).
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(0);
                    a.battle.render_pitch = 1;
                    if let Some(p) = a.battle_animation.as_mut() {
                        p.seek_sixteenths(0x30);
                    }
                }
            }
            14 => {
                if let Some(a) = self.actors.get_mut(v) {
                    a.battle.render_color = SHOWN_TINT;
                    a.battle.render_flag = 0;
                }
            }
            0x13 => {
                let mut view = self.cast_view(victim);
                view.staged_anim = 0;
                view.anim_rate = 8;
                view.restage = view.restage.wrapping_add(1);
                self.write_cast_view(victim, &view);
                let saved = self
                    .casting
                    .module_saved_poses
                    .iter()
                    .find(|p| p.0 == v)
                    .copied();
                if let Some(a) = self.actors.get_mut(v) {
                    a.battle.render_pitch = 0;
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

/// The three records arm 9 seats on the creature and keeps by handle
/// (`0x801F92CC..0x801F92D4`, `sw v0` at `0x801F78A4..0x801F78C4`): the spear
/// arms 13 to 17 walk down the victim's line.
const SPEAR_RECORDS: [u32; 3] = [0x801F_8ED8, 0x801F_8F48, 0x801F_8FB8];
/// The record arm 7 seats and keeps at `0x801F92C8` (`0x801F7500`).
const ARM7_RECORD: u32 = 0x801F_8E88;

impl World {
    /// The part writes PROT 0918's arms 13..17 make by handle, one module
    /// tick at a time (a pass's `delta` ticks make retail's one
    /// `scalar * delta` store). `countdown` is the module word after this
    /// tick's drain, `depth` the frame height `ctx[+0x6D0]`. Draw only: no
    /// seat, roll or clock reads these parts; the shots of arms 16 and 17
    /// keep the height arm `0xE` stood the spear at
    /// (`ModuleCamState::part_height`), which arm 17 reads ahead of its own
    /// store (`0x801F83A8`).
    ///
    /// | arm | writes on the three spear records |
    /// |---|---|
    /// | 13 (`0x801F7C40..0x801F7CD4`, while the word is at or under `scalar << 5` and the first record's `z` is at least `-0x7FF`) | `z -= (scalar * step) << 5` |
    /// | 13 pass (`0x801F7E04..0x801F7E28`) | `z = 0x1400` |
    /// | 14 (`0x801F7E2C..0x801F7E80`, every pass) | `z = word * 32 - 0xA00` |
    /// | 14 pass (`0x801F7EF0..0x801F7FC8`) | `z = ctx[+0x6D0] / 8 + 0x800`, `y = 4 * victim[+0x3E] / 3` |
    /// | 15, 16 (`0x801F7FCC..0x801F805C`, `0x801F8124..0x801F81B4`, every pass) | `z -= (scalar * step) << 5` |
    /// | 16 pass (`0x801F82D4..0x801F8300`) | `z = -0xB00`; arm 7's record at `y = 0x200` |
    /// | 17 (`0x801F8408..0x801F8524`, its one pass) | `y` the victim's `+0x3E` scaled by its monster id ([`kemaro_spear_height`]) |
    fn kemaro_spear_writes(
        &mut self,
        phase: u8,
        passed: bool,
        victim: u8,
        countdown: i32,
        depth: i32,
    ) {
        let s = legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
        let walk = (s << 5) as i16;
        let body_y = self.module_body_seat(victim).map(|b| i32::from(b.y));
        let monster = self
            .actors
            .get(usize::from(victim))
            .and_then(|a| a.battle_monster_id);
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        let first_z = scene
            .parts
            .iter()
            .find(|p| p.tag == Some(SPEAR_RECORDS[0]))
            .map(|p| p.state.world_z);
        for part in &mut scene.parts {
            let Some(tag) = part.tag else {
                continue;
            };
            let p = &mut part.state;
            if tag == ARM7_RECORD && phase == 16 && passed {
                p.world_y = 0x200;
            }
            if !SPEAR_RECORDS.contains(&tag) {
                continue;
            }
            match phase {
                13 if passed => p.world_z = 0x1400,
                // The test reads the first record alone (`lw a0,0x8(a2)`).
                13 if countdown <= s << 5 && first_z.is_some_and(|z| z >= -0x7FF) => {
                    p.world_z = p.world_z.wrapping_sub(walk);
                }
                14 if passed => {
                    p.world_z = (depth / 8 + 0x800) as i16;
                    if let Some(h) = body_y {
                        p.world_y = (h * 4 / 3) as i16;
                    }
                }
                14 => p.world_z = (countdown * 32 - 0xA00) as i16,
                16 if passed => p.world_z = -0xB00,
                15 | 16 => p.world_z = p.world_z.wrapping_sub(walk),
                17 => {
                    if let Some(h) = body_y {
                        p.world_y = kemaro_spear_height(h, monster);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The spear's height arm 17 stores off the victim's display height `h`
/// (`+0x3E`): `5 h / 4`, or another fraction for the monster ids the switch
/// at `0x801F8444` names (the jump table at the slot-B base `0x801F69D8`,
/// indexed by `id - 1` under `0xA6`). Each divide rounds toward zero
/// (`bgez` / `addiu` / `sra`).
fn kemaro_spear_height(h: i32, monster: Option<u16>) -> i16 {
    let y = match monster.unwrap_or(0) {
        1..=3 | 0x2E..=0x30 => h * 7 / 8,
        0x10..=0x12 | 0x19..=0x1E | 0x3D..=0x3F | 0x59..=0x5B | 0x9F..=0xA1 | 0xA6 => h,
        0x53..=0x55 => h * 9 / 4,
        0x6E..=0x70 => h * 3 / 4,
        _ => h * 5 / 4,
    };
    y as i16
}

/// PROT 0918 arm 8's creature height for the module word `w`, stored while
/// `w <= scalar << 6` (`0x801F7640..0x801F7674`):
/// `-(6 * w + sin(4 * w) / 4)`, the sine the SCUS table entry at byte
/// `w * 8` and the quarter a signed divide (`bgez` / `addiu 3` / `sra 2`).
/// From `-0xC00` at the gate the creature lifts a little, then sinks to the
/// floor as the word drains; the pass stores `0`.
fn kemaro_arm8_height(w: i32) -> i16 {
    let sin = i32::from(legaia_engine_system::trig::sin12((4 * w) as u16));
    (-(6 * w + sin / 4)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gobu Gobu's spear stands at five quarters of its height; the switch's
    /// named ids take their own fractions, each rounded toward zero.
    #[test]
    fn the_spear_height_follows_the_victims_id() {
        assert_eq!(kemaro_spear_height(-215, Some(0x08)), -268);
        assert_eq!(kemaro_spear_height(-215, None), -268);
        assert_eq!(kemaro_spear_height(-215, Some(0x02)), -188);
        assert_eq!(kemaro_spear_height(-215, Some(0x1A)), -215);
        assert_eq!(kemaro_spear_height(-215, Some(0x54)), -483);
        assert_eq!(kemaro_spear_height(-215, Some(0x6F)), -161);
        assert_eq!(kemaro_spear_height(-215, Some(0xA7)), -268);
    }
}
